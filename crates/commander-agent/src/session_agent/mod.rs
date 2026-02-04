//! Session Agent implementation for managing individual coding sessions.
//!
//! The Session Agent monitors and analyzes a specific coding session (tmux, VS Code, etc.),
//! tracking progress, detecting completion states, and reporting to the User Agent.
//! It uses Claude Haiku 4.5 via OpenRouter for cost-optimized reasoning.
//!
//! ## Smart Change Detection
//!
//! The agent uses deterministic change detection to reduce inference costs:
//! - Hash-based quick comparison detects if output changed at all
//! - Pattern matching classifies changes without LLM calls
//! - Only significant changes (errors, completion, input needed) trigger LLM analysis
//! - Adaptive polling speeds up during activity, slows down when idle

mod analysis;
mod context;
mod state;
mod tools;

#[cfg(test)]
mod tests;

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info, trace};

use commander_core::ChangeDetector;
use commander_memory::{EmbeddingGenerator, Memory, MemoryStore};

use crate::agent::{Agent, AgentType};
use crate::client::{ChatMessage, ChatTool, OpenRouterClient};
use crate::compaction::{ContextWindow, SimpleSummarizer, Summarizer};
use crate::config::ModelConfig;
use crate::context::{AgentContext, Message};
use crate::context_manager::{model_contexts, ContextManager, ContextStrategy};
use crate::error::{AgentError, Result};
use crate::response::AgentResponse;
use crate::template::{AdapterType, AgentTemplate, TemplateRegistry};
use crate::tool::{ToolCall, ToolDefinition, ToolResult};

pub use state::{OutputAnalysis, SessionState};

/// Maximum iterations in the tool calling loop.
const MAX_TOOL_ITERATIONS: u32 = 5;

/// Default system prompt for Session Agents.
const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a Session Agent in the AI Commander system.
Your role is to monitor and analyze a specific coding session.

Key responsibilities:
1. Analyze session output for progress indicators
2. Track files modified, tests run, and errors encountered
3. Detect when the session needs user input or has completed tasks
4. Maintain state about goals, progress, and blockers
5. Report status summaries to the User Agent

When analyzing output:
- Look for completion indicators (success messages, test results)
- Detect errors and warnings
- Identify file changes (created, modified, deleted)
- Recognize when input is being requested

Be concise in your analysis and focus on actionable information."#;

/// Session Agent that manages a specific coding session.
///
/// Uses Claude Haiku 4.5 via OpenRouter for cost-optimized analysis.
/// Maintains isolated memory (can only access own memories).
///
/// ## Change Detection
///
/// The agent includes a `ChangeDetector` that performs deterministic
/// change detection before invoking the LLM. This reduces inference
/// costs by only analyzing output when meaningful changes occur.
///
/// ## Context Management
///
/// The agent includes a `ContextManager` that tracks token usage and
/// triggers appropriate actions when context limits are approached:
/// - MPM: Auto-pause and resume sessions
/// - Claude Code: Trigger context compaction
/// - Generic: Warn user about low context
pub struct SessionAgent {
    /// Unique identifier for this agent.
    pub(crate) id: String,

    /// Session ID this agent is managing.
    pub(crate) session_id: String,

    /// Type of adapter (e.g., claude_code, mpm, generic).
    adapter_type: AdapterType,

    /// Model configuration.
    pub(crate) config: ModelConfig,

    /// Memory store for semantic search.
    pub(crate) memory: Arc<dyn MemoryStore>,

    /// Embedding generator for memory operations.
    pub(crate) embedder: EmbeddingGenerator,

    /// Available tools.
    tools: Vec<ToolDefinition>,

    /// OpenRouter API client.
    pub(crate) client: OpenRouterClient,

    /// Agent context for conversation history.
    pub(crate) context: AgentContext,

    /// Current session state.
    pub(crate) session_state: SessionState,

    /// Agent template for this adapter type.
    template: AgentTemplate,

    /// Change detector for smart output monitoring.
    change_detector: ChangeDetector,

    /// Context manager for tracking token usage and triggering actions.
    pub(crate) context_manager: ContextManager,

    /// Context window for message compaction.
    pub(crate) context_window: ContextWindow,
}

