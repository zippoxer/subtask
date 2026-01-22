//! Send command - Send a message to a task

use clap::Parser;

use crate::config::{find_project_root, Config};
use crate::error::validate_task_name;
use crate::harness::{self, build_prompt, Callbacks};
use crate::task::{history::Event, History, Task};
use crate::workspace::{ensure_task_symlink, Pool};

/// Send a message to a task (starts or resumes)
#[derive(Parser)]
pub struct SendCmd {
    /// Task name
    pub task: String,

    /// Message to send
    pub message: String,

    /// Don't wait for completion (run in background)
    #[arg(long)]
    pub background: bool,
}

impl SendCmd {
    pub fn run(self) -> anyhow::Result<()> {
        // Security: Validate task name
        validate_task_name(&self.task)?;

        let project_root = find_project_root()?;
        let config = Config::load(&project_root)?;

        // Load or create task
        let mut task = if Task::exists(&self.task)? {
            Task::load(&self.task)?
        } else {
            anyhow::bail!(
                "task not found: {}\n\nCreate it first with:\n  subtask draft {} --base-branch main --title \"...\"",
                self.task,
                self.task
            );
        };

        // Check if worker is already running
        if task.is_worker_running() {
            anyhow::bail!(
                "worker already running for task {}\n\nTo interrupt:\n  subtask interrupt {}",
                self.task,
                self.task
            );
        }

        // Acquire workspace if needed
        let workspace_path = if let Some(ref path) = task.workspace_path {
            path.clone()
        } else {
            let pool = Pool::new(&project_root, &config)?;
            let ws = pool.acquire(&self.task)?;

            // Record workspace assignment
            let mut history = History::load(&self.task)?;
            history.append(Event::workspace_assigned(&ws.path.to_string_lossy()))?;

            // Setup branch in workspace
            let repo = crate::git::Repository::open(&ws.path)?;
            if repo.branch_exists(&self.task)? {
                repo.checkout(&self.task)?;
            } else {
                repo.switch_create(&self.task, &task.base_branch)?;
            }

            // Create task symlink
            ensure_task_symlink(&ws.path, &project_root, &self.task)?;

            ws.path
        };

        // Build the prompt
        let prompt = build_prompt(&task, &workspace_path, true, &self.message, None);

        // Record message sent
        let mut history = History::load(&self.task)?;
        history.append(Event::message_sent(&self.message, task.session_id.as_deref()))?;

        // Create harness
        let harness = harness::create(&config.harness, &config.options)?;

        println!("Sending to task: {}", self.task);
        println!("Workspace: {}", workspace_path.display());
        println!("Harness: {}", harness.name());

        if self.background {
            println!("\nRunning in background...");
            println!("Check status with: subtask show {}", self.task);
            // Note: Full implementation would spawn a background process
            return Ok(());
        }

        // Run the harness
        println!("\nStarting worker...\n");

        let callbacks = Callbacks {
            on_session_start: Some(Box::new(|session_id| {
                println!("Session: {}", session_id);
            })),
            on_tool_call: Some(Box::new(|_| {
                print!(".");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            })),
            on_text: None,
        };

        // Record worker started
        let session_id = task.session_id.clone().unwrap_or_else(|| "pending".to_string());
        history.append(Event::worker_started(&session_id))?;

        // Run synchronously for now (full impl would use tokio runtime)
        let rt = tokio::runtime::Runtime::new()?;
        let result = rt.block_on(async {
            harness
                .run(
                    &workspace_path,
                    &prompt,
                    task.session_id.as_deref(),
                    callbacks,
                )
                .await
        });

        match result {
            Ok(run_result) => {
                // Record success
                history.append(Event::worker_replied(
                    &run_result.reply,
                    Some(run_result.tool_calls),
                    Some(run_result.duration_secs),
                ))?;

                println!("\n\nWorker completed.");
                println!("  Tool calls: {}", run_result.tool_calls);
                println!("  Duration: {:.1}s", run_result.duration_secs);

                if !run_result.reply.is_empty() {
                    println!("\nReply:\n{}", run_result.reply);
                }
            }
            Err(e) => {
                // Record error
                history.append(Event::worker_error(&e.to_string()))?;

                println!("\n\nWorker failed: {}", e);
                return Err(e.into());
            }
        }

        Ok(())
    }
}
