//! Error types for the runtime crate.

use thiserror::Error;

/// Errors that can occur in the runtime.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Tmux error.
    #[error("tmux error: {0}")]
    Tmux(#[from] commander_tmux::TmuxError),

    /// Instance not found.
    #[error("instance not found: {0}")]
    InstanceNotFound(String),

    /// Instance already exists.
    #[error("instance already exists: {0}")]
    InstanceExists(String),

    /// Maximum instances reached.
    #[error("maximum instances reached: {0}")]
    MaxInstancesReached(usize),

    /// Runtime not started.
    #[error("runtime not started")]
    NotStarted,

    /// Runtime already started.
    #[error("runtime already started")]
    AlreadyStarted,

    /// Shutdown error.
    #[error("shutdown error: {0}")]
    Shutdown(String),

    /// Channel error.
    #[error("channel error: {0}")]
    Channel(String),
}

/// Result type for runtime operations.
pub type Result<T> = std::result::Result<T, RuntimeError>;
