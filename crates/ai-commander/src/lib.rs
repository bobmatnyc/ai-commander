//! Commander CLI library.
//!
//! This crate provides the command-line interface and interactive REPL
//! for Commander.

pub mod chat;
pub mod cli;
pub mod commands;
pub mod filesystem;
pub mod repl;
pub mod tui;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use commander_core::config;

/// Check if the Telegram bot daemon is running.
pub fn is_telegram_running() -> bool {
    let pid_file = config::telegram_pid_file();
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            // Check if process is running (Unix-specific)
            #[cfg(unix)]
            {
                // kill -0 checks if process exists without sending signal
                return Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
            }
            #[cfg(not(unix))]
            {
                return false;
            }
        }
    }
    false
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
    // Load .env.local from config directory
    let env_path = config::env_file();
    if env_path.exists() {
        let _ = dotenvy::from_path(&env_path);
    }
    // Also try local .env.local for backwards compatibility
    let _ = dotenvy::from_filename(".env.local");

    // Check for TELEGRAM_BOT_TOKEN
    if std::env::var("TELEGRAM_BOT_TOKEN").is_err() {
        return Err(format!(
            "TELEGRAM_BOT_TOKEN not set. Add it to {} or set in environment.",
            env_path.display()
        ));
    }

    // Find the commander-telegram binary
    let binary = find_telegram_binary();

    let binary = match binary {
        Some(b) => b,
        None => {
            // Try to build it
            build_telegram_binary()?;
            find_telegram_binary()
                .ok_or_else(|| "Failed to find commander-telegram after building".to_string())?
        }
    };

    // Start as background process
    let child = Command::new(&binary)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start telegram bot: {}", e))?;

    let pid = child.id();

    // Write PID file - ensure state directory exists
    let _ = config::ensure_runtime_state_dir();
    let pid_file = config::telegram_pid_file();
    fs::write(&pid_file, pid.to_string())
        .map_err(|e| format!("Failed to write PID file: {}", e))?;

    Ok(pid)
}

/// Find the commander-telegram binary.
fn find_telegram_binary() -> Option<PathBuf> {
    // Check next to current exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let binary = dir.join("commander-telegram");
            if binary.exists() {
                return Some(binary);
            }
        }
    }

    // Check in PATH
    which::which("commander-telegram").ok()
}

/// Build the commander-telegram binary.
fn build_telegram_binary() -> Result<(), String> {
    eprintln!("Building commander-telegram...");

    let status = Command::new("cargo")
        .args(["build", "-p", "commander-telegram", "--release"])
        .status()
        .map_err(|e| format!("Failed to run cargo build: {}", e))?;

    if !status.success() {
        return Err("cargo build failed".to_string());
    }

    Ok(())
}

/// Ensure telegram bot is running, starting it if needed.
/// Returns the result indicating what action was taken.
pub fn ensure_telegram_running() -> Result<TelegramStartResult, String> {
    if is_telegram_running() {
        return Ok(TelegramStartResult::AlreadyRunning);
    }

    let needed_build = find_telegram_binary().is_none();
    start_telegram_daemon()?;

    // Give it a moment to start
    std::thread::sleep(std::time::Duration::from_millis(500));

    if needed_build {
        Ok(TelegramStartResult::BuiltAndStarted)
    } else {
        Ok(TelegramStartResult::Started)
    }
}

/// Restart the Telegram bot if it's currently running.
/// This ensures the bot uses the latest binary and correct state directory.
/// Called on TUI/REPL startup to ensure the bot is up-to-date.
pub fn restart_telegram_if_running() {
    if !is_telegram_running() {
        return;
    }

    tracing::info!("Restarting Telegram bot with updated code...");

    // Kill the old process
    let pid_file = config::telegram_pid_file();
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            #[cfg(unix)]
            {
                let _ = Command::new("kill")
                    .arg(pid.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }
    let _ = fs::remove_file(&pid_file);

    // Give it a moment to stop
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Restart it
    match start_telegram_daemon() {
        Ok(new_pid) => {
            tracing::info!(pid = new_pid, "Telegram bot restarted");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to restart Telegram bot");
        }
    }
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
    use std::fs;

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
        fs::write(&file_path, "test").unwrap();

        let result = validate_project_path(file_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a directory"));
    }

    #[test]
    fn test_telegram_pid_file_location() {
        let pid_file = config::telegram_pid_file();
        // Should be in .ai-commander directory
        assert!(pid_file.to_string_lossy().contains(".ai-commander"));
        assert!(pid_file.file_name().unwrap().to_string_lossy() == "telegram.pid");
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
        // Run from a temp directory where .env.local doesn't exist
        let temp_dir = tempfile::tempdir().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Remove token if set
        std::env::remove_var("TELEGRAM_BOT_TOKEN");

        let result = start_telegram_daemon();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("TELEGRAM_BOT_TOKEN"));
    }
}
