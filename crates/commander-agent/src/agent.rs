//! Core Agent trait definition.
//!
//! This module defines the `Agent` trait that both User Agent and Session Agents
//! implement, providing a common interface for message processing, tool calling,
//! and memory access.

use async_trait::async_trait;
use commander_memory::MemoryStore;
use serde::{Deserialize, Serialize};

use crate::config::ModelConfig;
use crate::context::AgentContext;
use crate::error::Result;
use crate::response::AgentResponse;
use crate::tool::{ToolCall, ToolDefinition, ToolResult};

/// Type of agent in the multi-agent system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentType {
    /// User-facing agent that coordinates tasks.
    User,

    /// Session agent that manages a specific coding session.
    Session {
        /// Unique identifier of the session.
        session_id: String,
        /// Type of adapter (e.g., "tmux", "vscode").
        adapter_type: String,
    },
}

impl AgentType {
    /// Create a User agent type.
    pub fn user() -> Self {
        Self::User
    }

    /// Create a Session agent type.
    pub fn session(session_id: impl Into<String>, adapter_type: impl Into<String>) -> Self {
        Self::Session {
            session_id: session_id.into(),
            adapter_type: adapter_type.into(),
        }
    }

    /// Check if this is a user agent.
    pub fn is_user(&self) -> bool {
        matches!(self, Self::User)
    }

    /// Check if this is a session agent.
    pub fn is_session(&self) -> bool {
        matches!(self, Self::Session { .. })
    }

    /// Get the session ID if this is a session agent.
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Session { session_id, .. } => Some(session_id),
            Self::User => None,
        }
    }

    /// Get the adapter type if this is a session agent.
    pub fn adapter_type(&self) -> Option<&str> {
        match self {
            Self::Session { adapter_type, .. } => Some(adapter_type),
            Self::User => None,
        }
    }
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Session {
                session_id,
                adapter_type,
            } => write!(f, "session[{}:{}]", adapter_type, session_id),
        }
    }
}

/// Core trait for agents in the multi-agent system.
///
/// Agents are autonomous entities that can process messages, execute tools,
/// and maintain memory. Both the User Agent and Session Agents implement
/// this trait.
///
/// # Object Safety
///
/// This trait is object-safe for use with dynamic dispatch (`dyn Agent`),
/// allowing different agent implementations to be used interchangeably.
///
/// # Example
///
/// ```ignore
/// use commander_agent::{Agent, AgentContext, AgentResponse};
///
/// struct MyAgent { /* ... */ }
///
/// #[async_trait]
/// impl Agent for MyAgent {
///     fn id(&self) -> &str { "my-agent" }
///     fn agent_type(&self) -> AgentType { AgentType::User }
///     // ... implement other methods
/// }
/// ```
#[async_trait]
pub trait Agent: Send + Sync {
    /// Get the unique identifier for this agent.
    fn id(&self) -> &str;

    /// Get the type of this agent (user or session).
    fn agent_type(&self) -> AgentType;

    /// Process a message and return a response.
    ///
    /// This is the main entry point for agent interaction. The agent will:
    /// 1. Consider the message and context
    /// 2. Optionally make tool calls
    /// 3. Return a response
    ///
    /// # Arguments
    /// * `message` - The input message to process
    /// * `context` - Context including conversation history and relevant memories
    ///
    /// # Returns
    /// An `AgentResponse` containing the agent's output and any tool calls.
    async fn process(&mut self, message: &str, context: &AgentContext) -> Result<AgentResponse>;

    /// Get the tools available to this agent.
    ///
    /// Returns a list of tool definitions that the agent can use.
    /// These are passed to the LLM for function calling.
    fn tools(&self) -> &[ToolDefinition];

    /// Execute a tool call and return the result.
    ///
    /// This method is called when the LLM decides to use a tool.
    /// Implementations should dispatch to the appropriate tool handler.
    ///
    /// # Arguments
    /// * `call` - The tool call to execute
    ///
    /// # Returns
    /// A `ToolResult` containing the tool's output or error.
    async fn execute_tool(&self, call: &ToolCall) -> Result<ToolResult>;

    /// Get access to the agent's memory store.
    ///
    /// Returns a reference to the memory store for storing and retrieving
    /// semantic memories.
    fn memory(&self) -> &dyn MemoryStore;

    /// Get the model configuration for this agent.
    ///
    /// Returns the LLM configuration including model name, parameters,
    /// and provider settings.
    fn model_config(&self) -> &ModelConfig;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_agent_type_user() {
        let agent_type = AgentType::user();

        assert!(agent_type.is_user());
        assert!(!agent_type.is_session());
        assert_eq!(agent_type.session_id(), None);
        assert_eq!(agent_type.adapter_type(), None);
        assert_eq!(agent_type.to_string(), "user");
    }

    #[test]
    fn test_agent_type_session() {
        let agent_type = AgentType::session("session-123", "tmux");

        assert!(!agent_type.is_user());
        assert!(agent_type.is_session());
        assert_eq!(agent_type.session_id(), Some("session-123"));
        assert_eq!(agent_type.adapter_type(), Some("tmux"));
        assert_eq!(agent_type.to_string(), "session[tmux:session-123]");
    }

    #[test]
    fn test_agent_type_serialization() {
        let user_type = AgentType::user();
        let json = serde_json::to_string(&user_type).unwrap();
        let parsed: AgentType = serde_json::from_str(&json).unwrap();
        assert_eq!(user_type, parsed);

        let session_type = AgentType::session("sess-1", "vscode");
        let json = serde_json::to_string(&session_type).unwrap();
        let parsed: AgentType = serde_json::from_str(&json).unwrap();
        assert_eq!(session_type, parsed);

        // Check JSON structure
        let json_value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(json_value["type"], json!("session"));
        assert_eq!(json_value["session_id"], json!("sess-1"));
        assert_eq!(json_value["adapter_type"], json!("vscode"));
    }
}
