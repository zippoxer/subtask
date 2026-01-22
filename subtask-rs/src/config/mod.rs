//! Configuration management for Subtask
//!
//! Handles project-level configuration stored in `.subtask/config.json`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{Result, SubtaskError};

/// Project configuration stored in `.subtask/config.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// The harness to use (claude, codex, opencode)
    pub harness: String,

    /// Maximum number of concurrent workspaces
    #[serde(default = "default_max_workspaces")]
    pub max_workspaces: usize,

    /// Harness-specific options
    #[serde(default)]
    pub options: HarnessOptions,
}

fn default_max_workspaces() -> usize {
    4
}

/// Harness-specific configuration options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HarnessOptions {
    /// Model identifier (e.g., "claude-sonnet-4-20250514", "o3")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Permission mode for Claude harness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,

    /// Tools configuration for Claude harness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<String>,

    /// Reasoning level for Codex harness (low, medium, high)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    /// Agent type for OpenCode harness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// Variant for OpenCode harness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,

    /// Additional arbitrary options
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Config {
    /// Loads configuration from the project directory
    pub fn load(project_dir: &Path) -> Result<Self> {
        let config_path = project_dir.join(".subtask").join("config.json");

        if !config_path.exists() {
            return Err(SubtaskError::ConfigNotFound);
        }

        let content = std::fs::read_to_string(&config_path)?;
        let config: Config = serde_json::from_str(&content)?;

        // Validate configuration
        config.validate()?;

        Ok(config)
    }

    /// Saves configuration to the project directory
    pub fn save(&self, project_dir: &Path) -> Result<()> {
        let subtask_dir = project_dir.join(".subtask");
        std::fs::create_dir_all(&subtask_dir)?;

        let config_path = subtask_dir.join("config.json");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;

        Ok(())
    }

    /// Validates the configuration
    fn validate(&self) -> Result<()> {
        // Validate harness name
        let valid_harnesses = ["claude", "codex", "opencode"];
        if !valid_harnesses.contains(&self.harness.as_str()) {
            return Err(SubtaskError::InvalidConfig(format!(
                "unknown harness '{}', expected one of: {}",
                self.harness,
                valid_harnesses.join(", ")
            )));
        }

        // Validate max_workspaces
        if self.max_workspaces == 0 {
            return Err(SubtaskError::InvalidConfig(
                "max_workspaces must be at least 1".to_string(),
            ));
        }

        if self.max_workspaces > 32 {
            return Err(SubtaskError::InvalidConfig(
                "max_workspaces cannot exceed 32".to_string(),
            ));
        }

        Ok(())
    }

    /// Creates a default configuration for a given harness
    pub fn default_for_harness(harness: &str) -> Self {
        Config {
            harness: harness.to_string(),
            max_workspaces: default_max_workspaces(),
            options: HarnessOptions::default(),
        }
    }
}

/// Finds the project root by looking for `.subtask` directory or `.git`
pub fn find_project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    find_project_root_from(&cwd)
}

/// Finds the project root starting from a given directory
pub fn find_project_root_from(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();

    loop {
        // Check for .subtask directory (project already initialized)
        if current.join(".subtask").is_dir() {
            return Ok(current);
        }

        // Check for .git directory (git repository root)
        if current.join(".git").exists() {
            return Ok(current);
        }

        // Move to parent directory
        if !current.pop() {
            return Err(SubtaskError::ConfigNotFound);
        }
    }
}

/// Returns the path to the global subtask directory (~/.subtask)
pub fn global_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| {
        SubtaskError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not determine home directory",
        ))
    })?;

    Ok(home.join(".subtask"))
}

/// Returns the path to the global workspaces directory (~/.subtask/workspaces)
pub fn global_workspaces_dir() -> Result<PathBuf> {
    Ok(global_dir()?.join("workspaces"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_roundtrip() {
        let temp = TempDir::new().unwrap();
        let config = Config {
            harness: "claude".to_string(),
            max_workspaces: 4,
            options: HarnessOptions {
                model: Some("claude-sonnet-4-20250514".to_string()),
                permission_mode: Some("default".to_string()),
                ..Default::default()
            },
        };

        config.save(temp.path()).unwrap();
        let loaded = Config::load(temp.path()).unwrap();

        assert_eq!(loaded.harness, "claude");
        assert_eq!(loaded.max_workspaces, 4);
        assert_eq!(
            loaded.options.model,
            Some("claude-sonnet-4-20250514".to_string())
        );
    }

    #[test]
    fn test_invalid_harness() {
        let config = Config {
            harness: "invalid".to_string(),
            max_workspaces: 4,
            options: HarnessOptions::default(),
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_max_workspaces() {
        let config = Config {
            harness: "claude".to_string(),
            max_workspaces: 0,
            options: HarnessOptions::default(),
        };

        assert!(config.validate().is_err());
    }
}
