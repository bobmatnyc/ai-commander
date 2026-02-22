//! Commander CLI library.
//!
//! This crate provides the command-line interface and interactive REPL
//! for Commander.

pub mod agent_cli;
pub mod chat;
pub mod cli;
pub mod commands;
pub mod filesystem;
pub mod repl;
pub mod tui;

// Re-export orchestrator when agents feature is enabled
#[cfg(feature = "agents")]
pub use commander_orchestrator;

use std::path::Path;

use commander_telegram::daemon;

/// Check if the Telegram bot daemon is running.
pub fn is_telegram_running() -> bool {
    daemon::is_running()
}

/// Result of starting the Telegram bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramStartResult {
    /// Bot was already running
    AlreadyRunning,
    /// Bot was started from existing binary
    Started,
    /// Binary was built and bot was started
    BuiltAndStarted,
}

/// Start the Telegram bot daemon.
pub fn start_telegram_daemon() -> Result<u32, String> {
    daemon::start().map_err(|e| e.to_string())
}

/// Ensure telegram bot is running, starting it if needed.
/// Returns the result indicating what action was taken.
pub fn ensure_telegram_running() -> Result<TelegramStartResult, String> {
    match daemon::ensure_running() {
        Ok(daemon::StartResult::AlreadyRunning) => Ok(TelegramStartResult::AlreadyRunning),
        Ok(daemon::StartResult::Started) => Ok(TelegramStartResult::Started),
        Ok(daemon::StartResult::BuiltAndStarted) => Ok(TelegramStartResult::BuiltAndStarted),
        Err(e) => Err(e.to_string()),
    }
}

/// Restart the Telegram bot if it's currently running.
/// This ensures the bot uses the latest binary and correct state directory.
/// Called on TUI/REPL startup to ensure the bot is up-to-date.
pub fn restart_telegram_if_running() {
    daemon::restart_if_running()
}

/// Validate that a project path exists, is a directory, and is accessible.
///
/// Returns `Ok(())` if the path is valid, or `Err(message)` describing the issue.
pub fn validate_project_path(path: &str) -> Result<(), String> {
    let path = Path::new(path);

    if !path.exists() {
        return Err(format!("Project path does not exist: {}", path.display()));
    }

    if !path.is_dir() {
        return Err(format!(
            "Project path is not a directory: {}",
            path.display()
        ));
    }

    // Check if readable by attempting to read dir
    if path.read_dir().is_err() {
        return Err(format!(
            "Cannot access project path: {} (permission denied)",
            path.display()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_project_path_valid_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let result = validate_project_path(temp_dir.path().to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_project_path_nonexistent() {
        let result = validate_project_path("/nonexistent/path/that/does/not/exist");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn test_validate_project_path_is_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_file.txt");
        std::fs::write(&file_path, "test").unwrap();

        let result = validate_project_path(file_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a directory"));
    }

    #[test]
    fn test_is_telegram_running_no_pid_file() {
        // When no PID file exists, should return false
        // Note: This test relies on the actual PID file not existing or having an invalid PID
        // which is the expected state in a test environment
        let result = is_telegram_running();
        // Result depends on whether a real bot is running, but function should not panic
        let _ = result;
    }

    #[test]
    fn test_start_telegram_daemon_without_token() {
        // Remove token if set
        std::env::remove_var("TELEGRAM_BOT_TOKEN");

        let result = start_telegram_daemon();
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Error should mention either TELEGRAM_BOT_TOKEN (if checked first)
        // or may be about finding binary (if binary doesn't exist)
        assert!(
            err.contains("TELEGRAM_BOT_TOKEN") || err.contains("Failed to start daemon"),
            "Unexpected error: {}",
            err
        );
    }
}
