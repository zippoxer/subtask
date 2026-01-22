//! Harness module - Worker backend implementations
//!
//! A harness is an abstraction over different AI coding assistants
//! (Claude Code, Codex, OpenCode) that execute prompts and return results.

mod claude;
mod codex;
mod opencode;
mod prompt;

pub use claude::ClaudeHarness;
pub use codex::CodexHarness;
pub use opencode::OpenCodeHarness;
pub use prompt::{build_prompt, build_review_prompt, ReviewTarget};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::path::Path;

use crate::config::HarnessOptions;
use crate::error::{Result, SubtaskError};
use crate::task::Task;

/// Result of running a harness
#[derive(Debug, Clone, Default)]
pub struct RunResult {
    /// Session ID for continuing the conversation
    pub session_id: Option<String>,

    /// Whether the prompt was successfully delivered
    pub prompt_delivered: bool,

    /// Whether the agent replied
    pub agent_replied: bool,

    /// The agent's reply text
    pub reply: String,

    /// Number of tool calls made
    pub tool_calls: u32,

    /// Duration of the run in seconds
    pub duration_secs: f64,

    /// Error message if the run failed
    pub error: Option<String>,
}

/// Callbacks for harness events
#[derive(Default)]
pub struct Callbacks {
    /// Called when the session starts
    pub on_session_start: Option<Box<dyn Fn(&str) + Send + Sync>>,

    /// Called when a tool is invoked
    pub on_tool_call: Option<Box<dyn Fn(DateTime<Utc>) + Send + Sync>>,

    /// Called when text is streamed
    pub on_text: Option<Box<dyn Fn(&str) + Send + Sync>>,
}

/// The harness trait - implemented by each AI backend
#[async_trait]
pub trait Harness: Send + Sync {
    /// Runs a prompt and returns the result
    async fn run(
        &self,
        cwd: &Path,
        prompt: &str,
        continue_from: Option<&str>,
        callbacks: Callbacks,
    ) -> Result<RunResult>;

    /// Runs a code review
    async fn review(
        &self,
        cwd: &Path,
        target: ReviewTarget,
        instructions: &str,
    ) -> Result<String>;

    /// Migrates a session to a new working directory
    fn migrate_session(
        &self,
        session_id: &str,
        old_cwd: &Path,
        new_cwd: &Path,
    ) -> Result<()>;

    /// Duplicates a session for a new working directory
    fn duplicate_session(
        &self,
        session_id: &str,
        old_cwd: &Path,
        new_cwd: &Path,
    ) -> Result<Option<String>>;

    /// Returns the harness name
    fn name(&self) -> &str;
}

/// Creates a harness based on the configuration
pub fn create(harness_name: &str, options: &HarnessOptions) -> Result<Box<dyn Harness>> {
    match harness_name {
        "claude" => Ok(Box::new(ClaudeHarness::new(options))),
        "codex" => Ok(Box::new(CodexHarness::new(options))),
        "opencode" => Ok(Box::new(OpenCodeHarness::new(options))),
        _ => Err(SubtaskError::HarnessNotFound(harness_name.to_string())),
    }
}

/// Checks if a CLI tool is available
pub fn cli_available(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Safe shell word validation
///
/// Returns true if the word is safe to use in shell commands without quoting
pub fn is_safe_shell_word(word: &str) -> bool {
    if word.is_empty() {
        return false;
    }

    // Only allow alphanumeric, dash, underscore, dot, plus
    word.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '+'
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_shell_word() {
        assert!(is_safe_shell_word("claude"));
        assert!(is_safe_shell_word("codex-cli"));
        assert!(is_safe_shell_word("tool_name"));
        assert!(is_safe_shell_word("v1.2.3"));

        assert!(!is_safe_shell_word(""));
        assert!(!is_safe_shell_word("$(whoami)"));
        assert!(!is_safe_shell_word("`rm -rf`"));
        assert!(!is_safe_shell_word("a b"));
        assert!(!is_safe_shell_word("a;b"));
    }
}
