//! Error types for persistence operations.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during persistence operations.
#[derive(Error, Debug)]
pub enum PersistenceError {
    /// Failed to read from file system.
    #[error("failed to read {path}: {source}")]
    ReadError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to write to file system.
    #[error("failed to write {path}: {source}")]
    WriteError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to serialize data to JSON.
    #[error("failed to serialize: {0}")]
    SerializeError(#[from] serde_json::Error),

    /// Failed to create directory.
    #[error("failed to create directory {path}: {source}")]
    DirectoryError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Item not found.
    #[error("{kind} not found: {id}")]
    NotFound { kind: String, id: String },

    /// Invalid data.
    #[error("invalid data: {0}")]
    InvalidData(String),
}

/// Result type alias for persistence operations.
pub type Result<T> = std::result::Result<T, PersistenceError>;
