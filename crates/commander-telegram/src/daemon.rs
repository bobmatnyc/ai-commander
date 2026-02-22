//! Daemon lifecycle management for commander-telegram.
//!
//! Provides cross-platform process management for starting, stopping, and checking
//! the status of the Telegram bot daemon.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use thiserror::Error;

use commander_core::config;

/// Daemon lifecycle errors.
#[derive(Error, Debug)]
pub enum DaemonError {
    #[error("Failed to start daemon: {0}")]
    StartFailed(String),
    #[error("Failed to stop daemon: {0}")]
    StopFailed(String),
    #[error("Daemon not running")]
    NotRunning,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result of starting the Telegram bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartResult {
    /// Bot was already running
    AlreadyRunning,
    /// Bot was started from existing binary
    Started,
    /// Binary was built and bot was started
    BuiltAndStarted,
}

/// Status of the daemon process.
pub struct DaemonStatus {
    pub running: bool,
    pub pid: Option<u32>,
}

/// Check if the Telegram bot daemon is running (cross-platform).
pub fn is_running() -> bool {
    let pid_file = config::telegram_pid_file();
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            return is_process_running(pid);
        }
    }
    false
}

/// Check if a process with the given PID is running (cross-platform).
fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill -0 checks if process exists without sending signal
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        // Use tasklist to check if process exists
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid)])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|output| output.contains(&pid.to_string()))
            .unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Fallback: assume not running on unsupported platforms
        false
    }
}

/// Start the Telegram bot daemon.
pub fn start() -> Result<u32, DaemonError> {
    // Load .env.local from config directory
    let env_path = config::env_file();
    if env_path.exists() {
        let _ = dotenvy::from_path(&env_path);
    }
    // Also try local .env.local for backwards compatibility
    let _ = dotenvy::from_filename(".env.local");

    // Check for TELEGRAM_BOT_TOKEN
    if std::env::var("TELEGRAM_BOT_TOKEN").is_err() {
        return Err(DaemonError::StartFailed(format!(
            "TELEGRAM_BOT_TOKEN not set. Add it to {} or set in environment.",
            env_path.display()
        )));
    }

    // Find the commander-telegram binary
    let binary = find_telegram_binary();

    let binary = match binary {
        Some(b) => b,
        None => {
            // Try to build it
            build_telegram_binary()?;
            find_telegram_binary().ok_or_else(|| {
                DaemonError::StartFailed(
                    "Failed to find commander-telegram after building".to_string(),
                )
            })?
        }
    };

    // Start as background process
    let child = Command::new(&binary)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| DaemonError::StartFailed(format!("Failed to spawn process: {}", e)))?;

    let pid = child.id();

    // Write PID file - ensure state directory exists
    let _ = config::ensure_runtime_state_dir();
    let pid_file = config::telegram_pid_file();
    fs::write(&pid_file, pid.to_string())
        .map_err(|e| DaemonError::StartFailed(format!("Failed to write PID file: {}", e)))?;

    Ok(pid)
}

/// Stop the daemon gracefully (SIGTERM with 5s timeout, then SIGKILL fallback).
pub fn stop() -> Result<(), DaemonError> {
    let pid_file = config::telegram_pid_file();

    let pid_str = fs::read_to_string(&pid_file).map_err(|_| DaemonError::NotRunning)?;

    let pid = pid_str
        .trim()
        .parse::<u32>()
        .map_err(|_| DaemonError::NotRunning)?;

    if !is_process_running(pid) {
        // Clean up stale PID file
        let _ = fs::remove_file(&pid_file);
        return Err(DaemonError::NotRunning);
    }

    // Try graceful shutdown first (SIGTERM)
    graceful_kill(pid)?;

    // Wait up to 5 seconds for process to exit
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !is_process_running(pid) {
            let _ = fs::remove_file(&pid_file);
            return Ok(());
        }
    }

    // Process didn't exit gracefully, force kill (SIGKILL)
    tracing::warn!(pid = pid, "Process did not exit gracefully, forcing kill");
    force_kill(pid)?;

    // Wait a moment for force kill to complete
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Clean up PID file
    let _ = fs::remove_file(&pid_file);

    Ok(())
}

