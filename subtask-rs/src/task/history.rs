//! Event sourcing for task history
//!
//! The history.jsonl file is the source of truth for task state.
//! Each line is a JSON event that can be replayed to reconstruct state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use super::{task_dir, Task, TaskStatus, WorkerStatus, WorkflowStage};
use crate::error::{validate_task_name, Result, SubtaskError};

/// An event in the task history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// When the event occurred
    pub timestamp: DateTime<Utc>,

    /// The type of event
    #[serde(flatten)]
    pub kind: EventKind,
}

/// Types of events that can occur in a task's lifecycle
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    /// Task was created
    TaskCreated {
        title: String,
        base_branch: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        follow_up: Option<String>,
    },

    /// Description was updated
    DescriptionUpdated { description: String },

    /// Workspace was assigned
    WorkspaceAssigned { path: String },

    /// Workspace was released
    WorkspaceReleased,

    /// Message was sent to worker
    MessageSent {
        prompt: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },

    /// Worker started executing
    WorkerStarted {
        session_id: String,
    },

    /// Worker finished with a reply
    WorkerReplied {
        reply: String,
        tool_calls: Option<u32>,
        duration_secs: Option<f64>,
    },

    /// Worker encountered an error
    WorkerError { error: String },

    /// Worker was interrupted
    WorkerInterrupted,

    /// Workflow stage changed
    StageChanged {
        from: WorkflowStage,
        to: WorkflowStage,
    },

    /// Task was merged
    TaskMerged {
        commit: String,
        message: String,
    },

    /// Task was closed without merging
    TaskClosed {
        #[serde(default)]
        abandoned: bool,
    },

    /// Task was reopened (from merged or closed)
    TaskReopened,
}

impl Event {
    /// Creates a new event with the current timestamp
    fn new(kind: EventKind) -> Self {
        Event {
            timestamp: Utc::now(),
            kind,
        }
    }

    /// Creates a TaskCreated event
    pub fn task_created(title: &str, base_branch: &str, follow_up: Option<&str>) -> Self {
        Event::new(EventKind::TaskCreated {
            title: title.to_string(),
            base_branch: base_branch.to_string(),
            follow_up: follow_up.map(String::from),
        })
    }

    /// Creates a MessageSent event
    pub fn message_sent(prompt: &str, session_id: Option<&str>) -> Self {
        Event::new(EventKind::MessageSent {
            prompt: prompt.to_string(),
            session_id: session_id.map(String::from),
        })
    }

    /// Creates a WorkerStarted event
    pub fn worker_started(session_id: &str) -> Self {
        Event::new(EventKind::WorkerStarted {
            session_id: session_id.to_string(),
        })
    }

    /// Creates a WorkerReplied event
    pub fn worker_replied(reply: &str, tool_calls: Option<u32>, duration_secs: Option<f64>) -> Self {
        Event::new(EventKind::WorkerReplied {
            reply: reply.to_string(),
            tool_calls,
            duration_secs,
        })
    }

    /// Creates a WorkerError event
    pub fn worker_error(error: &str) -> Self {
        Event::new(EventKind::WorkerError {
            error: error.to_string(),
        })
    }

    /// Creates a WorkspaceAssigned event
    pub fn workspace_assigned(path: &str) -> Self {
        Event::new(EventKind::WorkspaceAssigned {
            path: path.to_string(),
        })
    }

    /// Creates a TaskMerged event
    pub fn task_merged(commit: &str, message: &str) -> Self {
        Event::new(EventKind::TaskMerged {
            commit: commit.to_string(),
            message: message.to_string(),
        })
    }

    /// Creates a TaskClosed event
    pub fn task_closed(abandoned: bool) -> Self {
        Event::new(EventKind::TaskClosed { abandoned })
    }

    /// Creates a StageChanged event
    pub fn stage_changed(from: WorkflowStage, to: WorkflowStage) -> Self {
        Event::new(EventKind::StageChanged { from, to })
    }
}

/// History manager for a task
pub struct History {
    task_name: String,
    path: PathBuf,
}

impl History {
    /// Creates a new history manager for a task
    pub fn new(task_name: &str) -> Result<Self> {
        validate_task_name(task_name)?;
        let path = task_dir(task_name)?.join("history.jsonl");
        Ok(History {
            task_name: task_name.to_string(),
            path,
        })
    }

    /// Loads an existing history
    pub fn load(task_name: &str) -> Result<Self> {
        let history = Self::new(task_name)?;
        if !history.path.exists() {
            return Err(SubtaskError::TaskNotFound(task_name.to_string()));
        }
        Ok(history)
    }

    /// Appends an event to the history
    pub fn append(&mut self, event: Event) -> Result<()> {
        // Ensure directory exists
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Append to file
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let line = serde_json::to_string(&event)?;
        writeln!(file, "{}", line)?;

        Ok(())
    }

