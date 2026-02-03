//! Agent trait and types for the AI Commander multi-agent system.
//!
//! This crate defines the core `Agent` trait and supporting types that both
//! the User Agent and Session Agents implement. It provides a unified interface
//! for message processing, tool calling, and memory access.
//!
//! # Overview
//!
//! The multi-agent system consists of:
//!
//! - **User Agent**: Coordinates tasks, delegates to session agents, maintains
//!   global context
//! - **Session Agents**: Manage individual coding sessions (tmux, VS Code, etc.)
//!
//! All agents implement the `Agent` trait, enabling consistent interaction patterns.
//!
//! # Core Types
//!
//! - [`Agent`]: The core trait for all agents
//! - [`AgentType`]: Enum distinguishing user vs session agents
//! - [`AgentContext`]: Context passed to agents when processing messages
//! - [`AgentResponse`]: Response returned by agents
//! - [`Message`]: Conversation message with role and content
//! - [`ToolDefinition`]: Definition of an available tool
//! - [`ToolCall`]: Request to execute a tool
//! - [`ToolResult`]: Result of tool execution
//! - [`ModelConfig`]: LLM configuration (model, temperature, etc.)
//!
//! # Example
//!
//! ```ignore
//! use commander_agent::{Agent, AgentContext, AgentResponse, AgentType, ModelConfig};
//! use commander_memory::MemoryStore;
//! use async_trait::async_trait;
//!
//! struct MyAgent {
//!     id: String,
//!     config: ModelConfig,
//!     // ... other fields
//! }
//!
//! #[async_trait]
//! impl Agent for MyAgent {
//!     fn id(&self) -> &str {
//!         &self.id
//!     }
//!
//!     fn agent_type(&self) -> AgentType {
//!         AgentType::User
//!     }
//!
//!     async fn process(&mut self, message: &str, context: &AgentContext) -> Result<AgentResponse> {
//!         // Process the message and return a response
//!         Ok(AgentResponse::text(format!("Received: {}", message)))
//!     }
//!
//!     // ... implement other methods
//! }
//! ```

pub mod agent;
pub mod client;
pub mod config;
pub mod context;
pub mod error;
pub mod response;
pub mod template;
pub mod tool;
pub mod user_agent;

