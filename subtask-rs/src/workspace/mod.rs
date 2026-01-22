//! Workspace pool management
//!
//! Workspaces are git worktrees that provide isolated environments for tasks.
//! The pool manages allocation, tracking, and cleanup of workspaces.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{global_workspaces_dir, Config};
use crate::error::{Result, SubtaskError};
use crate::git;

/// A workspace entry in the pool
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Unique identifier for this workspace
    pub id: String,

    /// Path to the worktree
    pub path: PathBuf,

    /// Task currently using this workspace (if any)
    pub task: Option<String>,
}

/// Manages the workspace pool
pub struct Pool {
    /// Path to the main repository
    repo_path: PathBuf,

    /// Path to the workspaces directory
    workspaces_dir: PathBuf,

    /// Maximum number of workspaces
    max_workspaces: usize,
}

impl Pool {
    /// Creates a new workspace pool
    pub fn new(repo_path: &Path, config: &Config) -> Result<Self> {
        let workspaces_dir = global_workspaces_dir()?;
        fs::create_dir_all(&workspaces_dir)?;

        Ok(Pool {
            repo_path: repo_path.to_path_buf(),
            workspaces_dir,
            max_workspaces: config.max_workspaces,
        })
    }

    /// Lists all workspaces in the pool
    pub fn list(&self) -> Result<Vec<Workspace>> {
        let mut workspaces = Vec::new();

        if !self.workspaces_dir.exists() {
            return Ok(workspaces);
        }

        // Get project-specific workspace prefix
        let prefix = self.project_prefix()?;

        for entry in fs::read_dir(&self.workspaces_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Only include workspaces for this project
                if !name.starts_with(&prefix) {
                    continue;
                }

                let id = name.to_string();
                let task = self.read_task_assignment(&path)?;

                workspaces.push(Workspace { id, path, task });
            }
        }

        Ok(workspaces)
    }

    /// Acquires a workspace for a task
    ///
    /// Returns an existing free workspace or creates a new one if under the limit.
    pub fn acquire(&self, task_name: &str) -> Result<Workspace> {
        self.acquire_excluding(task_name, None)
    }

    /// Acquires a workspace, excluding a specific path
    ///
    /// Used when reviving a task to get a fresh workspace.
    pub fn acquire_excluding(
        &self,
        task_name: &str,
        exclude: Option<&Path>,
    ) -> Result<Workspace> {
        let workspaces = self.list()?;

        // First, try to find a free workspace
        for ws in &workspaces {
            if ws.task.is_none() {
                if let Some(excl) = exclude {
                    if ws.path == excl {
                        continue;
                    }
                }
                // Assign to task
                self.assign_task(&ws.path, task_name)?;
                return Ok(Workspace {
                    task: Some(task_name.to_string()),
                    ..ws.clone()
                });
            }
        }

        // Check if we can create a new workspace
        if workspaces.len() >= self.max_workspaces {
            return Err(SubtaskError::NoWorkspacesAvailable {
                limit: self.max_workspaces,
            });
        }

        // Create a new workspace
        let ws = self.create_workspace()?;
        self.assign_task(&ws.path, task_name)?;

        Ok(Workspace {
            task: Some(task_name.to_string()),
            ..ws
        })
    }

    /// Releases a workspace from a task
    pub fn release(&self, workspace_path: &Path) -> Result<()> {
        let task_file = workspace_path.join(".subtask-task");
        if task_file.exists() {
            fs::remove_file(&task_file)?;
        }

        // Detach HEAD to avoid branch conflicts
        let _ = std::process::Command::new("git")
            .current_dir(workspace_path)
            .args(["checkout", "--detach", "HEAD"])
            .output();

        Ok(())
    }

    /// Creates a new workspace
    fn create_workspace(&self) -> Result<Workspace> {
        let id = self.generate_workspace_id()?;
        let path = self.workspaces_dir.join(&id);

        // Create the worktree
        git::add_worktree(&self.repo_path, &path, "HEAD")?;

        Ok(Workspace {
            id,
            path,
            task: None,
        })
    }

    /// Generates a unique workspace ID
    fn generate_workspace_id(&self) -> Result<String> {
        let prefix = self.project_prefix()?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        Ok(format!("{}-{}", prefix, timestamp))
    }

    /// Gets the project prefix for workspace naming
    fn project_prefix(&self) -> Result<String> {
        // Use a hash of the repo path for uniqueness
        let path_str = self.repo_path.to_string_lossy();
        let hash = simple_hash(&path_str);
        Ok(format!("ws-{:08x}", hash))
    }

    /// Assigns a task to a workspace
    fn assign_task(&self, workspace_path: &Path, task_name: &str) -> Result<()> {
        let task_file = workspace_path.join(".subtask-task");
        fs::write(&task_file, task_name)?;
        Ok(())
    }

    /// Reads the task assignment for a workspace
    fn read_task_assignment(&self, workspace_path: &Path) -> Result<Option<String>> {
        let task_file = workspace_path.join(".subtask-task");
        if task_file.exists() {
            let content = fs::read_to_string(&task_file)?;
            let task = content.trim();
            if task.is_empty() {
                Ok(None)
            } else {
                Ok(Some(task.to_string()))
            }
        } else {
            Ok(None)
        }
    }

    /// Removes a workspace entirely
    pub fn remove(&self, workspace_path: &Path) -> Result<()> {
        git::remove_worktree(&self.repo_path, workspace_path)?;

        // Also remove the directory if it still exists
        if workspace_path.exists() {
            fs::remove_dir_all(workspace_path)?;
        }

        Ok(())
    }

    /// Cleans up orphaned workspaces (those with no valid task)
    pub fn cleanup(&self) -> Result<usize> {
        let workspaces = self.list()?;
        let mut cleaned = 0;

        for ws in workspaces {
            if ws.task.is_none() {
                // Check if it's been idle for a while
                // For now, just count it
                cleaned += 1;
            }
        }

        Ok(cleaned)
    }
}

