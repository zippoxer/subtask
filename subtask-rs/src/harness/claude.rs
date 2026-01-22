//! Claude Code harness implementation

use async_trait::async_trait;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use super::{build_review_prompt, Callbacks, Harness, ReviewTarget, RunResult};
use crate::config::HarnessOptions;
use crate::error::{Result, SubtaskError};

/// Claude Code CLI harness
pub struct ClaudeHarness {
    model: Option<String>,
    permission_mode: String,
    tools: Option<String>,
}

impl ClaudeHarness {
    /// Creates a new Claude harness from options
    pub fn new(options: &HarnessOptions) -> Self {
        ClaudeHarness {
            model: options.model.clone(),
            permission_mode: options
                .permission_mode
                .clone()
                .unwrap_or_else(|| "bypassPermissions".to_string()),
            tools: options.tools.clone(),
        }
    }

    fn build_args(&self, continue_from: Option<&str>) -> Vec<String> {
        let mut args = vec!["--output-format".to_string(), "stream-json".to_string()];

        // Permission mode
        args.push(format!("--{}", self.permission_mode));

        // Model
        if let Some(ref model) = self.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        // Tools
        if let Some(ref tools) = self.tools {
            args.push("--tools".to_string());
            args.push(tools.clone());
        }

        // Continue from session
        if let Some(session) = continue_from {
            args.push("--continue".to_string());
            args.push(session.to_string());
        }

        args
    }
}

#[async_trait]
impl Harness for ClaudeHarness {
    async fn run(
        &self,
        cwd: &Path,
        prompt: &str,
        continue_from: Option<&str>,
        callbacks: Callbacks,
    ) -> Result<RunResult> {
        let args = self.build_args(continue_from);

        let mut cmd = Command::new("claude");
        cmd.current_dir(cwd)
            .args(&args)
            .arg("--")
            .arg(prompt)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let start = Instant::now();

        let mut child = cmd.spawn().map_err(|e| SubtaskError::HarnessExecution {
            harness: "claude".to_string(),
            message: format!("failed to start claude: {}", e),
        })?;

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        let mut result = RunResult {
            session_id: continue_from.map(String::from),
            prompt_delivered: continue_from.is_some(),
            ..Default::default()
        };

        let mut reply = String::new();

        // Parse streaming JSON output
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            if line.trim().is_empty() {
                continue;
            }

            // Parse the JSON event
            if let Ok(event) = serde_json::from_str::<ClaudeEvent>(&line) {
                match event {
                    ClaudeEvent::SystemInit { session_id, .. } => {
                        result.session_id = Some(session_id.clone());
                        result.prompt_delivered = true;
                        if let Some(ref cb) = callbacks.on_session_start {
                            cb(&session_id);
                        }
                    }
                    ClaudeEvent::AssistantMessage { content, .. } => {
                        if let Some(text) = content.first().and_then(|c| c.text.as_ref()) {
                            reply.push_str(text);
                            result.reply = reply.clone();
                            result.agent_replied = true;
                            if let Some(ref cb) = callbacks.on_text {
                                cb(text);
                            }
                        }
                    }
                    ClaudeEvent::ToolUse { .. } => {
                        result.tool_calls += 1;
                        if let Some(ref cb) = callbacks.on_tool_call {
                            cb(chrono::Utc::now());
                        }
                    }
                    ClaudeEvent::ResultError { error, .. } => {
                        result.error = Some(error);
                    }
                    _ => {}
                }
            }
        }

        let status = child.wait().map_err(|e| SubtaskError::HarnessExecution {
            harness: "claude".to_string(),
            message: format!("failed to wait for claude: {}", e),
        })?;

        result.duration_secs = start.elapsed().as_secs_f64();

        if !status.success() && result.error.is_none() {
            result.error = Some(format!("claude exited with status: {}", status));
        }

        if result.error.is_some() {
            return Err(SubtaskError::HarnessExecution {
                harness: "claude".to_string(),
                message: result.error.clone().unwrap(),
            });
        }

        Ok(result)
    }

    async fn review(
        &self,
        cwd: &Path,
        target: ReviewTarget,
        instructions: &str,
    ) -> Result<String> {
        let prompt = build_review_prompt(cwd, &target, instructions);
        let result = self.run(cwd, &prompt, None, Callbacks::default()).await?;
        Ok(result.reply)
    }

    fn migrate_session(
        &self,
        _session_id: &str,
        _old_cwd: &Path,
        _new_cwd: &Path,
    ) -> Result<()> {
        // Claude sessions are workspace-bound; migration not supported
        Ok(())
    }

    fn duplicate_session(
        &self,
        _session_id: &str,
        _old_cwd: &Path,
        _new_cwd: &Path,
    ) -> Result<Option<String>> {
        // Claude doesn't support session duplication
        Ok(None)
    }

    fn name(&self) -> &str {
        "claude"
    }
}

/// Claude streaming event types
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClaudeEvent {
    SystemInit {
        session_id: String,
        #[serde(default)]
        tools: Vec<String>,
    },
    AssistantMessage {
        #[serde(default)]
        content: Vec<ContentBlock>,
    },
    ToolUse {
        name: String,
    },
    ToolResult {
        #[serde(default)]
        is_error: bool,
    },
    ResultError {
        error: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, serde::Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HarnessOptions;

    #[test]
    fn test_build_args() {
        let options = HarnessOptions {
            model: Some("claude-sonnet-4-20250514".to_string()),
            permission_mode: Some("default".to_string()),
            tools: Some("all".to_string()),
            ..Default::default()
        };

        let harness = ClaudeHarness::new(&options);
        let args = harness.build_args(Some("session-123"));

        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"claude-sonnet-4-20250514".to_string()));
        assert!(args.contains(&"--continue".to_string()));
        assert!(args.contains(&"session-123".to_string()));
    }
}
