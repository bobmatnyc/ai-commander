//! Shared configuration for Commander.
//!
//! Provides functions to locate Commander's state directory and common
//! configuration files across all Commander interfaces.
//!
//! # Storage Structure
//!
//! All application data is stored under `~/.ai-commander/`:
//!
//! ```text
//! ~/.ai-commander/
//! ├── db/           # Databases (ChromaDB, etc.)
//! │   └── chroma/
//! ├── logs/         # Application logs
//! ├── config/       # User configuration files
//! ├── cache/        # Temporary cache files
//! └── state/        # Runtime state files
//! ```
//!
//! # Environment Variables
//!
//! - `COMMANDER_STATE_DIR`: Override the base state directory
//! - `COMMANDER_DB_DIR`: Override the database directory
//! - `COMMANDER_LOG_DIR`: Override the log directory
//! - `COMMANDER_CONFIG_DIR`: Override the config directory
//! - `COMMANDER_CACHE_DIR`: Override the cache directory

use std::path::PathBuf;
use std::sync::OnceLock;

/// Environment variable for custom state directory.
pub const STATE_DIR_ENV: &str = "COMMANDER_STATE_DIR";

/// Environment variable for custom database directory.
pub const DB_DIR_ENV: &str = "COMMANDER_DB_DIR";

/// Environment variable for custom log directory.
pub const LOG_DIR_ENV: &str = "COMMANDER_LOG_DIR";

/// Environment variable for custom config directory.
pub const CONFIG_DIR_ENV: &str = "COMMANDER_CONFIG_DIR";

/// Environment variable for custom cache directory.
pub const CACHE_DIR_ENV: &str = "COMMANDER_CACHE_DIR";

/// Default state directory name under home.
const DEFAULT_STATE_DIR: &str = ".ai-commander";

/// Legacy state directory name (for migration).
const LEGACY_STATE_DIR: &str = ".commander";

// Subdirectory names
const DB_SUBDIR: &str = "db";
const LOGS_SUBDIR: &str = "logs";
const CONFIG_SUBDIR: &str = "config";
const CACHE_SUBDIR: &str = "cache";
const STATE_SUBDIR: &str = "state";

// Static caches for lazy initialization
static STATE_DIR_CACHE: OnceLock<PathBuf> = OnceLock::new();

/// Get the Commander state directory.
///
/// The state directory is determined by:
/// 1. `COMMANDER_STATE_DIR` environment variable if set
/// 2. `~/.ai-commander` if home directory is available
/// 3. `.ai-commander` in current directory as fallback
pub fn state_dir() -> PathBuf {
    STATE_DIR_CACHE
        .get_or_init(|| {
            std::env::var(STATE_DIR_ENV)
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    dirs::home_dir()
                        .map(|h| h.join(DEFAULT_STATE_DIR))
                        .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR))
                })
        })
        .clone()
}

/// Get the legacy state directory path (for migration).
pub fn legacy_state_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(LEGACY_STATE_DIR))
}

/// Get the database directory.
///
/// Defaults to `~/.ai-commander/db/` or `COMMANDER_DB_DIR` env var.
pub fn db_dir() -> PathBuf {
    std::env::var(DB_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| state_dir().join(DB_SUBDIR))
}

/// Get the ChromaDB directory.
pub fn chroma_dir() -> PathBuf {
    db_dir().join("chroma")
}

/// Get the logs directory.
///
/// Defaults to `~/.ai-commander/logs/` or `COMMANDER_LOG_DIR` env var.
pub fn logs_dir() -> PathBuf {
    std::env::var(LOG_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| state_dir().join(LOGS_SUBDIR))
}

/// Get the user config directory.
///
/// Defaults to `~/.ai-commander/config/` or `COMMANDER_CONFIG_DIR` env var.
pub fn config_dir() -> PathBuf {
    std::env::var(CONFIG_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| state_dir().join(CONFIG_SUBDIR))
}

/// Get the cache directory.
///
/// Defaults to `~/.ai-commander/cache/` or `COMMANDER_CACHE_DIR` env var.
pub fn cache_dir() -> PathBuf {
    std::env::var(CACHE_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| state_dir().join(CACHE_SUBDIR))
}

