//! Merge command - Merge a task into the base branch

use clap::Parser;

use crate::config::find_project_root;
use crate::error::validate_task_name;
use crate::git::Repository;
use crate::task::{history::Event, History, Task, TaskStatus};
use crate::workspace::Pool;

/// Merge a task into the base branch
#[derive(Parser)]
pub struct MergeCmd {
    /// Task name
    pub task: String,

    /// Commit message
    #[arg(short, long)]
    pub message: String,

    /// Delete branch after merge
    #[arg(long, default_value = "true")]
    pub delete_branch: bool,
}

impl MergeCmd {
    pub fn run(self) -> anyhow::Result<()> {
        // Security: Validate task name
        validate_task_name(&self.task)?;

        let task = Task::load(&self.task)?;

        // Check task status
        if task.status != TaskStatus::Open {
            anyhow::bail!(
                "cannot merge task {} (status: {})\n\nOnly open tasks can be merged.",
                self.task,
                task.status
            );
        }

        // Check for running worker
        if task.is_worker_running() {
            anyhow::bail!(
                "cannot merge while worker is running\n\nWait for completion or interrupt:\n  subtask interrupt {}",
                self.task
            );
        }

        let project_root = find_project_root()?;

        // Open the main repository
        let repo = Repository::discover(&project_root)?;

        // Check that we're on the base branch
        let current_branch = repo.current_branch()?;
        if current_branch.as_deref() != Some(&task.base_branch) {
            println!("Switching to base branch: {}", task.base_branch);
            repo.checkout(&task.base_branch)?;
        }

        // Check that the task branch exists
        if !repo.branch_exists(&self.task)? {
            anyhow::bail!(
                "task branch not found: {}\n\nThe branch may have been deleted.",
                self.task
            );
        }

        // Perform squash merge
        println!("Merging task: {}", self.task);
        println!("  Into: {}", task.base_branch);
        println!("  Message: {}", self.message);
        println!();

        let commit = repo.merge_squash(&self.task, &self.message)?;

        println!("✓ Merged as commit: {}", &commit[..8]);

        // Delete the branch if requested
        if self.delete_branch {
            if let Err(e) = repo.delete_branch(&self.task, true) {
                eprintln!("Warning: could not delete branch: {}", e);
            } else {
                println!("✓ Deleted branch: {}", self.task);
            }
        }

        // Release workspace if assigned
        if let Some(ref ws_path) = task.workspace_path {
            let config = crate::config::Config::load(&project_root)?;
            let pool = Pool::new(&project_root, &config)?;
            if let Err(e) = pool.release(ws_path) {
                eprintln!("Warning: could not release workspace: {}", e);
            }
        }

        // Record the merge event
        let mut history = History::load(&self.task)?;
        history.append(Event::task_merged(&commit, &self.message))?;

        println!("\nTask {} merged successfully.", self.task);

        Ok(())
    }
}
