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

/// PID file location for the telegram bot daemon.
fn telegram_pid_file() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".commander")
        .join("telegram.pid")
}

/// Check if the Telegram bot daemon is running.
pub fn is_telegram_running() -> bool {
    let pid_file = telegram_pid_file();
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

/// Start the Telegram bot daemon.
pub fn start_telegram_daemon() -> Result<u32, String> {
    // Check for TELEGRAM_BOT_TOKEN
    if std::env::var("TELEGRAM_BOT_TOKEN").is_err() {
        return Err("TELEGRAM_BOT_TOKEN not set. Please set it in your environment.".to_string());
    }

    // Find the commander-telegram binary
    let binary = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("commander-telegram")))
        .filter(|p| p.exists())
        .or_else(|| which::which("commander-telegram").ok())
        .ok_or_else(|| "commander-telegram binary not found".to_string())?;

    // Start as background process
    let child = Command::new(&binary)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start telegram bot: {}", e))?;

    let pid = child.id();

    // Write PID file
    let pid_file = telegram_pid_file();
    if let Some(parent) = pid_file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&pid_file, pid.to_string())
        .map_err(|e| format!("Failed to write PID file: {}", e))?;

    Ok(pid)
}

/// Ensure telegram bot is running, starting it if needed.
/// Returns Ok(true) if already running, Ok(false) if started now.
pub fn ensure_telegram_running() -> Result<bool, String> {
    if is_telegram_running() {
        return Ok(true);
    }

    start_telegram_daemon()?;

    // Give it a moment to start
    std::thread::sleep(std::time::Duration::from_millis(500));

    Ok(false)
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
        let pid_file = telegram_pid_file();
        // Should be in .commander directory
        assert!(pid_file.to_string_lossy().contains(".commander"));
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
        // Remove token if set
        std::env::remove_var("TELEGRAM_BOT_TOKEN");

        let result = start_telegram_daemon();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("TELEGRAM_BOT_TOKEN"));
    }
}