/// Simple hash function for generating workspace prefixes
fn simple_hash(s: &str) -> u32 {
    let mut hash: u32 = 5381;
    for c in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(c as u32);
    }
    hash
}

/// Ensures the task symlink exists in the workspace
///
/// Creates: `<workspace>/.subtask/tasks/<escaped-name>` -> `<main-repo>/.subtask/tasks/<escaped-name>`
///
/// # Security
/// Uses atomic operations where possible to prevent TOCTOU issues
pub fn ensure_task_symlink(
    workspace_path: &Path,
    project_dir: &Path,
    task_name: &str,
) -> Result<()> {
    use crate::error::escape_task_name;

    let escaped = escape_task_name(task_name);
    let task_dir_abs = project_dir.join(".subtask").join("tasks").join(&escaped);
    let ws_tasks_dir = workspace_path.join(".subtask").join("tasks");
    let ws_task_dir = ws_tasks_dir.join(&escaped);

    fs::create_dir_all(&ws_tasks_dir)?;

    // Security: Use atomic symlink creation
    // First create a temporary symlink, then rename it into place
    let temp_link = ws_tasks_dir.join(format!(".{}.tmp", escaped));

    // Remove any existing temp link
    let _ = fs::remove_file(&temp_link);

    // Create the symlink
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&task_dir_abs, &temp_link)?;
    }

    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(&task_dir_abs, &temp_link)?;
    }

    // Atomically rename into place
    // Note: This isn't truly atomic on all filesystems, but it's better than remove+create
    let _ = fs::remove_file(&ws_task_dir);
    fs::rename(&temp_link, &ws_task_dir)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_hash() {
        let hash1 = simple_hash("/home/user/project1");
        let hash2 = simple_hash("/home/user/project2");
        assert_ne!(hash1, hash2);

        // Same input should give same output
        let hash3 = simple_hash("/home/user/project1");
        assert_eq!(hash1, hash3);
    }
}