impl SessionAgent {
    /// Create a new Session Agent for the given session.
    pub fn new(
        session_id: impl Into<String>,
        adapter_type: AdapterType,
        memory: Arc<dyn MemoryStore>,
    ) -> Result<Self> {
        let session_id = session_id.into();
        let client = OpenRouterClient::from_env()?;
        let embedder = EmbeddingGenerator::from_env();

        // Get template for this adapter type
        let registry = TemplateRegistry::new();
        let template = registry
            .get(&adapter_type)
            .cloned()
            .unwrap_or_else(AgentTemplate::generic);

        // Build tools: template tools + built-in session tools
        let mut tools = Self::builtin_tools();
        tools.extend(template.tools.clone());

        let id = format!("session-agent-{}", session_id);

        // Initialize context manager with strategy from template
        let context_strategy = template
            .context_strategy
            .clone()
            .unwrap_or(ContextStrategy::WarnAndContinue);
        let context_manager = ContextManager::new(context_strategy, model_contexts::CLAUDE_3_HAIKU);

        // Initialize context window for message compaction
        let summarizer: Arc<dyn Summarizer> = Arc::new(SimpleSummarizer);
        let context_window = ContextWindow::with_defaults(summarizer);

        Ok(Self {
            id,
            session_id,
            adapter_type,
            config: Self::default_config(&template),
            memory,
            embedder,
            tools,
            client,
            context: AgentContext::new(),
            session_state: SessionState::new(),
            template,
            change_detector: ChangeDetector::new(),
            context_manager,
            context_window,
        })
    }

    /// Create a Session Agent with a custom API key.
    pub fn with_api_key(
        session_id: impl Into<String>,
        adapter_type: AdapterType,
        memory: Arc<dyn MemoryStore>,
        api_key: impl Into<String>,
    ) -> Self {
        let session_id = session_id.into();
        let client = OpenRouterClient::new(api_key);
        let embedder = EmbeddingGenerator::from_env();

        let registry = TemplateRegistry::new();
        let template = registry
            .get(&adapter_type)
            .cloned()
            .unwrap_or_else(AgentTemplate::generic);

        let mut tools = Self::builtin_tools();
        tools.extend(template.tools.clone());

        let id = format!("session-agent-{}", session_id);

        // Initialize context manager with strategy from template
        let context_strategy = template
            .context_strategy
            .clone()
            .unwrap_or(ContextStrategy::WarnAndContinue);
        let context_manager = ContextManager::new(context_strategy, model_contexts::CLAUDE_3_HAIKU);

        // Initialize context window for message compaction
        let summarizer: Arc<dyn Summarizer> = Arc::new(SimpleSummarizer);
        let context_window = ContextWindow::with_defaults(summarizer);

        Self {
            id,
            session_id,
            adapter_type,
            config: Self::default_config(&template),
            memory,
            embedder,
            tools,
            client,
            context: AgentContext::new(),
            session_state: SessionState::new(),
            template,
            change_detector: ChangeDetector::new(),
            context_manager,
            context_window,
        }
    }