    /// Reads all events from history
    pub fn read_all(&self) -> Result<Vec<Event>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for (line_num, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str(&line) {
                Ok(event) => events.push(event),
                Err(e) => {
                    return Err(SubtaskError::HistoryCorrupted {
                        path: self.path.clone(),
                        reason: format!("line {}: {}", line_num + 1, e),
                    });
                }
            }
        }

        Ok(events)
    }

    /// Returns the last N events
    pub fn tail(&self, n: usize) -> Result<Vec<Event>> {
        let events = self.read_all()?;
        let start = events.len().saturating_sub(n);
        Ok(events[start..].to_vec())
    }

    /// Reconstructs the task state from history
    pub fn reconstruct_task(&self) -> Result<Task> {
        let events = self.read_all()?;

        if events.is_empty() {
            return Err(SubtaskError::TaskNotFound(self.task_name.clone()));
        }

        // Find the TaskCreated event
        let created = events.iter().find_map(|e| match &e.kind {
            EventKind::TaskCreated {
                title,
                base_branch,
                follow_up,
            } => Some((title.clone(), base_branch.clone(), follow_up.clone(), e.timestamp)),
            _ => None,
        });

        let (title, base_branch, follow_up, created_at) =
            created.ok_or_else(|| SubtaskError::HistoryCorrupted {
                path: self.path.clone(),
                reason: "no TaskCreated event found".to_string(),
            })?;

        let mut task = Task {
            name: self.task_name.clone(),
            title,
            description: String::new(),
            base_branch,
            status: TaskStatus::Open,
            worker_status: WorkerStatus::Idle,
            stage: WorkflowStage::Doing,
            follow_up,
            workspace_path: None,
            session_id: None,
            created_at,
            updated_at: created_at,
        };

        // Replay events to reconstruct state
        for event in events {
            task.updated_at = event.timestamp;

            match event.kind {
                EventKind::DescriptionUpdated { description } => {
                    task.description = description;
                }
                EventKind::WorkspaceAssigned { path } => {
                    task.workspace_path = Some(PathBuf::from(path));
                }
                EventKind::WorkspaceReleased => {
                    task.workspace_path = None;
                }
                EventKind::WorkerStarted { session_id } => {
                    task.worker_status = WorkerStatus::Running;
                    task.session_id = Some(session_id);
                }
                EventKind::WorkerReplied { .. } => {
                    task.worker_status = WorkerStatus::Replied;
                }
                EventKind::WorkerError { .. } => {
                    task.worker_status = WorkerStatus::Error;
                }
                EventKind::WorkerInterrupted => {
                    task.worker_status = WorkerStatus::Replied;
                }
                EventKind::StageChanged { to, .. } => {
                    task.stage = to;
                }
                EventKind::TaskMerged { .. } => {
                    task.status = TaskStatus::Merged;
                    task.worker_status = WorkerStatus::Idle;
                    task.workspace_path = None;
                }
                EventKind::TaskClosed { .. } => {
                    task.status = TaskStatus::Closed;
                    task.worker_status = WorkerStatus::Idle;
                    task.workspace_path = None;
                }
                EventKind::TaskReopened => {
                    task.status = TaskStatus::Open;
                    task.worker_status = WorkerStatus::Idle;
                }
                _ => {}
            }
        }

        Ok(task)
    }

    /// Gets conversation messages (prompts and replies)
    pub fn get_conversation(&self) -> Result<Vec<ConversationMessage>> {
        let events = self.read_all()?;
        let mut messages = Vec::new();

        for event in events {
            match event.kind {
                EventKind::MessageSent { prompt, .. } => {
                    messages.push(ConversationMessage {
                        timestamp: event.timestamp,
                        role: MessageRole::User,
                        content: prompt,
                    });
                }
                EventKind::WorkerReplied { reply, .. } => {
                    messages.push(ConversationMessage {
                        timestamp: event.timestamp,
                        role: MessageRole::Assistant,
                        content: reply,
                    });
                }
                _ => {}
            }
        }

        Ok(messages)
    }
}

/// A message in the conversation
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub timestamp: DateTime<Utc>,
    pub role: MessageRole,
    pub content: String,
}

/// Role in a conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Note: These tests would need a proper test setup with project root mocking
    // For now, they serve as documentation of expected behavior

    #[test]
    fn test_event_serialization() {
        let event = Event::task_created("Fix auth bug", "main", None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("task_created"));
        assert!(json.contains("Fix auth bug"));
    }

    #[test]
    fn test_event_roundtrip() {
        let event = Event::worker_replied("Done!", Some(5), Some(10.5));
        let json = serde_json::to_string(&event).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();

        match parsed.kind {
            EventKind::WorkerReplied {
                reply,
                tool_calls,
                duration_secs,
            } => {
                assert_eq!(reply, "Done!");
                assert_eq!(tool_calls, Some(5));
                assert_eq!(duration_secs, Some(10.5));
            }
            _ => panic!("Wrong event kind"),
        }
    }
}
