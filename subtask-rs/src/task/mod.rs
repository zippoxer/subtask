//! Task management module
//!
//! Tasks are the core unit of work in Subtask. Each task has:
//! - A name (e.g., "fix/auth-bug")
//! - A folder at `.subtask/tasks/<escaped-name>/`
//! - Status tracking via history.jsonl
//! - Progress tracking via PROGRESS.json

pub mod history;
pub mod progress;
pub mod status;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::find_project_root;
use crate::error::{escape_task_name, validate_task_name, Result, SubtaskError};

pub use history::{Event, EventKind, History};
pub use progress::Progress;
pub use status::{TaskStatus, WorkerStatus, WorkflowStage};

/// Represents a task in Subtask
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Task name (e.g., "fix/auth-bug")
    pub name: String,

    /// Human-readable title
    pub title: String,

    /// Detailed description (from TASK.md frontmatter or body)
    #[serde(default)]
    pub description: String,

    /// Base branch this task branches from
    pub base_branch: String,

    /// Current task status
    #[serde(default)]
    pub status: TaskStatus,

    /// Current worker status
    #[serde(default)]
    pub worker_status: WorkerStatus,

    /// Current workflow stage
    #[serde(default)]
    pub stage: WorkflowStage,

    /// Task this follows up on (for context preservation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_up: Option<String>,

    /// Path to the assigned workspace (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<PathBuf>,

    /// Current session ID with the harness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// When the task was created
    pub created_at: DateTime<Utc>,

    /// When the task was last updated
    pub updated_at: DateTime<Utc>,
}

impl Task {
    /// Creates a new task with the given parameters
    ///
    /// # Security
    /// Validates task name to prevent path traversal and option injection
    pub fn new(name: &str, title: &str, base_branch: &str) -> Result<Self> {
        // Security: Validate task name
        validate_task_name(name)?;

        let now = Utc::now();
        Ok(Task {
            name: name.to_string(),
            title: title.to_string(),
            description: String::new(),
            base_branch: base_branch.to_string(),
            status: TaskStatus::Open,
            worker_status: WorkerStatus::Idle,
            stage: WorkflowStage::Doing,
            follow_up: None,
            workspace_path: None,
            session_id: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Returns the escaped name for use in file paths
    pub fn escaped_name(&self) -> String {
        escape_task_name(&self.name)
    }

    /// Returns the task directory path
    pub fn dir(&self) -> Result<PathBuf> {
        task_dir(&self.name)
    }

    /// Returns the path to TASK.md
    pub fn task_md_path(&self) -> Result<PathBuf> {
        Ok(self.dir()?.join("TASK.md"))
    }

    /// Returns the path to PLAN.md
    pub fn plan_md_path(&self) -> Result<PathBuf> {
        Ok(self.dir()?.join("PLAN.md"))
    }

    /// Returns the path to PROGRESS.json
    pub fn progress_path(&self) -> Result<PathBuf> {
        Ok(self.dir()?.join("PROGRESS.json"))
    }

    /// Returns the path to history.jsonl
    pub fn history_path(&self) -> Result<PathBuf> {
        Ok(self.dir()?.join("history.jsonl"))
    }

    /// Checks if the task is open
    pub fn is_open(&self) -> bool {
        self.status == TaskStatus::Open
    }

    /// Checks if a worker is currently running
    pub fn is_worker_running(&self) -> bool {
        self.worker_status == WorkerStatus::Running
    }

    /// Loads a task by name
    pub fn load(name: &str) -> Result<Self> {
        // Security: Validate task name before loading
        validate_task_name(name)?;

        let history = History::load(name)?;
        history.reconstruct_task()
    }

    /// Checks if a task exists
    pub fn exists(name: &str) -> Result<bool> {
        // Security: Validate task name
        validate_task_name(name)?;

        let dir = task_dir(name)?;
        Ok(dir.exists() && dir.join("TASK.md").exists())
    }

    /// Creates the task on disk
    pub fn create(&self) -> Result<()> {
        let dir = self.dir()?;
        std::fs::create_dir_all(&dir)?;

        // Write TASK.md
        self.write_task_md()?;

        // Initialize history with Created event
        let mut history = History::new(&self.name)?;
        history.append(Event::task_created(
            &self.title,
            &self.base_branch,
            self.follow_up.as_deref(),
        ))?;

        Ok(())
    }

    /// Writes the TASK.md file
    fn write_task_md(&self) -> Result<()> {
        let content = format!(
            "---\nversion: 1\n---\n\n# {}\n\n{}\n",
            self.title,
            self.description
        );
        std::fs::write(self.task_md_path()?, content)?;
        Ok(())
    }
}

/// Returns the tasks directory for the current project
pub fn tasks_dir() -> Result<PathBuf> {
    let project = find_project_root()?;
    Ok(project.join(".subtask").join("tasks"))
}

/// Returns the directory for a specific task
pub fn task_dir(name: &str) -> Result<PathBuf> {
    // Security: Validate task name
    validate_task_name(name)?;

    let escaped = escape_task_name(name);
    Ok(tasks_dir()?.join(escaped))
}

/// Returns the internal directory for runtime state
pub fn internal_dir(name: &str) -> Result<PathBuf> {
    // Security: Validate task name
    validate_task_name(name)?;

    let project = find_project_root()?;
    let escaped = escape_task_name(name);
    Ok(project.join(".subtask").join("internal").join(escaped))
}

/// Lists all tasks in the current project
pub fn list_tasks() -> Result<Vec<String>> {
    let dir = tasks_dir()?;

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut tasks = Vec::new();

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Check for TASK.md to confirm it's a valid task
            if path.join("TASK.md").exists() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    // Unescape the name
                    let task_name = crate::error::unescape_task_name(name);
                    tasks.push(task_name);
                }
            }
        }
    }

    // Sort by name for consistent output
    tasks.sort();

    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation_validates_name() {
        // Valid names
        assert!(Task::new("fix/auth-bug", "Fix auth bug", "main").is_ok());
        assert!(Task::new("simple-task", "Simple task", "main").is_ok());

        // Invalid names (path traversal)
        assert!(Task::new("../../../etc/passwd", "Evil", "main").is_err());

        // Invalid names (option injection)
        assert!(Task::new("-f", "Evil", "main").is_err());
        assert!(Task::new("--force", "Evil", "main").is_err());
    }

    #[test]
    fn test_escaped_name() {
        let task = Task::new("fix/auth-bug", "Fix auth bug", "main").unwrap();
        assert_eq!(task.escaped_name(), "fix--auth-bug");
    }
}
