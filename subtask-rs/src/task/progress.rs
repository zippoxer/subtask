//! Progress tracking for tasks
//!
//! PROGRESS.json is updated by workers to report their progress.
//! This allows the lead to track worker activity without interrupting them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::Result;

/// Progress information reported by a worker
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Progress {
    /// Current status message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Percentage complete (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,

    /// List of completed items
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completed: Vec<String>,

    /// List of pending items
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<String>,

    /// List of blocked items
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked: Vec<String>,

    /// When progress was last updated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,

    /// Number of tool calls made
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<u32>,
}

impl Progress {
    /// Creates a new empty progress
    pub fn new() -> Self {
        Progress::default()
    }

    /// Loads progress from a file
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Progress::new());
        }

        let content = std::fs::read_to_string(path)?;
        let progress: Progress = serde_json::from_str(&content)?;
        Ok(progress)
    }

    /// Saves progress to a file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Updates the status message
    pub fn set_status(&mut self, status: &str) {
        self.status = Some(status.to_string());
        self.updated_at = Some(Utc::now());
    }

    /// Updates the percentage
    pub fn set_percent(&mut self, percent: u8) {
        self.percent = Some(percent.min(100));
        self.updated_at = Some(Utc::now());
    }

    /// Marks an item as completed
    pub fn mark_completed(&mut self, item: &str) {
        // Remove from pending if present
        self.pending.retain(|i| i != item);
        // Remove from blocked if present
        self.blocked.retain(|i| i != item);
        // Add to completed if not already there
        if !self.completed.contains(&item.to_string()) {
            self.completed.push(item.to_string());
        }
        self.updated_at = Some(Utc::now());
    }

    /// Adds a pending item
    pub fn add_pending(&mut self, item: &str) {
        if !self.pending.contains(&item.to_string()) {
            self.pending.push(item.to_string());
        }
        self.updated_at = Some(Utc::now());
    }

    /// Marks an item as blocked
    pub fn mark_blocked(&mut self, item: &str) {
        // Remove from pending if present
        self.pending.retain(|i| i != item);
        // Add to blocked if not already there
        if !self.blocked.contains(&item.to_string()) {
            self.blocked.push(item.to_string());
        }
        self.updated_at = Some(Utc::now());
    }

    /// Increments the tool call counter
    pub fn increment_tool_calls(&mut self) {
        self.tool_calls = Some(self.tool_calls.unwrap_or(0) + 1);
        self.updated_at = Some(Utc::now());
    }

    /// Returns a summary string
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if let Some(status) = &self.status {
            parts.push(status.clone());
        }

        if let Some(percent) = self.percent {
            parts.push(format!("{}%", percent));
        }

        let completed = self.completed.len();
        let pending = self.pending.len();
        let blocked = self.blocked.len();

        if completed > 0 || pending > 0 {
            parts.push(format!("{}/{} done", completed, completed + pending));
        }

        if blocked > 0 {
            parts.push(format!("{} blocked", blocked));
        }

        if let Some(tool_calls) = self.tool_calls {
            parts.push(format!("{} tools", tool_calls));
        }

        if parts.is_empty() {
            "No progress reported".to_string()
        } else {
            parts.join(" • ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_progress_roundtrip() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("PROGRESS.json");

        let mut progress = Progress::new();
        progress.set_status("Working on auth");
        progress.set_percent(50);
        progress.add_pending("Fix login");
        progress.add_pending("Fix logout");
        progress.mark_completed("Fix signup");

        progress.save(&path).unwrap();

        let loaded = Progress::load(&path).unwrap();
        assert_eq!(loaded.status, Some("Working on auth".to_string()));
        assert_eq!(loaded.percent, Some(50));
        assert_eq!(loaded.completed.len(), 1);
        assert_eq!(loaded.pending.len(), 2);
    }

    #[test]
    fn test_progress_summary() {
        let mut progress = Progress::new();
        progress.set_status("Working");
        progress.set_percent(75);
        progress.mark_completed("task1");
        progress.add_pending("task2");

        let summary = progress.summary();
        assert!(summary.contains("Working"));
        assert!(summary.contains("75%"));
        assert!(summary.contains("1/2 done"));
    }
}
