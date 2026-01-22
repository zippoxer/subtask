//! Error types for Subtask
//!
//! Centralized error handling with security-focused validation errors.

use std::path::PathBuf;
use thiserror::Error;

/// Main error type for Subtask operations
#[derive(Error, Debug)]
pub enum SubtaskError {
    // ============================================
    // Security-related errors (high priority)
    // ============================================
    #[error("invalid task name: {reason}")]
    InvalidTaskName { name: String, reason: String },

    #[error("path traversal attempt detected: {path}")]
    PathTraversal { path: String },

    #[error("invalid git reference: {reference} - {reason}")]
    InvalidGitReference { reference: String, reason: String },

    // ============================================
    // Task errors
    // ============================================
    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("task already exists: {0}")]
    TaskAlreadyExists(String),

    #[error("task is not open: {name} (status: {status})")]
    TaskNotOpen { name: String, status: String },

    #[error("task has no workspace assigned")]
    NoWorkspace { task_name: String },

    // ============================================
    // Workspace errors
    // ============================================
    #[error("no workspaces available (limit: {limit})")]
    NoWorkspacesAvailable { limit: usize },

    #[error("workspace not found: {0}")]
    WorkspaceNotFound(PathBuf),

    #[error("workspace is in use by task: {0}")]
    WorkspaceInUse(String),

    // ============================================
    // Git errors
    // ============================================
    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("git command failed: {command} - {message}")]
    GitCommand { command: String, message: String },

    #[error("branch not found: {0}")]
    BranchNotFound(String),

    #[error("merge conflict in files: {files:?}")]
    MergeConflict { files: Vec<String> },

    // ============================================
    // Harness errors
    // ============================================
    #[error("harness not found: {0}")]
    HarnessNotFound(String),

    #[error("harness execution failed: {harness} - {message}")]
    HarnessExecution { harness: String, message: String },

    #[error("worker is already running for task: {0}")]
    WorkerAlreadyRunning(String),

    #[error("session not found: {0}")]
    SessionNotFound(String),

    // ============================================
    // Config errors
    // ============================================
    #[error("config not found - run 'subtask init' first")]
    ConfigNotFound,

    #[error("invalid config: {0}")]
    InvalidConfig(String),

    // ============================================
    // I/O and system errors
    // ============================================
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    // ============================================
    // History/Event sourcing errors
    // ============================================
    #[error("history file corrupted: {path} - {reason}")]
    HistoryCorrupted { path: PathBuf, reason: String },

    #[error("invalid event: {0}")]
    InvalidEvent(String),
}

/// Result type alias for Subtask operations
pub type Result<T> = std::result::Result<T, SubtaskError>;

// ============================================
// Security validation helpers
// ============================================

/// Validates a task name for security issues
///
/// # Security
/// - Prevents path traversal attacks
/// - Prevents git option injection
/// - Ensures name is a valid single path component
pub fn validate_task_name(name: &str) -> Result<()> {
    // Check for empty name
    if name.is_empty() {
        return Err(SubtaskError::InvalidTaskName {
            name: name.to_string(),
            reason: "task name cannot be empty".to_string(),
        });
    }

    // Check for path traversal attempts
    if name.contains("..") {
        return Err(SubtaskError::PathTraversal {
            path: name.to_string(),
        });
    }

    // Check for absolute paths
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(SubtaskError::PathTraversal {
            path: name.to_string(),
        });
    }

    // Check for git option injection (names starting with -)
    if name.starts_with('-') {
        return Err(SubtaskError::InvalidTaskName {
            name: name.to_string(),
            reason: "task name cannot start with '-' (would be interpreted as git option)".to_string(),
        });
    }

    // Check for reserved sequences
    if name.contains("--") {
        return Err(SubtaskError::InvalidTaskName {
            name: name.to_string(),
            reason: "task name cannot contain '--' (used for path escaping)".to_string(),
        });
    }

    // Check for null bytes
    if name.contains('\0') {
        return Err(SubtaskError::InvalidTaskName {
            name: name.to_string(),
            reason: "task name cannot contain null bytes".to_string(),
        });
    }

    // Validate that escaping and unescaping roundtrips correctly
    // This ensures the name doesn't contain characters that would
    // cause path confusion after escaping
    let escaped = escape_task_name(name);
    let unescaped = unescape_task_name(&escaped);
    if unescaped != name {
        return Err(SubtaskError::InvalidTaskName {
            name: name.to_string(),
            reason: "task name contains invalid characters".to_string(),
        });
    }

    Ok(())
}

/// Escapes a task name for use in file paths
///
/// Converts `/` to `--` for hierarchical task names like `fix/auth-bug`
pub fn escape_task_name(name: &str) -> String {
    name.replace('/', "--")
}

/// Unescapes a task name from file path format
pub fn unescape_task_name(escaped: &str) -> String {
    escaped.replace("--", "/")
}

/// Validates a git reference (branch name, commit, etc.)
///
/// # Security
/// - Prevents option injection
pub fn validate_git_reference(reference: &str) -> Result<()> {
    if reference.starts_with('-') {
        return Err(SubtaskError::InvalidGitReference {
            reference: reference.to_string(),
            reason: "reference cannot start with '-' (would be interpreted as git option)".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_task_names() {
        assert!(validate_task_name("fix/auth-bug").is_ok());
        assert!(validate_task_name("feature/new-login").is_ok());
        assert!(validate_task_name("simple-task").is_ok());
        assert!(validate_task_name("task_with_underscore").is_ok());
    }

    #[test]
    fn test_path_traversal_prevention() {
        assert!(validate_task_name("../../../etc/passwd").is_err());
        assert!(validate_task_name("foo/../bar").is_err());
        assert!(validate_task_name("..").is_err());
    }

    #[test]
    fn test_git_option_injection_prevention() {
        assert!(validate_task_name("-f").is_err());
        assert!(validate_task_name("--force").is_err());
        assert!(validate_task_name("-").is_err());
    }

    #[test]
    fn test_reserved_sequences() {
        assert!(validate_task_name("foo--bar").is_err());
    }

    #[test]
    fn test_escape_roundtrip() {
        let name = "fix/auth-bug";
        let escaped = escape_task_name(name);
        let unescaped = unescape_task_name(&escaped);
        assert_eq!(name, unescaped);
        assert_eq!(escaped, "fix--auth-bug");
    }
}
