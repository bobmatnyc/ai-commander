//! Error types for tmux operations.

use thiserror::Error;

/// Errors that can occur during tmux operations.
#[derive(Error, Debug)]
pub enum TmuxError {
    /// tmux not found in PATH.
    #[error("tmux not found in PATH")]
    NotFound,

    /// Session not found.
    #[error("session '{0}' not found")]
    SessionNotFound(String),

    /// Pane not found in session.
    #[error("pane '{0}' not found in session '{1}'")]
    PaneNotFound(String, String),

    /// tmux command failed.
    #[error("tmux command failed: {0}")]
    CommandFailed(String),

    /// I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse tmux output.
    #[error("parse error: {0}")]
    ParseError(String),
}

/// Result type alias for tmux operations.
pub type Result<T> = std::result::Result<T, TmuxError>;
