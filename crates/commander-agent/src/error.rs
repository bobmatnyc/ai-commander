//! Error types for the agent crate.

use thiserror::Error;

/// Errors that can occur in agent operations.
#[derive(Error, Debug)]
pub enum AgentError {
    /// Tool execution failed.
    #[error("tool execution failed: {tool_name}: {message}")]
    ToolExecution {
        /// Name of the tool that failed.
        tool_name: String,
        /// Error message.
        message: String,
    },

    /// Tool not found.
    #[error("tool not found: {0}")]
    ToolNotFound(String),

    /// Invalid tool arguments.
    #[error("invalid tool arguments for {tool_name}: {message}")]
    InvalidArguments {
        /// Name of the tool.
        tool_name: String,
        /// Error message.
        message: String,
    },

    /// Context retrieval failed.
    #[error("failed to build context: {0}")]
    ContextBuild(String),

    /// Model invocation failed.
    #[error("model invocation failed: {0}")]
    ModelInvocation(String),

    /// Response parsing failed.
    #[error("failed to parse response: {0}")]
    ResponseParse(String),

    /// Memory operation failed.
    #[error("memory operation failed: {0}")]
    Memory(#[from] commander_memory::MemoryError),

    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Maximum iterations exceeded in tool loop.
    #[error("maximum iterations ({0}) exceeded in tool execution loop")]
    MaxIterationsExceeded(u32),

    /// Agent not initialized.
    #[error("agent not initialized: {0}")]
    NotInitialized(String),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Configuration(String),

    /// Internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Result type for agent operations.
pub type Result<T> = std::result::Result<T, AgentError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AgentError::ToolExecution {
            tool_name: "read_file".into(),
            message: "file not found".into(),
        };
        assert_eq!(
            err.to_string(),
            "tool execution failed: read_file: file not found"
        );

        let err = AgentError::ToolNotFound("unknown_tool".into());
        assert_eq!(err.to_string(), "tool not found: unknown_tool");

        let err = AgentError::MaxIterationsExceeded(10);
        assert_eq!(
            err.to_string(),
            "maximum iterations (10) exceeded in tool execution loop"
        );
    }

    #[test]
    fn test_error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let err: AgentError = json_err.into();
        assert!(matches!(err, AgentError::Serialization(_)));
    }
}
