//! User Agent implementation for the AI Commander multi-agent system.
//!
//! The User Agent acts as the user's proxy, coordinating tasks and delegating
//! to session agents. It uses Claude Opus via OpenRouter for reasoning and
//! has access to memory search tools.
//!
//! ## Autonomous Execution
//!
//! The User Agent supports "Ralph" style push-to-completion behavior via the
//! [`CompletionDriver`]. It drives work forward autonomously, only stopping when:
//! - All goals are complete
//! - A blocker requires user input
//! - Maximum iterations reached (safety limit)

mod autonomous;
mod blockers;
mod tools;
#[cfg(test)]
mod tests;

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info, trace};

use commander_memory::{EmbeddingGenerator, Memory, MemoryStore};

use crate::agent::{Agent, AgentType};
use crate::client::{ChatMessage, ChatTool, OpenRouterClient};
use crate::completion_driver::CompletionDriver;
use crate::config::ModelConfig;
use crate::context::{AgentContext, Message};
use crate::error::{AgentError, Result};
use crate::response::AgentResponse;
use crate::tool::{ToolCall, ToolDefinition, ToolResult};

/// Maximum iterations in the tool calling loop.
const MAX_TOOL_ITERATIONS: u32 = 10;

/// Default system prompt for the User Agent (autonomous mode).
pub(crate) const DEFAULT_SYSTEM_PROMPT: &str = r#"You are an autonomous AI agent that drives projects to completion.

## Core Principles
1. **Take Action** - Don't ask permission, do the work
2. **Be Proactive** - Anticipate next steps and execute them
3. **Report Progress** - Tell the user what you did, not what you might do
4. **Only Stop When Blocked** - Continue until you genuinely need user input

## When to Continue Autonomously
- Task is clear and you know what to do
- You can make reasonable decisions without user input
- Errors are recoverable or you can try alternatives
- Next steps are obvious from context

## When to Stop for User
- Ambiguous requirements that could go multiple ways
- Destructive operations (delete, overwrite) on user data
- Need credentials or access you don't have
- Multiple valid approaches and user preference matters
- Error requires judgment call on how to proceed

## Response Format When Working
"[x] [What I just completed]
-> [What I'm doing next]"

## Response Format When Blocked
"[!] I need your input:
[Clear description of what's needed]

Options:
1. [Option A]
2. [Option B]
..."

## Response Format When Complete
"[DONE] Completed: [Summary of what was achieved]

Results:
- [Key outcome 1]
- [Key outcome 2]"

## Your Capabilities
You have access to tools for:
- Searching memories for relevant context
- Delegating tasks to session agents
- Querying session status

Use these proactively to drive work forward."#;

/// User Agent that acts as the user's proxy in the multi-agent system.
///
/// The User Agent coordinates tasks, maintains context through memory,
/// and delegates work to session agents. It uses Claude Opus 4 via
/// OpenRouter for reasoning.
///
/// ## Autonomous Mode
///
/// The User Agent supports autonomous "push-to-completion" behavior where it
/// drives work forward without waiting for user permission at each step.
/// Use [`process_autonomous`](Self::process_autonomous) for this mode.
pub struct UserAgent {
    /// Unique identifier for this agent.
    pub(crate) id: String,

    /// Model configuration.
    pub(crate) config: ModelConfig,

    /// Memory store for semantic search.
    pub(crate) memory: Arc<dyn MemoryStore>,

    /// Embedding generator for memory operations.
    pub(crate) embedder: EmbeddingGenerator,

    /// Available tools.
    pub(crate) tools: Vec<ToolDefinition>,

    /// OpenRouter API client.
    pub(crate) client: OpenRouterClient,

    /// Agent context for conversation history.
    pub(crate) context: AgentContext,

    /// Completion driver for autonomous execution.
    pub(crate) completion_driver: Option<CompletionDriver>,
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
            tools: tools::default_tools(),
            client,
            context: AgentContext::new(),
            completion_driver: None,
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
            tools: tools::default_tools(),
            client,
            context: AgentContext::new(),
            completion_driver: None,
        })
    }

    /// Create a User Agent with a custom API key.
    pub fn with_api_key(memory: Arc<dyn MemoryStore>, api_key: impl Into<String>) -> Self {
        let client = OpenRouterClient::new(api_key);
        let embedder = EmbeddingGenerator::from_env();

        Self {
            id: "user-agent".to_string(),
            config: Self::default_config(),
            memory,
            embedder,
            tools: tools::default_tools(),
            client,
            context: AgentContext::new(),
            completion_driver: None,
        }
    }

    /// Get the default model configuration for User Agent.
    pub(crate) fn default_config() -> ModelConfig {
        ModelConfig {
            model: "anthropic/claude-opus-4".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            provider: crate::config::Provider::OpenRouter,
            system_prompt: Some(DEFAULT_SYSTEM_PROMPT.to_string()),
            api_key: None,
        }
    }

    /// Build chat messages from context.
    pub(crate) fn build_messages(&self, user_message: &str) -> Vec<ChatMessage> {
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

    /// Get the current completion driver state.
    pub fn completion_driver(&self) -> Option<&CompletionDriver> {
        self.completion_driver.as_ref()
    }

    /// Set or replace the completion driver.
    pub fn set_completion_driver(&mut self, driver: CompletionDriver) {
        self.completion_driver = Some(driver);
    }

    /// Clear the completion driver.
    pub fn clear_completion_driver(&mut self) {
        self.completion_driver = None;
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
        info!(
            "Processing message: {}...",
            &message[..message.len().min(50)]
        );

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
                .chat(&self.config, messages.clone(), Some(chat_tools.clone()))
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
            "search_all_memories" => tools::execute_search_all_memories(self, call).await,
            "search_memories" => tools::execute_search_memories(self, call).await,
            "delegate_to_session" => tools::execute_delegate_to_session(self, call).await,
            "get_session_status" => tools::execute_get_session_status(self, call).await,
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
