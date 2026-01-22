//! Task and worker status types
//!
//! Task status is organizational and durable.
//! Worker status is ephemeral, within an open task.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Task status (organizational, durable)
///
/// Represents the lifecycle state of a task itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    /// Task is active and can be worked on
    #[default]
    Open,
    /// Work has been merged into base branch
    Merged,
    /// Task was closed without merging
    Closed,
}

impl TaskStatus {
    /// Returns true if the task can be sent work
    pub fn can_send(&self) -> bool {
        // Can send to open tasks, but also to merged/closed to reopen
        true
    }

    /// Returns true if the task can be merged
    pub fn can_merge(&self) -> bool {
        *self == TaskStatus::Open
    }

    /// Returns true if the task can be closed
    pub fn can_close(&self) -> bool {
        *self == TaskStatus::Open
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskStatus::Open => write!(f, "open"),
            TaskStatus::Merged => write!(f, "merged"),
            TaskStatus::Closed => write!(f, "closed"),
        }
    }
}

/// Worker status (ephemeral, operational)
///
/// Represents the current state of the worker within an open task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerStatus {
    /// No worker activity yet
    #[default]
    Idle,
    /// Worker is currently executing
    Running,
    /// Worker finished, awaiting follow-up
    Replied,
    /// Last run failed
    Error,
}

impl fmt::Display for WorkerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkerStatus::Idle => write!(f, "idle"),
            WorkerStatus::Running => write!(f, "running"),
            WorkerStatus::Replied => write!(f, "replied"),
            WorkerStatus::Error => write!(f, "error"),
        }
    }
}

/// Workflow stage for tasks with planning workflows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowStage {
    /// Planning phase (for planning workflows)
    Plan,
    /// Implementation phase (for planning workflows)
    Implement,
    /// Default stage: doing the work
    #[default]
    Doing,
    /// Review phase
    Review,
    /// Ready for merge
    Ready,
}

impl WorkflowStage {
    /// Returns the next stage in the default workflow
    pub fn next(&self) -> Option<WorkflowStage> {
        match self {
            WorkflowStage::Plan => Some(WorkflowStage::Implement),
            WorkflowStage::Implement => Some(WorkflowStage::Review),
            WorkflowStage::Doing => Some(WorkflowStage::Review),
            WorkflowStage::Review => Some(WorkflowStage::Ready),
            WorkflowStage::Ready => None,
        }
    }

    /// Returns all stages in the default workflow
    pub fn default_workflow() -> Vec<WorkflowStage> {
        vec![
            WorkflowStage::Doing,
            WorkflowStage::Review,
            WorkflowStage::Ready,
        ]
    }

    /// Returns all stages in the planning workflow
    pub fn planning_workflow() -> Vec<WorkflowStage> {
        vec![
            WorkflowStage::Plan,
            WorkflowStage::Implement,
            WorkflowStage::Review,
            WorkflowStage::Ready,
        ]
    }
}

impl fmt::Display for WorkflowStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkflowStage::Plan => write!(f, "plan"),
            WorkflowStage::Implement => write!(f, "implement"),
            WorkflowStage::Doing => write!(f, "doing"),
            WorkflowStage::Review => write!(f, "review"),
            WorkflowStage::Ready => write!(f, "ready"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_transitions() {
        assert!(TaskStatus::Open.can_merge());
        assert!(TaskStatus::Open.can_close());
        assert!(!TaskStatus::Merged.can_merge());
        assert!(!TaskStatus::Closed.can_close());
    }

    #[test]
    fn test_workflow_stage_progression() {
        assert_eq!(WorkflowStage::Doing.next(), Some(WorkflowStage::Review));
        assert_eq!(WorkflowStage::Review.next(), Some(WorkflowStage::Ready));
        assert_eq!(WorkflowStage::Ready.next(), None);
    }
}
