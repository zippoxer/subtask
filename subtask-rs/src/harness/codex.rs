//! OpenAI Codex harness implementation

use async_trait::async_trait;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use super::{build_review_prompt, Callbacks, Harness, ReviewTarget, RunResult};
use crate::config::HarnessOptions;
use crate::error::{Result, SubtaskError};

/// OpenAI Codex CLI harness
pub struct CodexHarness {
    model: Option<String>,
    reasoning: Option<String>,
}

impl CodexHarness {
    /// Creates a new Codex harness from options
    pub fn new(options: &HarnessOptions) -> Self {
        CodexHarness {
            model: options.model.clone(),
            reasoning: options.reasoning.clone(),
        }
    }

    fn build_args(&self, continue_from: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "--output-format".to_string(),
            "json".to_string(),
            // ⚠️ Security note: This bypasses approvals and sandbox
            "--dangerously-bypass-approvals-and-sandbox".to_string(),
        ];

        // Model
        if let Some(ref model) = self.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        // Reasoning level
        if let Some(ref reasoning) = self.reasoning {
            args.push("--reasoning".to_string());
            args.push(reasoning.clone());
        }

        // Continue from session
        if let Some(session) = continue_from {
            args.push("--session".to_string());
            args.push(session.to_string());
        }

        args
    }
}

#[async_trait]
impl Harness for CodexHarness {
    async fn run(
        &self,
        cwd: &Path,
        prompt: &str,
        continue_from: Option<&str>,
        callbacks: Callbacks,
    ) -> Result<RunResult> {
        let args = self.build_args(continue_from);

        let mut cmd = Command::new("codex");
        cmd.current_dir(cwd)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let start = Instant::now();

        let mut child = cmd.spawn().map_err(|e| SubtaskError::HarnessExecution {
            harness: "codex".to_string(),
            message: format!("failed to start codex: {}", e),
        })?;

        // Send prompt via stdin
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(prompt.as_bytes());
        }

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        let mut result = RunResult {
            session_id: continue_from.map(String::from),
            prompt_delivered: continue_from.is_some(),
            ..Default::default()
        };

        let mut reply = String::new();

        // Parse JSON output
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            if line.trim().is_empty() {
                continue;
            }

            if let Ok(event) = serde_json::from_str::<CodexEvent>(&line) {
                match event {
                    CodexEvent::SessionStart { id } => {
                        result.session_id = Some(id.clone());
                        result.prompt_delivered = true;
                        if let Some(ref cb) = callbacks.on_session_start {
                            cb(&id);
                        }
                    }
                    CodexEvent::Message { content, .. } => {
                        reply.push_str(&content);
                        result.reply = reply.clone();
                        result.agent_replied = true;
                        if let Some(ref cb) = callbacks.on_text {
                            cb(&content);
                        }
                    }
                    CodexEvent::ToolCall { .. } => {
                        result.tool_calls += 1;
                        if let Some(ref cb) = callbacks.on_tool_call {
                            cb(chrono::Utc::now());
                        }
                    }
                    CodexEvent::Error { message } => {
                        result.error = Some(message);
                    }
                    _ => {}
                }
            }
        }

        let status = child.wait().map_err(|e| SubtaskError::HarnessExecution {
            harness: "codex".to_string(),
            message: format!("failed to wait for codex: {}", e),
        })?;

        result.duration_secs = start.elapsed().as_secs_f64();

        if !status.success() && result.error.is_none() {
            result.error = Some(format!("codex exited with status: {}", status));
        }

        if result.error.is_some() {
            return Err(SubtaskError::HarnessExecution {
                harness: "codex".to_string(),
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
        // Codex has a built-in review command
        let target_arg = match &target {
            ReviewTarget::Branch(b) => b.clone(),
            ReviewTarget::Staged => "HEAD".to_string(),
            ReviewTarget::Commit(c) => c.clone(),
        };

        let mut cmd = Command::new("codex");
        cmd.current_dir(cwd)
            .args(["review", &target_arg])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().map_err(|e| SubtaskError::HarnessExecution {
            harness: "codex".to_string(),
            message: format!("failed to run codex review: {}", e),
        })?;

        if !output.status.success() {
            return Err(SubtaskError::HarnessExecution {
                harness: "codex".to_string(),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn migrate_session(
        &self,
        session_id: &str,
        old_cwd: &Path,
        new_cwd: &Path,
    ) -> Result<()> {
        // Codex sessions can be migrated by copying the session file
        let home = dirs::home_dir().ok_or_else(|| SubtaskError::HarnessExecution {
            harness: "codex".to_string(),
            message: "could not determine home directory".to_string(),
        })?;

        let sessions_dir = home.join(".codex").join("sessions");
        let session_file = sessions_dir.join(format!("{}.json", session_id));

        if session_file.exists() {
            // Update the session's cwd field
            // For now, just return Ok - full implementation would modify the file
            tracing::debug!("migrating codex session {} from {:?} to {:?}", session_id, old_cwd, new_cwd);
        }

        Ok(())
    }

    fn duplicate_session(
        &self,
        session_id: &str,
        _old_cwd: &Path,
        _new_cwd: &Path,
    ) -> Result<Option<String>> {
        // Codex doesn't support session duplication directly
        tracing::debug!("codex session duplication not supported for {}", session_id);
        Ok(None)
    }

    fn name(&self) -> &str {
        "codex"
    }
}

/// Codex streaming event types
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CodexEvent {
    SessionStart { id: String },
    Message { content: String, role: String },
    ToolCall { name: String },
    ToolResult { success: bool },
    Error { message: String },
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HarnessOptions;

    #[test]
    fn test_build_args() {
        let options = HarnessOptions {
            model: Some("o3".to_string()),
            reasoning: Some("high".to_string()),
            ..Default::default()
        };

        let harness = CodexHarness::new(&options);
        let args = harness.build_args(None);

        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"o3".to_string()));
        assert!(args.contains(&"--reasoning".to_string()));
        assert!(args.contains(&"high".to_string()));
    }
}