/// Get the runtime state directory.
///
/// Used for session state, PID files, etc.
pub fn runtime_state_dir() -> PathBuf {
    state_dir().join(STATE_SUBDIR)
}

/// Get the pairing file path.
///
/// The pairing file stores chat ID to project mappings for Telegram.
pub fn pairing_file() -> PathBuf {
    runtime_state_dir().join("pairings.json")
}

/// Get the Telegram bot PID file path.
///
/// Used to track running Telegram bot instances.
pub fn telegram_pid_file() -> PathBuf {
    runtime_state_dir().join("telegram.pid")
}

/// Get the projects database file path.
///
/// Stores project definitions and metadata.
pub fn projects_file() -> PathBuf {
    runtime_state_dir().join("projects.json")
}

/// Get the sessions directory path.
///
/// Stores session state for active tmux sessions.
pub fn sessions_dir() -> PathBuf {
    runtime_state_dir().join("sessions")
}

/// Get the notifications file path.
///
/// Stores cross-channel notification queue.
pub fn notifications_file() -> PathBuf {
    runtime_state_dir().join("notifications.json")
}

/// Get the main config file path.
///
/// The config.toml file for user settings.
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// Get the .env.local file path.
///
/// Environment file for secrets (API keys, tokens).
pub fn env_file() -> PathBuf {
    config_dir().join(".env.local")
}

/// Ensure the state directory and all subdirectories exist.
///
/// Creates the full directory structure:
/// - ~/.ai-commander/
/// - ~/.ai-commander/db/
/// - ~/.ai-commander/logs/
/// - ~/.ai-commander/config/
/// - ~/.ai-commander/cache/
/// - ~/.ai-commander/state/
///
/// # Errors
/// Returns an error if any directory cannot be created.
pub fn ensure_all_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(db_dir())?;
    std::fs::create_dir_all(logs_dir())?;
    std::fs::create_dir_all(config_dir())?;
    std::fs::create_dir_all(cache_dir())?;
    std::fs::create_dir_all(runtime_state_dir())?;
    std::fs::create_dir_all(sessions_dir())?;
    Ok(())
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

/// Ensure the runtime state directory exists.
///
/// # Errors
/// Returns an error if the directory cannot be created.
pub fn ensure_runtime_state_dir() -> std::io::Result<()> {
    let dir = runtime_state_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(())
}

/// Ensure the config directory exists.
///
/// # Errors
/// Returns an error if the directory cannot be created.
pub fn ensure_config_dir() -> std::io::Result<()> {
    let dir = config_dir();
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
        assert!(dir.is_absolute() || dir.ends_with(".ai-commander"));
    }

    #[test]
    fn test_db_dir_name() {
        let dir = db_dir();
        assert!(dir.ends_with("db") || dir.to_string_lossy().contains("db"));
    }

    #[test]
    fn test_chroma_dir_name() {
        let dir = chroma_dir();
        assert!(dir.ends_with("chroma"));
    }

    #[test]
    fn test_logs_dir_name() {
        let dir = logs_dir();
        assert!(dir.ends_with("logs") || dir.to_string_lossy().contains("logs"));
    }

    #[test]
    fn test_config_dir_name() {
        let dir = config_dir();
        assert!(dir.ends_with("config") || dir.to_string_lossy().contains("config"));
    }

    #[test]
    fn test_cache_dir_name() {
        let dir = cache_dir();
        assert!(dir.ends_with("cache") || dir.to_string_lossy().contains("cache"));
    }

    #[test]
    fn test_runtime_state_dir_name() {
        let dir = runtime_state_dir();
        assert!(dir.ends_with("state"));
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

    #[test]
    fn test_notifications_file_name() {
        let file = notifications_file();
        assert!(file.ends_with("notifications.json"));
    }

    #[test]
    fn test_config_file_name() {
        let file = config_file();
        assert!(file.ends_with("config.toml"));
    }

    #[test]
    fn test_env_file_name() {
        let file = env_file();
        assert!(file.ends_with(".env.local"));
    }

    #[test]
    fn test_legacy_state_dir() {
        if let Some(dir) = legacy_state_dir() {
            assert!(dir.ends_with(".commander"));
        }
    }
}
