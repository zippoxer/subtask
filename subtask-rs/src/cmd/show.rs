//! Show command - Show task details

use clap::Parser;

use crate::error::validate_task_name;
use crate::git::Repository;
use crate::task::{Progress, Task};

/// Show task details
#[derive(Parser)]
pub struct ShowCmd {
    /// Task name
    pub task: String,

    /// Show full description
    #[arg(long)]
    pub full: bool,

    /// Output format (text, json)
    #[arg(long, default_value = "text")]
    pub format: String,
}

impl ShowCmd {
    pub fn run(self) -> anyhow::Result<()> {
        // Security: Validate task name
        validate_task_name(&self.task)?;

        let task = Task::load(&self.task)?;

        match self.format.as_str() {
            "json" => {
                let json = serde_json::to_string_pretty(&task)?;
                println!("{}", json);
            }
            _ => {
                self.print_text(&task)?;
            }
        }

        Ok(())
    }

    fn print_text(&self, task: &Task) -> anyhow::Result<()> {
        println!("Task: {}", task.name);
        println!("Title: {}", task.title);
        println!();

        // Status section
        println!("Status");
        println!("  Task: {}", task.status);
        println!("  Worker: {}", task.worker_status);
        println!("  Stage: {}", task.stage);
        println!();

        // Git section
        println!("Git");
        println!("  Branch: {}", task.name);
        println!("  Base: {}", task.base_branch);

        if let Some(ref ws_path) = task.workspace_path {
            println!("  Workspace: {}", ws_path.display());

            // Try to get diff stats
            if let Ok(repo) = Repository::open(ws_path) {
                if let Ok(stats) = repo.diff_stats(&task.base_branch, "HEAD") {
                    println!(
                        "  Changes: {} files, +{} -{} ",
                        stats.files_changed, stats.insertions, stats.deletions
                    );
                }
                if let Ok(count) = repo.commit_count(&task.base_branch, "HEAD") {
                    println!("  Commits: {}", count);
                }
            }
        } else {
            println!("  Workspace: (none assigned)");
        }
        println!();

        // Session section
        if let Some(ref session_id) = task.session_id {
            println!("Session");
            println!("  ID: {}", session_id);
            println!();
        }

        // Progress section
        if let Ok(progress_path) = task.progress_path() {
            if let Ok(progress) = Progress::load(&progress_path) {
                if progress.status.is_some()
                    || progress.percent.is_some()
                    || !progress.completed.is_empty()
                {
                    println!("Progress");
                    println!("  {}", progress.summary());

                    if !progress.completed.is_empty() {
                        println!("  Completed:");
                        for item in &progress.completed {
                            println!("    ✓ {}", item);
                        }
                    }

                    if !progress.pending.is_empty() {
                        println!("  Pending:");
                        for item in &progress.pending {
                            println!("    ○ {}", item);
                        }
                    }

                    if !progress.blocked.is_empty() {
                        println!("  Blocked:");
                        for item in &progress.blocked {
                            println!("    ✗ {}", item);
                        }
                    }
                    println!();
                }
            }
        }

        // Description
        if !task.description.is_empty() {
            println!("Description");
            if self.full || task.description.len() < 200 {
                println!("{}", task.description);
            } else {
                println!("{}...", &task.description[..200]);
                println!("\n(use --full to see complete description)");
            }
            println!();
        }

        // Follow-up
        if let Some(ref follow_up) = task.follow_up {
            println!("Follow-up from: {}", follow_up);
            println!();
        }

        // Timestamps
        println!("Created: {}", task.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
        println!("Updated: {}", task.updated_at.format("%Y-%m-%d %H:%M:%S UTC"));

        Ok(())
    }
}