/// Send graceful termination signal (SIGTERM on Unix, taskkill on Windows).
fn graceful_kill(pid: u32) -> Result<(), DaemonError> {
    #[cfg(unix)]
    {
        Command::new("kill")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| DaemonError::StopFailed(format!("Failed to send SIGTERM: {}", e)))?;
        Ok(())
    }

    #[cfg(windows)]
    {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| DaemonError::StopFailed(format!("Failed to send taskkill: {}", e)))?;
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err(DaemonError::StopFailed(
            "Graceful kill not supported on this platform".to_string(),
        ))
    }
}

/// Force kill process (SIGKILL on Unix, taskkill /F on Windows).
fn force_kill(pid: u32) -> Result<(), DaemonError> {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| DaemonError::StopFailed(format!("Failed to send SIGKILL: {}", e)))?;
        Ok(())
    }

    #[cfg(windows)]
    {
        Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| {
                DaemonError::StopFailed(format!("Failed to send taskkill /F: {}", e))
            })?;
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err(DaemonError::StopFailed(
            "Force kill not supported on this platform".to_string(),
        ))
    }
}

/// Restart the daemon.
pub fn restart() -> Result<u32, DaemonError> {
    if is_running() {
        tracing::info!("Restarting Telegram bot with updated code...");
        stop()?;
    }

    // Give it a moment to fully stop
    std::thread::sleep(std::time::Duration::from_millis(200));

    start()
}

/// Get daemon status.
pub fn status() -> DaemonStatus {
    let pid_file = config::telegram_pid_file();

    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            let running = is_process_running(pid);
            return DaemonStatus {
                running,
                pid: if running { Some(pid) } else { None },
            };
        }
    }

    DaemonStatus {
        running: false,
        pid: None,
    }
}

/// Ensure telegram bot is running, starting it if needed.
/// Returns the result indicating what action was taken.
pub fn ensure_running() -> Result<StartResult, DaemonError> {
    if is_running() {
        return Ok(StartResult::AlreadyRunning);
    }

    let needed_build = find_telegram_binary().is_none();
    start()?;

    // Give it a moment to start
    std::thread::sleep(std::time::Duration::from_millis(500));

    if needed_build {
        Ok(StartResult::BuiltAndStarted)
    } else {
        Ok(StartResult::Started)
    }
}

/// Restart the Telegram bot if it's currently running.
/// This ensures the bot uses the latest binary and correct state directory.
/// Called on TUI/REPL startup to ensure the bot is up-to-date.
pub fn restart_if_running() {
    if !is_running() {
        return;
    }

    match restart() {
        Ok(new_pid) => {
            tracing::info!(pid = new_pid, "Telegram bot restarted");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to restart Telegram bot");
        }
    }
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
fn build_telegram_binary() -> Result<(), DaemonError> {
    eprintln!("Building commander-telegram...");

    let status = Command::new("cargo")
        .args(["build", "-p", "commander-telegram", "--release"])
        .status()
        .map_err(|e| DaemonError::StartFailed(format!("Failed to run cargo build: {}", e)))?;

    if !status.success() {
        return Err(DaemonError::StartFailed("cargo build failed".to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telegram_pid_file_location() {
        let pid_file = config::telegram_pid_file();
        // Should be in .ai-commander directory
        assert!(pid_file.to_string_lossy().contains(".ai-commander"));
        assert!(pid_file.file_name().unwrap().to_string_lossy() == "telegram.pid");
    }

    #[test]
    fn test_is_running_no_pid_file() {
        // When no PID file exists, should return false
        let result = is_running();
        // Function should not panic
        let _ = result;
    }

    #[test]
    fn test_status_no_pid_file() {
        let status = status();
        // Function should not panic
        assert!(!status.running || status.pid.is_some());
    }
}
