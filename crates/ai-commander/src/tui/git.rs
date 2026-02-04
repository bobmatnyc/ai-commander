//! Git operations for TUI.

use super::App;

impl App {
    /// Check if path is inside a git worktree.
    pub fn is_git_worktree(path: &str) -> bool {
        std::process::Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false)
    }

    /// Commit any uncommitted git changes in the project directory.
    /// Returns Ok(None) if not a git repository, Ok(Some(true)) if committed,
    /// Ok(Some(false)) if no changes, or Err on failure.
    pub fn git_commit_changes(&self, path: &str, project_name: &str) -> Result<Option<bool>, String> {
        use std::process::Command;

        // Skip git operations if not in a git worktree
        if !Self::is_git_worktree(path) {
            return Ok(None);
        }

        // Check if there are changes
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to run git status: {}", e))?;

        let changes = String::from_utf8_lossy(&status.stdout);
        if changes.trim().is_empty() {
            return Ok(Some(false)); // No changes
        }

        // Stage all changes
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to stage changes: {}", e))?;

        // Commit with message (may fail if pre-commit hooks modify files)
        let message = format!("WIP: Auto-commit from Commander session '{}'", project_name);
        let commit = Command::new("git")
            .args(["commit", "-m", &message])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to commit: {}", e))?;

        if commit.status.success() {
            return Ok(Some(true));
        }

        // Pre-commit hooks may have modified files - re-stage and retry
        let stdout = String::from_utf8_lossy(&commit.stdout);
        if stdout.contains("Passed") || stdout.contains("Fixed") || stdout.contains("trailing whitespace") {
            // Hooks ran and fixed things - re-stage and commit again
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(path)
                .output()
                .map_err(|e| format!("Failed to re-stage changes: {}", e))?;

            let retry = Command::new("git")
                .args(["commit", "-m", &message])
                .current_dir(path)
                .output()
                .map_err(|e| format!("Failed to commit after hooks: {}", e))?;

            if retry.status.success() {
                return Ok(Some(true));
            }

            // Check if nothing to commit after hooks
            let status2 = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(path)
                .output()
                .ok();

            if let Some(s) = status2 {
                if String::from_utf8_lossy(&s.stdout).trim().is_empty() {
                    return Ok(Some(false)); // Hooks fixed everything, nothing to commit
                }
            }

            let stderr = String::from_utf8_lossy(&retry.stderr);
            Err(format!("Commit failed after hooks: {}", stderr))
        } else {
            let stderr = String::from_utf8_lossy(&commit.stderr);
            Err(format!("Commit failed: {}", stderr))
        }
    }
}
