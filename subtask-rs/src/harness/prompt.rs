//! Prompt building utilities
//!
//! Constructs prompts for workers with task context, instructions, and metadata.

use std::path::Path;

use crate::task::Task;

/// Target for code review
#[derive(Debug, Clone)]
pub enum ReviewTarget {
    /// Review a specific branch
    Branch(String),
    /// Review staged changes
    Staged,
    /// Review a commit
    Commit(String),
}

/// Information about repository staleness
#[derive(Debug, Default)]
pub struct RepoStatus {
    /// Number of commits the base branch is ahead
    pub commits_behind: usize,
    /// Files with conflicts
    pub conflict_files: Vec<String>,
}

/// Builds the full prompt for a worker
pub fn build_prompt(
    task: &Task,
    workspace: &Path,
    same_workspace: bool,
    prompt: &str,
    status: Option<&RepoStatus>,
) -> String {
    let mut sb = String::new();

    // Header
    sb.push_str("# Task\n");
    sb.push_str(&format!("Name: {}\n", task.name));
    sb.push_str(&format!("Title: {}\n", task.title));
    sb.push_str(&format!("Branch: {}\n", task.name));

    // Task directory
    let task_dir = format!(".subtask/tasks/{}", crate::error::escape_task_name(&task.name));
    sb.push_str(&format!("Directory: {}\n", task_dir));

    // Follow-up note
    if let Some(ref follow_up) = task.follow_up {
        sb.push_str(&format!("Follow-up: continuing from {}\n", follow_up));
        if !same_workspace {
            sb.push_str("Note: New workspace, branch checked out fresh.\n");
        }
    }

    // Staleness/conflict warnings
    if let Some(status) = status {
        if status.commits_behind > 0 {
            let word = if status.commits_behind == 1 {
                "commit"
            } else {
                "commits"
            };
            sb.push_str(&format!(
                "Note: {} is {} {} ahead of this task.\n",
                task.base_branch, status.commits_behind, word
            ));
        }

        if !status.conflict_files.is_empty() {
            sb.push_str(&format!(
                "Note: This branch conflicts with {} in: {}. Consider running `git merge {}` to resolve.\n",
                task.base_branch,
                status.conflict_files.join(", "),
                task.base_branch
            ));
        }
    }

    // Description
    if !task.description.is_empty() {
        sb.push_str("\n## Description\n");
        sb.push_str(&task.description);
        sb.push('\n');
    }

    // Separator and user prompt
    sb.push_str("\n--------------------\n\n");
    sb.push_str(prompt);

    sb
}

/// Builds a code review prompt
pub fn build_review_prompt(cwd: &Path, target: &ReviewTarget, instructions: &str) -> String {
    let mut sb = String::new();

    sb.push_str("# Code Review Request\n\n");

    match target {
        ReviewTarget::Branch(branch) => {
            sb.push_str(&format!("Review the changes on branch `{}`.\n", branch));
        }
        ReviewTarget::Staged => {
            sb.push_str("Review the staged changes.\n");
        }
        ReviewTarget::Commit(commit) => {
            sb.push_str(&format!("Review commit `{}`.\n", commit));
        }
    }

    sb.push_str("\n## Instructions\n\n");
    sb.push_str(instructions);
    sb.push_str("\n\n## Review Guidelines\n\n");
    sb.push_str("- Check for bugs, security issues, and code quality\n");
    sb.push_str("- Suggest improvements where appropriate\n");
    sb.push_str("- Be specific about issues and their locations\n");
    sb.push_str("- Provide actionable feedback\n");

    sb
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::Task;
    use std::path::PathBuf;

    #[test]
    fn test_build_prompt() {
        let task = Task::new("fix/auth-bug", "Fix authentication bug", "main").unwrap();
        let workspace = PathBuf::from("/tmp/workspace");
        let prompt = "Please fix the auth bug";

        let result = build_prompt(&task, &workspace, false, prompt, None);

        assert!(result.contains("# Task"));
        assert!(result.contains("Name: fix/auth-bug"));
        assert!(result.contains("Title: Fix authentication bug"));
        assert!(result.contains("Please fix the auth bug"));
    }

    #[test]
    fn test_build_prompt_with_status() {
        let task = Task::new("fix/auth-bug", "Fix authentication bug", "main").unwrap();
        let workspace = PathBuf::from("/tmp/workspace");
        let prompt = "Please fix the auth bug";
        let status = RepoStatus {
            commits_behind: 3,
            conflict_files: vec!["auth.rs".to_string()],
        };

        let result = build_prompt(&task, &workspace, false, prompt, Some(&status));

        assert!(result.contains("3 commits ahead"));
        assert!(result.contains("conflicts with main"));
        assert!(result.contains("auth.rs"));
    }
}
