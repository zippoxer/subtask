//! Git operations module
//!
//! Handles git operations including branches, worktrees, merges, and diffs.
//! Uses git2 for native bindings where possible, with subprocess fallback
//! for operations not supported by libgit2.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{validate_git_reference, Result, SubtaskError};

/// Git repository wrapper providing safe operations
pub struct Repository {
    path: PathBuf,
    repo: git2::Repository,
}

impl Repository {
    /// Opens a repository at the given path
    pub fn open(path: &Path) -> Result<Self> {
        let repo = git2::Repository::open(path)?;
        Ok(Repository {
            path: path.to_path_buf(),
            repo,
        })
    }

    /// Discovers and opens the repository containing the given path
    pub fn discover(path: &Path) -> Result<Self> {
        let repo = git2::Repository::discover(path)?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| SubtaskError::GitCommand {
                command: "discover".to_string(),
                message: "repository has no working directory".to_string(),
            })?
            .to_path_buf();

        Ok(Repository { path: workdir, repo })
    }

    /// Returns the repository path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the current branch name
    pub fn current_branch(&self) -> Result<Option<String>> {
        let head = match self.repo.head() {
            Ok(head) => head,
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        if head.is_branch() {
            Ok(head.shorthand().map(String::from))
        } else {
            Ok(None)
        }
    }

    /// Checks if a branch exists
    pub fn branch_exists(&self, name: &str) -> Result<bool> {
        validate_git_reference(name)?;

        match self.repo.find_branch(name, git2::BranchType::Local) {
            Ok(_) => Ok(true),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Creates a new branch from a starting point
    ///
    /// # Security
    /// Uses `--` to prevent option injection
    pub fn create_branch(&self, name: &str, start_point: &str) -> Result<()> {
        validate_git_reference(name)?;
        validate_git_reference(start_point)?;

        // Use git command with -- to prevent option injection
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(["branch", "--", name, start_point])
            .output()?;

        if !output.status.success() {
            return Err(SubtaskError::GitCommand {
                command: format!("git branch -- {} {}", name, start_point),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(())
    }

    /// Switches to a branch
    ///
    /// # Security
    /// Uses `--` to prevent option injection
    pub fn checkout(&self, branch: &str) -> Result<()> {
        validate_git_reference(branch)?;

        let output = Command::new("git")
            .current_dir(&self.path)
            .args(["checkout", "--", branch])
            .output()?;

        if !output.status.success() {
            return Err(SubtaskError::GitCommand {
                command: format!("git checkout -- {}", branch),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(())
    }

    /// Creates and switches to a new branch
    ///
    /// # Security
    /// Uses `--` to prevent option injection
    pub fn switch_create(&self, name: &str, start_point: &str) -> Result<()> {
        validate_git_reference(name)?;
        validate_git_reference(start_point)?;

        let output = Command::new("git")
            .current_dir(&self.path)
            .args(["switch", "-c", "--", name, start_point])
            .output()?;

        if !output.status.success() {
            return Err(SubtaskError::GitCommand {
                command: format!("git switch -c -- {} {}", name, start_point),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(())
    }

    /// Squash merges a branch into the current branch
    pub fn merge_squash(&self, branch: &str, message: &str) -> Result<String> {
        validate_git_reference(branch)?;

        // First, do the squash merge
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(["merge", "--squash", "--", branch])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("CONFLICT") {
                // Extract conflict files
                let conflicts = self.get_conflict_files()?;
                return Err(SubtaskError::MergeConflict { files: conflicts });
            }
            return Err(SubtaskError::GitCommand {
                command: format!("git merge --squash -- {}", branch),
                message: stderr.to_string(),
            });
        }

        // Then commit
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(["commit", "-m", message])
            .output()?;

        if !output.status.success() {
            return Err(SubtaskError::GitCommand {
                command: "git commit".to_string(),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        // Get the commit hash
        self.head_commit()
    }

    /// Gets the current HEAD commit hash
    pub fn head_commit(&self) -> Result<String> {
        let head = self.repo.head()?;
        let commit = head.peel_to_commit()?;
        Ok(commit.id().to_string())
    }

    /// Gets a list of files with conflicts
    fn get_conflict_files(&self) -> Result<Vec<String>> {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(["diff", "--name-only", "--diff-filter=U"])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().map(String::from).collect())
    }

    /// Deletes a branch
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        validate_git_reference(name)?;

        let flag = if force { "-D" } else { "-d" };
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(["branch", flag, "--", name])
            .output()?;

        if !output.status.success() {
            return Err(SubtaskError::GitCommand {
                command: format!("git branch {} -- {}", flag, name),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(())
    }

    /// Gets diff statistics between two refs
    pub fn diff_stats(&self, base: &str, head: &str) -> Result<DiffStats> {
        validate_git_reference(base)?;
        validate_git_reference(head)?;

        let output = Command::new("git")
            .current_dir(&self.path)
            .args(["diff", "--stat", "--", &format!("{}...{}", base, head)])
            .output()?;

        if !output.status.success() {
            return Err(SubtaskError::GitCommand {
                command: format!("git diff --stat -- {}...{}", base, head),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(DiffStats::parse(&stdout))
    }

    /// Gets the number of commits between base and head
    pub fn commit_count(&self, base: &str, head: &str) -> Result<usize> {
        validate_git_reference(base)?;
        validate_git_reference(head)?;

        let output = Command::new("git")
            .current_dir(&self.path)
            .args([
                "rev-list",
                "--count",
                "--",
                &format!("{}..{}", base, head),
            ])
            .output()?;

        if !output.status.success() {
            return Err(SubtaskError::GitCommand {
                command: format!("git rev-list --count -- {}..{}", base, head),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        let count: usize = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        Ok(count)
    }
}

/// Statistics about a diff
#[derive(Debug, Default)]
pub struct DiffStats {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

impl DiffStats {
    /// Parses diff stats from git diff --stat output
    fn parse(output: &str) -> Self {
        let mut stats = DiffStats::default();

        // Look for the summary line like "3 files changed, 10 insertions(+), 5 deletions(-)"
        for line in output.lines() {
            if line.contains("file") && line.contains("changed") {
                // Parse files changed
                if let Some(files) = line.split_whitespace().next() {
                    stats.files_changed = files.parse().unwrap_or(0);
                }

                // Parse insertions
                if let Some(pos) = line.find("insertion") {
                    let before = &line[..pos];
                    if let Some(num) = before.split_whitespace().last() {
                        stats.insertions = num.parse().unwrap_or(0);
                    }
                }

                // Parse deletions
                if let Some(pos) = line.find("deletion") {
                    let before = &line[..pos];
                    if let Some(num) = before.split_whitespace().last() {
                        stats.deletions = num.parse().unwrap_or(0);
                    }
                }
            }
        }

        stats
    }
}

// ============================================
// Worktree operations
// ============================================

/// Adds a new worktree
///
/// # Security
/// Uses `--` to prevent option injection
pub fn add_worktree(repo_path: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    validate_git_reference(branch)?;

    let output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "worktree",
            "add",
            "--detach",
            "--",
            worktree_path.to_str().unwrap_or(""),
        ])
        .output()?;

    if !output.status.success() {
        return Err(SubtaskError::GitCommand {
            command: "git worktree add".to_string(),
            message: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    Ok(())
}

/// Removes a worktree
pub fn remove_worktree(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "worktree",
            "remove",
            "--force",
            "--",
            worktree_path.to_str().unwrap_or(""),
        ])
        .output()?;

    if !output.status.success() {
        // Worktree might not exist, which is fine
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("is not a working tree") {
            return Err(SubtaskError::GitCommand {
                command: "git worktree remove".to_string(),
                message: stderr.to_string(),
            });
        }
    }

    Ok(())
}

/// Lists all worktrees
pub fn list_worktrees(repo_path: &Path) -> Result<Vec<WorktreeInfo>> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["worktree", "list", "--porcelain"])
        .output()?;

    if !output.status.success() {
        return Err(SubtaskError::GitCommand {
            command: "git worktree list".to_string(),
            message: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_worktree_list(&stdout))
}

/// Information about a worktree
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub is_bare: bool,
}

fn parse_worktree_list(output: &str) -> Vec<WorktreeInfo> {
    let mut worktrees = Vec::new();
    let mut current = WorktreeInfo {
        path: PathBuf::new(),
        head: String::new(),
        branch: None,
        is_bare: false,
    };

    for line in output.lines() {
        if line.is_empty() {
            if !current.path.as_os_str().is_empty() {
                worktrees.push(current.clone());
            }
            current = WorktreeInfo {
                path: PathBuf::new(),
                head: String::new(),
                branch: None,
                is_bare: false,
            };
            continue;
        }

        if let Some(path) = line.strip_prefix("worktree ") {
            current.path = PathBuf::from(path);
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            current.head = head.to_string();
        } else if let Some(branch) = line.strip_prefix("branch ") {
            current.branch = Some(branch.to_string());
        } else if line == "bare" {
            current.is_bare = true;
        }
    }

    // Don't forget the last one
    if !current.path.as_os_str().is_empty() {
        worktrees.push(current);
    }

    worktrees
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_stats_parsing() {
        let output = " 3 files changed, 45 insertions(+), 12 deletions(-)";
        let stats = DiffStats::parse(output);
        assert_eq!(stats.files_changed, 3);
        assert_eq!(stats.insertions, 45);
        assert_eq!(stats.deletions, 12);
    }

    #[test]
    fn test_worktree_list_parsing() {
        let output = "worktree /home/user/repo\nHEAD abc123\nbranch refs/heads/main\n\nworktree /home/user/repo-wt1\nHEAD def456\nbranch refs/heads/feature\n";
        let worktrees = parse_worktree_list(output);
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[0].path, PathBuf::from("/home/user/repo"));
        assert_eq!(worktrees[1].branch, Some("refs/heads/feature".to_string()));
    }

    #[test]
    fn test_git_reference_validation() {
        assert!(validate_git_reference("main").is_ok());
        assert!(validate_git_reference("feature/test").is_ok());
        assert!(validate_git_reference("-f").is_err());
        assert!(validate_git_reference("--force").is_err());
    }
}
