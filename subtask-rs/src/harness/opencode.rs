//! OpenCode harness implementation

use async_trait::async_trait;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use super::{build_review_prompt, Callbacks, Harness, ReviewTarget, RunResult};
use crate::config::HarnessOptions;
use crate::error::{Result, SubtaskError};

/// OpenCode CLI harness
pub struct OpenCodeHarness {
    model: Option<String>,
    variant: Option<String>,
    agent: Option<String>,
}

impl OpenCodeHarness {
    /// Creates a new OpenCode harness from options
    pub fn new(options: &HarnessOptions) -> Self {
        OpenCodeHarness {
            model: options.model.clone(),
            variant: options.variant.clone(),
            agent: options.agent.clone(),
        }
    }

    fn build_args(&self, continue_from: Option<&str>) -> Vec<String> {
        let mut args = vec!["run".to_string(), "--format".to_string(), "json".to_string()];

        // Model
        if let Some(ref model) = self.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        // Variant
        if let Some(ref variant) = self.variant {
            args.push("--variant".to_string());
            args.push(variant.clone());
        }

        // Agent
        if let Some(ref agent) = self.agent {
            args.push("--agent".to_string());
            args.push(agent.clone());
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
impl Harness for OpenCodeHarness {
    async fn run(
        &self,
        cwd: &Path,
        prompt: &str,
        continue_from: Option<&str>,
        callbacks: Callbacks,
    ) -> Result<RunResult> {
        let args = self.build_args(continue_from);

        let mut cmd = Command::new("opencode");
        cmd.current_dir(cwd)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let start = Instant::now();

        let mut child = cmd.spawn().map_err(|e| SubtaskError::HarnessExecution {
            harness: "opencode".to_string(),
            message: format!("failed to start opencode: {}", e),
        })?;

        // Send prompt via stdin (OpenCode re-quotes CLI args with spaces)
        if let Some(mut stdin) = child.stdin.take() {
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

            if let Ok(event) = serde_json::from_str::<OpenCodeEvent>(&line) {
                // Handle session ID
                if let Some(ref session_id) = event.session_id {
                    if result.session_id.is_none() {
                        result.session_id = Some(session_id.clone());
                        result.prompt_delivered = true;
                        if let Some(ref cb) = callbacks.on_session_start {
                            cb(session_id);
                        }
                    }
                }

                match event.event_type.as_str() {
                    "tool_use" => {
                        result.tool_calls += 1;
                        if let Some(ref cb) = callbacks.on_tool_call {
                            cb(chrono::Utc::now());
                        }
                    }
                    "text" => {
                        if let Some(ref part) = event.part {
                            if let Some(ref text) = part.text {
                                reply.push_str(text);
                                result.reply = reply.clone();
                                result.agent_replied = !result.reply.is_empty();
                                if let Some(ref cb) = callbacks.on_text {
                                    cb(text);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let status = child.wait().map_err(|e| SubtaskError::HarnessExecution {
            harness: "opencode".to_string(),
            message: format!("failed to wait for opencode: {}", e),
        })?;

        result.duration_secs = start.elapsed().as_secs_f64();

        if !status.success() && result.error.is_none() {
            result.error = Some(format!("opencode exited with status: {}", status));
        }

        if result.error.is_some() {
            return Err(SubtaskError::HarnessExecution {
                harness: "opencode".to_string(),
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
        // OpenCode sessions are resumable across directories
        Ok(())
    }

    fn duplicate_session(
        &self,
        session_id: &str,
        _old_cwd: &Path,
        _new_cwd: &Path,
    ) -> Result<Option<String>> {
        // OpenCode doesn't support session duplication
        tracing::debug!(
            "opencode does not support session duplication for {}",
            session_id
        );
        Err(SubtaskError::HarnessExecution {
            harness: "opencode".to_string(),
            message: "opencode does not support session duplication".to_string(),
        })
    }

    fn name(&self) -> &str {
        "opencode"
    }
}

/// OpenCode streaming event
#[derive(Debug, serde::Deserialize)]
struct OpenCodeEvent {
    #[serde(rename = "type")]
    event_type: String,

    #[serde(rename = "sessionID")]
    session_id: Option<String>,

    part: Option<OpenCodePart>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenCodePart {
    id: Option<String>,
    #[serde(rename = "type")]
    part_type: Option<String>,
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HarnessOptions;

    #[test]
    fn test_build_args() {
        let options = HarnessOptions {
            model: Some("gpt-4-turbo".to_string()),
            variant: Some("default".to_string()),
            agent: Some("coder".to_string()),
            ..Default::default()
        };

        let harness = OpenCodeHarness::new(&options);
        let args = harness.build_args(Some("session-abc"));

        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"gpt-4-turbo".to_string()));
        assert!(args.contains(&"--session".to_string()));
        assert!(args.contains(&"session-abc".to_string()));
    }
}
