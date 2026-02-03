//! User Agent implementation for the AI Commander multi-agent system.
//!
//! The User Agent acts as the user's proxy, coordinating tasks and delegating
//! to session agents. It uses Claude Opus via OpenRouter for reasoning and
//! has access to memory search tools.

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, info, trace};

use commander_memory::{EmbeddingGenerator, Memory, MemoryStore, SearchResult};

use crate::agent::{Agent, AgentType};
use crate::client::{ChatMessage, ChatTool, OpenRouterClient};
use crate::config::ModelConfig;
use crate::context::{AgentContext, Message};
use crate::error::{AgentError, Result};
use crate::response::AgentResponse;
use crate::tool::{ToolCall, ToolDefinition, ToolResult};

/// Maximum iterations in the tool calling loop.
const MAX_TOOL_ITERATIONS: u32 = 10;

/// Default system prompt for the User Agent.
const DEFAULT_SYSTEM_PROMPT: &str = r#"You are the User Agent in the AI Commander system. Your role is to:

1. Understand what the user wants to accomplish
2. Search memories for relevant context and past interactions
3. Coordinate with session agents to execute coding tasks
4. Maintain context across conversations

You have access to tools for searching memories and delegating tasks to session agents.
When working on a task:
- First search for relevant memories to understand context
- Consider what information you need from the user
- Delegate specific coding tasks to session agents
- Track progress and report back to the user

Be helpful, thorough, and proactive in driving projects to completion."#;

/// User Agent that acts as the user's proxy in the multi-agent system.
///
/// The User Agent coordinates tasks, maintains context through memory,
/// and delegates work to session agents. It uses Claude Opus 4 via
/// OpenRouter for reasoning.
pub struct UserAgent {
    /// Unique identifier for this agent.
    id: String,

    /// Model configuration.
    config: ModelConfig,

    /// Memory store for semantic search.
    memory: Arc<dyn MemoryStore>,

    /// Embedding generator for memory operations.
    embedder: EmbeddingGenerator,

    /// Available tools.
    tools: Vec<ToolDefinition>,

    /// OpenRouter API client.
    client: OpenRouterClient,

    /// Agent context for conversation history.
    context: AgentContext,
}

impl UserAgent {
    /// Create a new User Agent with the given memory store.
    pub fn new(memory: Arc<dyn MemoryStore>) -> Result<Self> {
        let client = OpenRouterClient::from_env()?;
        let embedder = EmbeddingGenerator::from_env();

        Ok(Self {
            id: "user-agent".to_string(),
            config: Self::default_config(),
            memory,
            embedder,
            tools: Self::default_tools(),
            client,
            context: AgentContext::new(),
        })
    }

    /// Create a User Agent with custom configuration.
    pub fn with_config(memory: Arc<dyn MemoryStore>, config: ModelConfig) -> Result<Self> {
        let client = OpenRouterClient::from_env()?;
        let embedder = EmbeddingGenerator::from_env();

        Ok(Self {
            id: "user-agent".to_string(),
            config,
            memory,
            embedder,
            tools: Self::default_tools(),
            client,
            context: AgentContext::new(),
        })
    }

    /// Create a User Agent with a custom API key.
    pub fn with_api_key(
        memory: Arc<dyn MemoryStore>,
        api_key: impl Into<String>,
    ) -> Self {
        let client = OpenRouterClient::new(api_key);
        let embedder = EmbeddingGenerator::from_env();

        Self {
            id: "user-agent".to_string(),
            config: Self::default_config(),
            memory,
            embedder,
            tools: Self::default_tools(),
            client,
            context: AgentContext::new(),
        }
    }

    /// Get the default model configuration for User Agent.
    fn default_config() -> ModelConfig {
        ModelConfig {
            model: "anthropic/claude-opus-4".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            provider: crate::config::Provider::OpenRouter,
            system_prompt: Some(DEFAULT_SYSTEM_PROMPT.to_string()),
            api_key: None,
        }
    }

