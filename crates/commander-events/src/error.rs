//! Error types for event operations.

use commander_persistence::PersistenceError;
use thiserror::Error;

/// Errors that can occur during event operations.
#[derive(Error, Debug)]
pub enum EventError {
    /// Event not found.
    #[error("event not found: {0}")]
    NotFound(String),

    /// Event is in invalid state for operation.
    #[error("invalid state: {0}")]
    InvalidState(String),

    /// Persistence error.
    #[error("persistence error: {0}")]
    Persistence(#[from] PersistenceError),

    /// Lock poisoned (thread panicked while holding lock).
    #[error("lock poisoned: {0}")]
    LockPoisoned(String),
}

/// Result type alias for event operations.
pub type Result<T> = std::result::Result<T, EventError>;
