//! Init command - Initialize subtask in a project

use clap::Parser;
use std::io::{self, Write};

use crate::config::Config;

/// Initialize subtask in the current project
#[derive(Parser)]
pub struct InitCmd {
    /// Harness to use (claude, codex, opencode)
    #[arg(long)]
    harness: Option<String>,

    /// Maximum concurrent workspaces
    #[arg(long, default_value = "4")]
    max_workspaces: usize,

    /// Skip interactive prompts
    #[arg(long)]
    yes: bool,
}

impl InitCmd {
    pub fn run(self) -> anyhow::Result<()> {
        let project_root = std::env::current_dir()?;

        // Check if already initialized
        let subtask_dir = project_root.join(".subtask");
        if subtask_dir.exists() {
            println!("Subtask is already initialized in this project.");
            println!("Config: {}", subtask_dir.join("config.json").display());
            return Ok(());
        }

        // Determine harness
        let harness = match self.harness {
            Some(h) => h,
            None if self.yes => "claude".to_string(),
            None => Self::select_harness()?,
        };

        // Create config
        let config = Config {
            harness: harness.clone(),
            max_workspaces: self.max_workspaces,
            options: Default::default(),
        };

        // Save config
        config.save(&project_root)?;

        // Create directories
        std::fs::create_dir_all(subtask_dir.join("tasks"))?;
        std::fs::create_dir_all(subtask_dir.join("internal"))?;

        // Add to .gitignore if it exists
        Self::update_gitignore(&project_root)?;

        println!("✓ Initialized subtask with {} harness", harness);
        println!("  Config: .subtask/config.json");
        println!("  Max workspaces: {}", self.max_workspaces);
        println!("\nNext steps:");
        println!("  subtask draft <task-name> --base-branch main --title \"...\"");
        println!("  subtask send <task-name> \"...\"");

        Ok(())
    }

    fn select_harness() -> anyhow::Result<String> {
        println!("Select a harness (AI coding assistant):\n");
        println!("  1. claude  - Claude Code CLI (Anthropic)");
        println!("  2. codex   - Codex CLI (OpenAI)");
        println!("  3. opencode - OpenCode CLI (Open source)");
        println!();

        loop {
            print!("Enter choice [1-3]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            match input.trim() {
                "1" | "claude" => return Ok("claude".to_string()),
                "2" | "codex" => return Ok("codex".to_string()),
                "3" | "opencode" => return Ok("opencode".to_string()),
                "" => return Ok("claude".to_string()), // Default
                _ => println!("Invalid choice. Please enter 1, 2, or 3."),
            }
        }
    }

    fn update_gitignore(project_root: &std::path::Path) -> anyhow::Result<()> {
        let gitignore_path = project_root.join(".gitignore");

        if !gitignore_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&gitignore_path)?;

        // Check if .subtask is already ignored
        if content.lines().any(|line| line.trim() == ".subtask" || line.trim() == ".subtask/") {
            return Ok(());
        }

        // Append .subtask to .gitignore
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&gitignore_path)?;

        writeln!(file, "\n# Subtask")?;
        writeln!(file, ".subtask/")?;

        println!("✓ Added .subtask/ to .gitignore");

        Ok(())
    }
}
