//! Draft command - Create a new task without starting it

use clap::Parser;

use crate::error::validate_task_name;
use crate::task::Task;

/// Create a new task without starting it
#[derive(Parser)]
pub struct DraftCmd {
    /// Task name (e.g., "fix/auth-bug")
    pub task: String,

    /// Base branch to branch from
    #[arg(long)]
    pub base_branch: String,

    /// Task title
    #[arg(long)]
    pub title: String,

    /// Task description
    #[arg(long)]
    pub description: Option<String>,

    /// Follow up from another task (preserves context)
    #[arg(long)]
    pub follow_up: Option<String>,

    /// Workflow type (default, you-plan, they-plan)
    #[arg(long, default_value = "default")]
    pub workflow: String,
}

impl DraftCmd {
    pub fn run(self) -> anyhow::Result<()> {
        // Security: Validate task name
        validate_task_name(&self.task)?;

        // Check if task already exists
        if Task::exists(&self.task)? {
            anyhow::bail!("task already exists: {}", self.task);
        }

        // Validate follow-up task exists if specified
        if let Some(ref follow_up) = self.follow_up {
            validate_task_name(follow_up)?;
            if !Task::exists(follow_up)? {
                anyhow::bail!("follow-up task not found: {}", follow_up);
            }
        }

        // Create the task
        let mut task = Task::new(&self.task, &self.title, &self.base_branch)?;

        if let Some(ref desc) = self.description {
            task.description = desc.clone();
        }

        task.follow_up = self.follow_up.clone();

        // Create on disk
        task.create()?;

        // Print result
        println!("Created task: {}", self.task);
        println!("  Title: {}", self.title);
        println!("  Base branch: {}", self.base_branch);
        if let Some(ref follow_up) = self.follow_up {
            println!("  Follow-up from: {}", follow_up);
        }
        println!("\nNext: subtask send {} \"...\"", self.task);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_draft_validates_task_name() {
        // This would need a proper test setup with project root
        // For now, we just test the validation function directly
        assert!(validate_task_name("fix/auth-bug").is_ok());
        assert!(validate_task_name("../../../etc/passwd").is_err());
        assert!(validate_task_name("-f").is_err());
    }
}
