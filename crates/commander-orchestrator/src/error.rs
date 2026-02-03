//! Error types for the orchestrator.

use thiserror::Error;

/// Orchestrator-specific errors.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    /// Agent error.
    #[error("Agent error: {0}")]
    Agent(#[from] commander_agent::AgentError),

    /// Memory error.
    #[error("Memory error: {0}")]
    Memory(#[from] commander_memory::MemoryError),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Session not found.
    #[error("Session not found: {0}")]
    SessionNotFound(String),
}

/// Result type for orchestrator operations.
pub type Result<T> = std::result::Result<T, OrchestratorError>;
