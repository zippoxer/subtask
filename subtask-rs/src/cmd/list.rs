//! List command - List all tasks

use clap::Parser;

use crate::task::{list_tasks, Task, TaskStatus, WorkerStatus};

/// List all tasks
#[derive(Parser)]
pub struct ListCmd {
    /// Show only open tasks
    #[arg(long)]
    open: bool,

    /// Show only tasks with running workers
    #[arg(long)]
    running: bool,

    /// Output format (table, json)
    #[arg(long, default_value = "table")]
    format: String,
}

impl ListCmd {
    pub fn run(self) -> anyhow::Result<()> {
        let task_names = list_tasks()?;

        if task_names.is_empty() {
            println!("No tasks found.");
            println!("\nCreate one with:");
            println!("  subtask draft <name> --base-branch main --title \"...\"");
            return Ok(());
        }

        // Load all tasks
        let mut tasks: Vec<Task> = Vec::new();
        for name in &task_names {
            match Task::load(name) {
                Ok(task) => tasks.push(task),
                Err(e) => {
                    eprintln!("Warning: could not load task {}: {}", name, e);
                }
            }
        }

        // Apply filters
        if self.open {
            tasks.retain(|t| t.status == TaskStatus::Open);
        }
        if self.running {
            tasks.retain(|t| t.worker_status == WorkerStatus::Running);
        }

        if tasks.is_empty() {
            println!("No tasks match the filter criteria.");
            return Ok(());
        }

        // Output
        match self.format.as_str() {
            "json" => {
                let json = serde_json::to_string_pretty(&tasks)?;
                println!("{}", json);
            }
            _ => {
                self.print_table(&tasks);
            }
        }

        Ok(())
    }

    fn print_table(&self, tasks: &[Task]) {
        // Header
        println!(
            "{:<30} {:<10} {:<10} {:<10} {}",
            "TASK", "STATUS", "WORKER", "STAGE", "TITLE"
        );
        println!("{}", "-".repeat(80));

        // Rows
        for task in tasks {
            let status_icon = match task.status {
                TaskStatus::Open => "●",
                TaskStatus::Merged => "✓",
                TaskStatus::Closed => "○",
            };

            let worker_icon = match task.worker_status {
                WorkerStatus::Idle => "○",
                WorkerStatus::Running => "▶",
                WorkerStatus::Replied => "●",
                WorkerStatus::Error => "✗",
            };

            // Truncate title if too long
            let title = if task.title.len() > 30 {
                format!("{}...", &task.title[..27])
            } else {
                task.title.clone()
            };

            println!(
                "{:<30} {} {:<8} {} {:<8} {:<10} {}",
                task.name,
                status_icon,
                task.status,
                worker_icon,
                task.worker_status,
                task.stage,
                title
            );
        }

        println!();
        println!(
            "{} task(s) total",
            tasks.len()
        );
    }
}