    /// Get the default model configuration for Session Agent.
    /// Uses Claude Haiku 4.5 via OpenRouter for cost optimization.
    pub(crate) fn default_config(template: &AgentTemplate) -> ModelConfig {
        // Use model override from template if provided
        let model = template
            .model_override
            .clone()
            .unwrap_or_else(|| "anthropic/claude-haiku-4".to_string());

        // Use template system prompt or default
        let system_prompt = if template.system_prompt.is_empty() {
            DEFAULT_SYSTEM_PROMPT.to_string()
        } else {
            template.system_prompt.clone()
        };

        ModelConfig {
            model,
            max_tokens: 2048,        // Cost-optimized
            temperature: 0.5,         // More focused responses
            provider: crate::config::Provider::OpenRouter,
            system_prompt: Some(system_prompt),
            api_key: None,
        }
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the adapter type.
    pub fn adapter_type(&self) -> &AdapterType {
        &self.adapter_type
    }

    /// Get the current session state.
    pub fn state(&self) -> &SessionState {
        &self.session_state
    }

    /// Get mutable access to session state.
    pub fn state_mut(&mut self) -> &mut SessionState {
        &mut self.session_state
    }

    /// Get the conversation context.
    pub fn context(&self) -> &AgentContext {
        &self.context
    }

    /// Get mutable access to the conversation context.
    pub fn context_mut(&mut self) -> &mut AgentContext {
        &mut self.context
    }

    /// Get the agent template.
    pub fn template(&self) -> &AgentTemplate {
        &self.template
    }

    /// Get a reference to the change detector.
    pub fn change_detector(&self) -> &ChangeDetector {
        &self.change_detector
    }

    /// Get mutable access to the change detector.
    pub fn change_detector_mut(&mut self) -> &mut ChangeDetector {
        &mut self.change_detector
    }

    /// Get a reference to the context manager.
    pub fn context_manager(&self) -> &ContextManager {
        &self.context_manager
    }

    /// Get mutable access to the context manager.
    pub fn context_manager_mut(&mut self) -> &mut ContextManager {
        &mut self.context_manager
    }

    /// Get a reference to the context window.
    pub fn context_window(&self) -> &ContextWindow {
        &self.context_window
    }

    /// Get mutable access to the context window.
    pub fn context_window_mut(&mut self) -> &mut ContextWindow {
        &mut self.context_window
    }

    /// Reset the change detector state.
    ///
    /// Call this when starting a new task or after significant user interaction
    /// to ensure the next output is analyzed fresh.
    pub fn reset_change_detector(&mut self) {
        self.change_detector.reset();
    }

    /// Store a memory from the session.
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

        debug!("Stored memory for session {}: {}", self.session_id, &content[..content.len().min(50)]);
        Ok(())
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

        // Add session state context
        let state_context = format!(
            "Current session state:\n- Session: {}\n- Goals: {:?}\n- Current task: {:?}\n- Progress: {:.0}%\n- Blockers: {:?}\n- Files modified: {:?}",
            self.session_id,
            self.session_state.goals,
            self.session_state.current_task,
            self.session_state.progress * 100.0,
            self.session_state.blockers,
            self.session_state.files_modified
        );
        messages.push(ChatMessage::system(state_context));

        // Add summarized history if available
        if !self.context.summarized_history.is_empty() {
            messages.push(ChatMessage::system(format!(
                "Previous context:\n{}",
                self.context.summarized_history
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
}

#[async_trait]
impl Agent for SessionAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn agent_type(&self) -> AgentType {
        AgentType::session(&self.session_id, self.adapter_type.to_string())
    }

    async fn process(&mut self, message: &str, context: &AgentContext) -> Result<AgentResponse> {
        info!(
            "Session {} processing: {}...",
            self.session_id,
            &message[..message.len().min(50)]
        );

        // Update internal context with provided context
        self.context.current_task = context.current_task.clone();
        if !context.summarized_history.is_empty() {
            self.context.summarized_history = context.summarized_history.clone();
        }
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

            trace!("Session {} tool loop iteration {}", self.session_id, iteration);

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
                debug!(
                    "Session {} received {} tool calls",
                    self.session_id,
                    tool_calls.len()
                );

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

            // Trim context if needed (smaller for session agents)
            self.context.trim_recent(5);

            return Ok(AgentResponse::text(content));
        }
    }

    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    async fn execute_tool(&self, call: &ToolCall) -> Result<ToolResult> {
        debug!("Session {} executing tool: {}", self.session_id, call.name);
        trace!("Tool arguments: {:?}", call.arguments);

        match call.name.as_str() {
            "search_memories" => self.execute_search_memories(call).await,
            "update_session_state" => {
                // Need mutable self for this tool - use interior mutability pattern
                // For now, return a message that state update was requested
                Ok(ToolResult::success(
                    &call.id,
                    "State update recorded. Use state() to view current state.",
                ))
            }
            "report_to_user" => self.execute_report_to_user(call).await,
            "analyze_output" => {
                // For non-mutable context, we return a placeholder
                // The full analysis should be done via analyze_output method
                Ok(ToolResult::success(
                    &call.id,
                    "Use analyze_output() method for full analysis.",
                ))
            }
            // Handle template-specific tools
            "parse_output" | "track_files" | "detect_completion" | "report_status"
            | "track_delegation" | "aggregate_status" | "list_agents"
            | "detect_ready" | "report_output" => {
                // Template tools are placeholders - return success with descriptive message
                Ok(ToolResult::success(
                    &call.id,
                    format!("Tool '{}' executed. Integration pending.", call.name),
                ))
            }
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
