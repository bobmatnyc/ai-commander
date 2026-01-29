//! Error types for work queue operations.

use commander_persistence::PersistenceError;
use thiserror::Error;

/// Errors that can occur during work queue operations.
#[derive(Error, Debug)]
pub enum WorkError {
    /// Work item not found.
    #[error("work item not found: {0}")]
    NotFound(String),

    /// Work item is in invalid state for operation.
    #[error("invalid state: {0}")]
    InvalidState(String),

    /// Dependency cycle detected.
    #[error("dependency cycle detected: {0}")]
    DependencyCycle(String),

    /// Persistence error.
    #[error("persistence error: {0}")]
    Persistence(#[from] PersistenceError),

    /// Lock poisoned (thread panicked while holding lock).
    #[error("lock poisoned: {0}")]
    LockPoisoned(String),
}

/// Result type alias for work queue operations.
pub type Result<T> = std::result::Result<T, WorkError>;