// Re-export commonly used items
pub use agent::{Agent, AgentType};
pub use client::OpenRouterClient;
pub use config::{ModelConfig, Provider};
pub use context::{AgentContext, Message, MessageRole};
pub use error::{AgentError, Result};
pub use response::AgentResponse;
pub use tool::{ToolCall, ToolDefinition, ToolResult};
pub use user_agent::UserAgent;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use commander_memory::{Memory, MemoryStore, SearchResult};
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Mock memory store for testing
    struct MockMemoryStore {
        memories: RwLock<Vec<Memory>>,
    }

    impl MockMemoryStore {
        fn new() -> Self {
            Self {
                memories: RwLock::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl MemoryStore for MockMemoryStore {
        async fn store(&self, memory: Memory) -> commander_memory::Result<()> {
            let mut memories = self.memories.write().await;
            memories.push(memory);
            Ok(())
        }

        async fn search(
            &self,
            _query: &[f32],
            agent_id: &str,
            limit: usize,
        ) -> commander_memory::Result<Vec<SearchResult>> {
            let memories = self.memories.read().await;
            Ok(memories
                .iter()
                .filter(|m| m.agent_id == agent_id)
                .take(limit)
                .map(|m| SearchResult::new(m.clone(), 0.9))
                .collect())
        }

        async fn search_all(
            &self,
            _query: &[f32],
            limit: usize,
        ) -> commander_memory::Result<Vec<SearchResult>> {
            let memories = self.memories.read().await;
            Ok(memories
                .iter()
                .take(limit)
                .map(|m| SearchResult::new(m.clone(), 0.9))
                .collect())
        }

        async fn delete(&self, id: &str) -> commander_memory::Result<()> {
            let mut memories = self.memories.write().await;
            memories.retain(|m| m.id != id);
            Ok(())
        }

        async fn get(&self, id: &str) -> commander_memory::Result<Option<Memory>> {
            let memories = self.memories.read().await;
            Ok(memories.iter().find(|m| m.id == id).cloned())
        }

        async fn list(&self, agent_id: &str, limit: usize) -> commander_memory::Result<Vec<Memory>> {
            let memories = self.memories.read().await;
            Ok(memories
                .iter()
                .filter(|m| m.agent_id == agent_id)
                .take(limit)
                .cloned()
                .collect())
        }

        async fn count(&self, agent_id: &str) -> commander_memory::Result<usize> {
            let memories = self.memories.read().await;
            Ok(memories.iter().filter(|m| m.agent_id == agent_id).count())
        }

        async fn clear_agent(&self, agent_id: &str) -> commander_memory::Result<()> {
            let mut memories = self.memories.write().await;
            memories.retain(|m| m.agent_id != agent_id);
            Ok(())
        }
    }

    /// Mock agent for testing the trait
    struct MockAgent {
        id: String,
        agent_type: AgentType,
        tools: Vec<ToolDefinition>,
        memory: Arc<MockMemoryStore>,
        config: ModelConfig,
    }

    impl MockAgent {
        fn new(id: &str, agent_type: AgentType) -> Self {
            Self {
                id: id.into(),
                agent_type,
                tools: vec![ToolDefinition::no_params("test_tool", "A test tool")],
                memory: Arc::new(MockMemoryStore::new()),
                config: ModelConfig::default(),
            }
        }
    }

    #[async_trait]
    impl Agent for MockAgent {
        fn id(&self) -> &str {
            &self.id
        }

        fn agent_type(&self) -> AgentType {
            self.agent_type.clone()
        }

        async fn process(&mut self, message: &str, _context: &AgentContext) -> Result<AgentResponse> {
            Ok(AgentResponse::text(format!("Echo: {}", message)))
        }

        fn tools(&self) -> &[ToolDefinition] {
            &self.tools
        }

        async fn execute_tool(&self, call: &ToolCall) -> Result<ToolResult> {
            if call.name == "test_tool" {
                Ok(ToolResult::success(&call.id, "Tool executed successfully"))
            } else {
                Err(AgentError::ToolNotFound(call.name.clone()))
            }
        }

        fn memory(&self) -> &dyn MemoryStore {
            self.memory.as_ref()
        }

        fn model_config(&self) -> &ModelConfig {
            &self.config
        }
    }

    #[tokio::test]
    async fn test_mock_agent_process() {
        let mut agent = MockAgent::new("test-agent", AgentType::user());
        let context = AgentContext::new();

        let response = agent.process("Hello", &context).await.unwrap();

        assert_eq!(response.content, "Echo: Hello");
        assert!(!response.has_tool_calls());
    }

    #[tokio::test]
    async fn test_mock_agent_tools() {
        let agent = MockAgent::new("test-agent", AgentType::user());

        let tools = agent.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "test_tool");
    }

    #[tokio::test]
    async fn test_mock_agent_execute_tool() {
        let agent = MockAgent::new("test-agent", AgentType::user());

        // Execute known tool
        let call = ToolCall::new("test_tool", json!({}));
        let result = agent.execute_tool(&call).await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "Tool executed successfully");

        // Execute unknown tool
        let call = ToolCall::new("unknown_tool", json!({}));
        let result = agent.execute_tool(&call).await;
        assert!(matches!(result, Err(AgentError::ToolNotFound(_))));
    }

    #[tokio::test]
    async fn test_mock_agent_memory() {
        let agent = MockAgent::new("test-agent", AgentType::user());
        let memory_store = agent.memory();

        // Store a memory
        let memory = Memory::new("test-agent", "Test content", vec![0.1; 64]);
        memory_store.store(memory).await.unwrap();

        // Count memories
        let count = memory_store.count("test-agent").await.unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_mock_agent_model_config() {
        let agent = MockAgent::new("test-agent", AgentType::user());
        let config = agent.model_config();

        assert_eq!(config.model, "anthropic/claude-sonnet-4");
    }

    #[test]
    fn test_agent_type_variants() {
        let user = AgentType::user();
        assert!(user.is_user());
        assert!(!user.is_session());

        let session = AgentType::session("sess-1", "tmux");
        assert!(!session.is_user());
        assert!(session.is_session());
        assert_eq!(session.session_id(), Some("sess-1"));
        assert_eq!(session.adapter_type(), Some("tmux"));
    }

    #[test]
    fn test_full_workflow_types() {
        // Create context
        let mut context = AgentContext::with_task("Implement feature");
        context.add_message(Message::user("Please help"));
        context.set_summarized_history("Previous conversation about setup");

        // Create response with tool calls
        let tool_call = ToolCall::new("read_file", json!({"path": "/src/main.rs"}));
        let response = AgentResponse::with_tool_calls("Let me read that file.", vec![tool_call]);

        assert!(response.has_tool_calls());
        assert!(response.should_continue);

        // Create tool result
        let result = ToolResult::success(
            &response.tool_calls[0].id,
            "fn main() { println!(\"Hello\"); }",
        );
        assert!(!result.is_error);

        // Add tool result to context
        context.add_message(Message::tool(result));

        assert_eq!(context.recent_messages.len(), 2);
    }
}
