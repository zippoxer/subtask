//! Subtask - Parallel task management and orchestration for AI coding agents
//!
//! A lead agent dispatches work to parallel workers in isolated git worktrees,
//! with context preservation and progress tracking.

mod cmd;
mod config;
mod error;
mod git;
mod harness;
mod task;
mod tui;
mod workspace;

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::cmd::Cli;

fn main() -> anyhow::Result<()> {
    // Initialize tracing/logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "subtask=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    // Parse CLI arguments and run
    let cli = Cli::parse();
    cli.run()
}