    /// Get the default tools for User Agent.
    fn default_tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition::new(
                "search_all_memories",
                "Search across all agent memories for relevant information",
                json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query to find relevant memories"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default: 5)",
                            "default": 5
                        }
                    },
                    "required": ["query"]
                }),
            ),
            ToolDefinition::new(
                "search_memories",
                "Search memories for a specific agent",
                json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query to find relevant memories"
                        },
                        "agent_id": {
                            "type": "string",
                            "description": "The agent ID to search within"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default: 5)",
                            "default": 5
                        }
                    },
                    "required": ["query", "agent_id"]
                }),
            ),
            ToolDefinition::new(
                "delegate_to_session",
                "Send a task to a session agent for execution",
                json!({
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "The session ID to delegate to"
                        },
                        "task": {
                            "type": "string",
                            "description": "The task description to execute"
                        },
                        "context": {
                            "type": "string",
                            "description": "Additional context for the task"
                        }
                    },
                    "required": ["session_id", "task"]
                }),
            ),
            ToolDefinition::new(
                "get_session_status",
                "Query the current status of a session agent",
                json!({
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "The session ID to query"
                        }
                    },
                    "required": ["session_id"]
                }),
            ),
        ]
    }

    /// Build chat messages from context.
    fn build_messages(&self, user_message: &str) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        // System prompt
        let system_prompt = self
            .config
            .system_prompt
            .as_deref()
            .unwrap_or(DEFAULT_SYSTEM_PROMPT);
        messages.push(ChatMessage::system(system_prompt));

        // Add summarized history if available
        if !self.context.summarized_history.is_empty() {
            messages.push(ChatMessage::system(format!(
                "Previous conversation summary:\n{}",
                self.context.summarized_history
            )));
        }

        // Add relevant memories if available
        if !self.context.relevant_memories.is_empty() {
            let memories: Vec<String> = self
                .context
                .relevant_memories
                .iter()
                .map(|m| format!("- {}", m.content))
                .collect();
            messages.push(ChatMessage::system(format!(
                "Relevant memories:\n{}",
                memories.join("\n")
            )));
        }

        // Add recent messages
        for msg in &self.context.recent_messages {
            messages.push(ChatMessage::from_message(msg));
        }

        // Add the new user message
        messages.push(ChatMessage::user(user_message));

        messages
    }

    /// Execute the search_all_memories tool.
    async fn execute_search_all_memories(&self, call: &ToolCall) -> Result<ToolResult> {
        let query = call.get_string_arg("query").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        let limit = call
            .get_arg("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        debug!("Searching all memories for: {} (limit: {})", query, limit);

        // Generate embedding for the query
        let embedding = self
            .embedder
            .embed(query)
            .await
            .map_err(|e| AgentError::ToolExecution {
                tool_name: call.name.clone(),
                message: format!("Failed to generate embedding: {}", e),
            })?;

        // Search memories
        let results = self
            .memory
            .search_all(&embedding, limit)
            .await
            .map_err(AgentError::Memory)?;

        let output = format_search_results(&results);
        Ok(ToolResult::success(&call.id, output))
    }

    /// Execute the search_memories tool.
    async fn execute_search_memories(&self, call: &ToolCall) -> Result<ToolResult> {
        let query = call.get_string_arg("query").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        let agent_id = call.get_string_arg("agent_id").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        let limit = call
            .get_arg("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        debug!(
            "Searching memories for agent '{}': {} (limit: {})",
            agent_id, query, limit
        );

        // Generate embedding for the query
        let embedding = self
            .embedder
            .embed(query)
            .await
            .map_err(|e| AgentError::ToolExecution {
                tool_name: call.name.clone(),
                message: format!("Failed to generate embedding: {}", e),
            })?;

        // Search memories
        let results = self
            .memory
            .search(&embedding, agent_id, limit)
            .await
            .map_err(AgentError::Memory)?;

        let output = format_search_results(&results);
        Ok(ToolResult::success(&call.id, output))
    }

    /// Execute the delegate_to_session tool (placeholder).
    async fn execute_delegate_to_session(&self, call: &ToolCall) -> Result<ToolResult> {
        let session_id = call.get_string_arg("session_id").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        let task = call.get_string_arg("task").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        let context = call.get_optional_string_arg("context");

        info!(
            "Delegating task to session '{}': {}",
            session_id, task
        );

        // Placeholder - will be implemented when session agent integration is complete
        let output = format!(
            "Task delegated to session '{}': {}\nContext: {}\n\nNote: Session agent integration is not yet implemented. This is a placeholder response.",
            session_id,
            task,
            context.unwrap_or("None")
        );

        Ok(ToolResult::success(&call.id, output))
    }

    /// Execute the get_session_status tool (placeholder).
    async fn execute_get_session_status(&self, call: &ToolCall) -> Result<ToolResult> {
        let session_id = call.get_string_arg("session_id").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        debug!("Querying status of session '{}'", session_id);

        // Placeholder - will be implemented when session agent integration is complete
        let output = format!(
            "Session '{}' status:\n- State: Not implemented\n- Note: Session agent integration is not yet implemented. This is a placeholder response.",
            session_id
        );

        Ok(ToolResult::success(&call.id, output))
    }

    /// Store a memory from the conversation.
    pub async fn store_memory(&self, content: &str) -> Result<()> {
        let embedding = self
            .embedder
            .embed(content)
            .await
            .map_err(|e| AgentError::ToolExecution {
                tool_name: "store_memory".to_string(),
                message: format!("Failed to generate embedding: {}", e),
            })?;

        let memory = Memory::new(&self.id, content, embedding);
        self.memory.store(memory).await.map_err(AgentError::Memory)?;

        debug!("Stored memory: {}", &content[..content.len().min(50)]);
        Ok(())
    }

    /// Set the current task.
    pub fn set_task(&mut self, task: impl Into<String>) {
        self.context.set_task(task);
    }

    /// Clear the current task.
    pub fn clear_task(&mut self) {
        self.context.clear_task();
    }

    /// Get the conversation context.
    pub fn context(&self) -> &AgentContext {
        &self.context
    }

    /// Get mutable access to the conversation context.
    pub fn context_mut(&mut self) -> &mut AgentContext {
        &mut self.context
    }
}

