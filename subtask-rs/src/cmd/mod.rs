//! CLI commands module
//!
//! Implements all subtask CLI commands using clap for argument parsing.

mod draft;
mod init;
mod list;
mod merge;
mod send;
mod show;

use clap::{Parser, Subcommand};

pub use draft::DraftCmd;
pub use init::InitCmd;
pub use list::ListCmd;
pub use merge::MergeCmd;
pub use send::SendCmd;
pub use show::ShowCmd;

/// Subtask - Parallel task management for AI coding agents
#[derive(Parser)]
#[command(name = "subtask")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize subtask in the current project
    Init(InitCmd),

    /// Create a new task without starting it
    Draft(DraftCmd),

    /// Send a message to a task (starts or resumes)
    Send(SendCmd),

    /// List all tasks
    List(ListCmd),

    /// Show task details
    Show(ShowCmd),

    /// Merge a task into the base branch
    Merge(MergeCmd),

    /// Close a task without merging
    Close(CloseCmd),

    /// Print the workspace path for a task
    Workspace(WorkspaceCmd),

    /// Ask a quick question (no task)
    Ask(AskCmd),

    /// Interrupt a running worker
    Interrupt(InterruptCmd),

    /// Show task conversation log
    Log(LogCmd),

    /// Advance task workflow stage
    Stage(StageCmd),

    /// Install the subtask skill for Claude Code
    Install(InstallCmd),
}

impl Cli {
    /// Runs the CLI
    pub fn run(self) -> anyhow::Result<()> {
        match self.command {
            Commands::Init(cmd) => cmd.run(),
            Commands::Draft(cmd) => cmd.run(),
            Commands::Send(cmd) => cmd.run(),
            Commands::List(cmd) => cmd.run(),
            Commands::Show(cmd) => cmd.run(),
            Commands::Merge(cmd) => cmd.run(),
            Commands::Close(cmd) => cmd.run(),
            Commands::Workspace(cmd) => cmd.run(),
            Commands::Ask(cmd) => cmd.run(),
            Commands::Interrupt(cmd) => cmd.run(),
            Commands::Log(cmd) => cmd.run(),
            Commands::Stage(cmd) => cmd.run(),
            Commands::Install(cmd) => cmd.run(),
        }
    }
}

// Placeholder commands that need full implementation

/// Close a task without merging
#[derive(Parser)]
pub struct CloseCmd {
    /// Task name
    task: String,

    /// Abandon changes (delete branch)
    #[arg(long)]
    abandon: bool,
}

impl CloseCmd {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::error::validate_task_name;
        use crate::task::{history::Event, Task};

        validate_task_name(&self.task)?;

        let task = Task::load(&self.task)?;
        if !task.is_open() {
            anyhow::bail!("task {} is not open (status: {})", self.task, task.status);
        }

        // Record the close event
        let mut history = crate::task::History::load(&self.task)?;
        history.append(Event::task_closed(self.abandon))?;

        println!("Closed task: {}", self.task);
        if self.abandon {
            println!("Branch abandoned (changes discarded)");
        }

        Ok(())
    }
}

/// Print workspace path
#[derive(Parser)]
pub struct WorkspaceCmd {
    /// Task name
    task: String,
}

impl WorkspaceCmd {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::error::validate_task_name;
        use crate::task::Task;

        validate_task_name(&self.task)?;

        let task = Task::load(&self.task)?;
        match task.workspace_path {
            Some(path) => println!("{}", path.display()),
            None => anyhow::bail!("task {} has no workspace assigned", self.task),
        }

        Ok(())
    }
}

/// Ask a quick question
#[derive(Parser)]
pub struct AskCmd {
    /// The question to ask
    question: String,
}

impl AskCmd {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::config::{find_project_root, Config};
        use crate::harness;

        let project_root = find_project_root()?;
        let config = Config::load(&project_root)?;
        let harness = harness::create(&config.harness, &config.options)?;

        println!("Asking: {}", self.question);
        println!("(Using {} harness)", harness.name());
        println!("\nNote: Full implementation would run the harness here.");

        Ok(())
    }
}

/// Interrupt a running worker
#[derive(Parser)]
pub struct InterruptCmd {
    /// Task name
    task: String,
}

impl InterruptCmd {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::error::validate_task_name;
        use crate::task::Task;

        validate_task_name(&self.task)?;

        let task = Task::load(&self.task)?;
        if !task.is_worker_running() {
            anyhow::bail!("no worker running for task {}", self.task);
        }

        println!("Interrupting worker for task: {}", self.task);
        println!("Note: Full implementation would send SIGINT to the worker process.");

        Ok(())
    }
}

/// Show task conversation log
#[derive(Parser)]
pub struct LogCmd {
    /// Task name
    task: String,

    /// Number of entries to show
    #[arg(short = 'n', default_value = "10")]
    count: usize,
}

impl LogCmd {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::error::validate_task_name;
        use crate::task::History;

        validate_task_name(&self.task)?;

        let history = History::load(&self.task)?;
        let messages = history.get_conversation()?;

        println!("Conversation log for: {}\n", self.task);

        let start = messages.len().saturating_sub(self.count);
        for msg in &messages[start..] {
            let role = match msg.role {
                crate::task::history::MessageRole::User => "You",
                crate::task::history::MessageRole::Assistant => "Agent",
            };
            println!("[{}] {}:", msg.timestamp.format("%Y-%m-%d %H:%M"), role);
            println!("{}\n", msg.content);
        }

        Ok(())
    }
}

/// Advance workflow stage
#[derive(Parser)]
pub struct StageCmd {
    /// Task name
    task: String,

    /// Target stage
    stage: String,
}

impl StageCmd {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::error::validate_task_name;
        use crate::task::{history::Event, History, Task, WorkflowStage};

        validate_task_name(&self.task)?;

        let task = Task::load(&self.task)?;

        let new_stage = match self.stage.as_str() {
            "plan" => WorkflowStage::Plan,
            "implement" => WorkflowStage::Implement,
            "doing" => WorkflowStage::Doing,
            "review" => WorkflowStage::Review,
            "ready" => WorkflowStage::Ready,
            _ => anyhow::bail!("unknown stage: {}", self.stage),
        };

        let mut history = History::load(&self.task)?;
        history.append(Event::stage_changed(task.stage, new_stage))?;

        println!(
            "Task {} stage: {} → {}",
            self.task, task.stage, new_stage
        );

        Ok(())
    }
}

/// Install subtask skill
#[derive(Parser)]
pub struct InstallCmd {
    /// Force reinstall
    #[arg(long)]
    force: bool,
}

impl InstallCmd {
    pub fn run(self) -> anyhow::Result<()> {
        println!("Installing subtask skill for Claude Code...");
        println!("\nNote: Full implementation would:");
        println!("  1. Locate Claude Code plugin directory");
        println!("  2. Copy SKILL.md to the appropriate location");
        println!("  3. Register the skill with Claude Code");

        if self.force {
            println!("\n(Force mode: would overwrite existing installation)");
        }

        println!("\nSkill installation placeholder complete.");
        Ok(())
    }
}
