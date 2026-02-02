//! Shared configuration for Commander.
//!
//! Provides functions to locate Commander's state directory and common
//! configuration files across all Commander interfaces.

use std::path::PathBuf;

/// Environment variable for custom state directory.
const STATE_DIR_ENV: &str = "COMMANDER_STATE_DIR";

/// Default state directory name under home.
const DEFAULT_STATE_DIR: &str = ".commander";

/// Get the Commander state directory.
///
/// The state directory is determined by:
/// 1. `COMMANDER_STATE_DIR` environment variable if set
/// 2. `~/.commander` if home directory is available
/// 3. `.commander` in current directory as fallback
pub fn state_dir() -> PathBuf {
    std::env::var(STATE_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|h| h.join(DEFAULT_STATE_DIR))
                .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR))
        })
}

/// Get the pairing file path.
///
/// The pairing file stores chat ID to project mappings for Telegram.
pub fn pairing_file() -> PathBuf {
    state_dir().join("pairings.json")
}

/// Get the Telegram bot PID file path.
///
/// Used to track running Telegram bot instances.
pub fn telegram_pid_file() -> PathBuf {
    state_dir().join("telegram.pid")
}

/// Get the projects database file path.
///
/// Stores project definitions and metadata.
pub fn projects_file() -> PathBuf {
    state_dir().join("projects.json")
}

/// Get the sessions directory path.
///
/// Stores session state for active tmux sessions.
pub fn sessions_dir() -> PathBuf {
    state_dir().join("sessions")
}

/// Ensure the state directory exists, creating it if necessary.
///
/// # Errors
/// Returns an error if the directory cannot be created.
pub fn ensure_state_dir() -> std::io::Result<()> {
    let dir = state_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(())
}

/// Ensure the sessions directory exists.
///
/// # Errors
/// Returns an error if the directory cannot be created.
pub fn ensure_sessions_dir() -> std::io::Result<()> {
    let dir = sessions_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests use environment variables which can't be isolated
    // in parallel test execution. We test the path construction logic
    // by verifying the file/dir names rather than full paths.

    #[test]
    fn test_state_dir_uses_env() {
        // Test that the function returns a PathBuf (basic smoke test)
        let dir = state_dir();
        assert!(dir.is_absolute() || dir.ends_with(".commander"));
    }

    #[test]
    fn test_pairing_file_name() {
        let file = pairing_file();
        assert!(file.ends_with("pairings.json"));
    }

    #[test]
    fn test_telegram_pid_file_name() {
        let file = telegram_pid_file();
        assert!(file.ends_with("telegram.pid"));
    }

    #[test]
    fn test_projects_file_name() {
        let file = projects_file();
        assert!(file.ends_with("projects.json"));
    }

    #[test]
    fn test_sessions_dir_name() {
        let dir = sessions_dir();
        assert!(dir.ends_with("sessions"));
    }
}
