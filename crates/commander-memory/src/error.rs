//! Error types for memory operations.

use thiserror::Error;

/// Errors that can occur during memory operations.
#[derive(Error, Debug)]
pub enum MemoryError {
    /// Failed to connect to or initialize the vector database.
    #[error("database error: {0}")]
    DatabaseError(String),

    /// Failed to generate embeddings.
    #[error("embedding error: {0}")]
    EmbeddingError(String),

    /// Failed to serialize/deserialize data.
    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Memory not found.
    #[error("memory not found: {0}")]
    NotFound(String),

    /// Invalid configuration.
    #[error("configuration error: {0}")]
    ConfigError(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Result type alias for memory operations.
pub type Result<T> = std::result::Result<T, MemoryError>;