#[async_trait]
impl Agent for UserAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn agent_type(&self) -> AgentType {
        AgentType::User
    }

    async fn process(&mut self, message: &str, context: &AgentContext) -> Result<AgentResponse> {
        info!("Processing message: {}...", &message[..message.len().min(50)]);

        // Update internal context with provided context
        self.context.current_task = context.current_task.clone();
        self.context.summarized_history = context.summarized_history.clone();
        self.context.relevant_memories = context.relevant_memories.clone();

        // Build chat messages
        let mut messages = self.build_messages(message);

        // Convert tools to chat format
        let chat_tools: Vec<ChatTool> = self
            .tools
            .iter()
            .map(ChatTool::from_definition)
            .collect();

        // Tool calling loop
        let mut iteration = 0;
        loop {
            iteration += 1;
            if iteration > MAX_TOOL_ITERATIONS {
                return Err(AgentError::MaxIterationsExceeded(MAX_TOOL_ITERATIONS));
            }

            trace!("Tool loop iteration {}", iteration);

            // Send request to OpenRouter
            let response = self
                .client
                .chat(
                    &self.config,
                    messages.clone(),
                    Some(chat_tools.clone()),
                )
                .await?;

            // Check for tool calls
            if response.has_tool_calls() {
                let tool_calls = response.tool_calls();
                debug!("Received {} tool calls", tool_calls.len());

                // Add assistant message with tool calls
                let assistant_content = response.message().and_then(|m| m.content.clone());
                let chat_tool_calls: Vec<_> = response
                    .message()
                    .and_then(|m| m.tool_calls.clone())
                    .unwrap_or_default();
                messages.push(ChatMessage::assistant_with_tools(
                    assistant_content,
                    chat_tool_calls,
                ));

                // Execute each tool call
                for call in &tool_calls {
                    let result = self.execute_tool(call).await?;
                    messages.push(ChatMessage::tool(&call.id, &result.content));
                }

                // Continue the loop to get the next response
                continue;
            }

            // No tool calls, extract final response
            let content = response
                .message()
                .and_then(|m| m.content.clone())
                .unwrap_or_default();

            // Add user message and assistant response to context
            self.context.add_message(Message::user(message));
            self.context.add_message(Message::assistant(&content));

            // Trim context if needed
            self.context.trim_recent(10);

            return Ok(AgentResponse::text(content));
        }
    }

    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    async fn execute_tool(&self, call: &ToolCall) -> Result<ToolResult> {
        debug!("Executing tool: {}", call.name);
        trace!("Tool arguments: {:?}", call.arguments);

        match call.name.as_str() {
            "search_all_memories" => self.execute_search_all_memories(call).await,
            "search_memories" => self.execute_search_memories(call).await,
            "delegate_to_session" => self.execute_delegate_to_session(call).await,
            "get_session_status" => self.execute_get_session_status(call).await,
            _ => Err(AgentError::ToolNotFound(call.name.clone())),
        }
    }

    fn memory(&self) -> &dyn MemoryStore {
        self.memory.as_ref()
    }

    fn model_config(&self) -> &ModelConfig {
        &self.config
    }
}

