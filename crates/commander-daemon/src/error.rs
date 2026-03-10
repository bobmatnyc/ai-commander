//! Error types for the commander daemon.

use thiserror::Error;

/// Result type for daemon operations.
pub type Result<T> = std::result::Result<T, DaemonError>;

/// Errors that can occur in daemon operations.
#[derive(Error, Debug)]
pub enum DaemonError {
    #[error("Failed to start daemon: {0}")]
    StartFailed(String),

    #[error("Failed to stop daemon: {0}")]
    StopFailed(String),

    #[error("Daemon not running")]
    NotRunning,

    #[error("Daemon already running with PID {0}")]
    AlreadyRunning(u32),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Session already exists: {0}")]
    SessionExists(String),

    #[error("IPC error: {0}")]
    Ipc(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Memory monitoring error: {0}")]
    Memory(String),

    #[error("Pairing error: {0}")]
    Pairing(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Orchestrator error: {0}")]
    Orchestrator(#[from] commander_orchestrator::OrchestratorError),
}