/// Format search results as a human-readable string.
fn format_search_results(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No relevant memories found.".to_string();
    }

    let mut output = format!("Found {} relevant memories:\n\n", results.len());

    for (i, result) in results.iter().enumerate() {
        output.push_str(&format!(
            "{}. [Score: {:.2}] {}\n   Agent: {}, Created: {}\n\n",
            i + 1,
            result.score,
            result.memory.content,
            result.memory.agent_id,
            result.memory.created_at.format("%Y-%m-%d %H:%M:%S")
        ));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::RwLock;

    /// Mock memory store for testing.
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
            _query_embedding: &[f32],
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
            _query_embedding: &[f32],
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

    #[test]
    fn test_default_config() {
        let config = UserAgent::default_config();
        assert_eq!(config.model, "anthropic/claude-opus-4");
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.temperature, 0.7);
    }

    #[test]
    fn test_default_tools() {
        let tools = UserAgent::default_tools();
        assert_eq!(tools.len(), 4);

        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"search_all_memories"));
        assert!(tool_names.contains(&"search_memories"));
        assert!(tool_names.contains(&"delegate_to_session"));
        assert!(tool_names.contains(&"get_session_status"));
    }

    #[test]
    fn test_format_search_results_empty() {
        let results: Vec<SearchResult> = vec![];
        let output = format_search_results(&results);
        assert_eq!(output, "No relevant memories found.");
    }

    #[test]
    fn test_format_search_results() {
        let memory = Memory::new("agent-1", "Test memory content", vec![0.1; 64]);
        let results = vec![SearchResult::new(memory, 0.95)];

        let output = format_search_results(&results);
        assert!(output.contains("Found 1 relevant memories"));
        assert!(output.contains("Test memory content"));
        assert!(output.contains("0.95"));
    }

    #[test]
    fn test_user_agent_id() {
        // We can't create a full UserAgent without API key, but we can test the default_tools
        let tools = UserAgent::default_tools();
        assert!(!tools.is_empty());
    }

    #[tokio::test]
    async fn test_mock_memory_store() {
        let store = MockMemoryStore::new();
        let memory = Memory::new("test-agent", "Test content", vec![0.1; 64]);

        store.store(memory).await.unwrap();

        let count = store.count("test-agent").await.unwrap();
        assert_eq!(count, 1);

        let results = store.search_all(&[0.1; 64], 10).await.unwrap();
        assert_eq!(results.len(), 1);
    }
}
