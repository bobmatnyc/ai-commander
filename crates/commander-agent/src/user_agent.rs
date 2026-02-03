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

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, info, trace, warn};

use commander_memory::{EmbeddingGenerator, Memory, MemoryStore, SearchResult};

use crate::agent::{Agent, AgentType};
use crate::client::{ChatMessage, ChatTool, OpenRouterClient};
use crate::completion_driver::{
    AutonomousResult, Blocker, BlockerType, CompletionDriver, ContinueDecision, Goal, GoalStatus,
};
use crate::config::ModelConfig;
use crate::context::{AgentContext, Message};
use crate::error::{AgentError, Result};
use crate::response::AgentResponse;
use crate::tool::{ToolCall, ToolDefinition, ToolResult};

/// Maximum iterations in the tool calling loop.
const MAX_TOOL_ITERATIONS: u32 = 10;

/// Default system prompt for the User Agent (autonomous mode).
const DEFAULT_SYSTEM_PROMPT: &str = r#"You are an autonomous AI agent that drives projects to completion.

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

    /// Completion driver for autonomous execution.
    completion_driver: Option<CompletionDriver>,
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
            tools: Self::default_tools(),
            client,
            context: AgentContext::new(),
            completion_driver: None,
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
            completion_driver: None,
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

    // ==================== Autonomous Execution ====================

    /// Process a user request autonomously until completion or blocker.
    ///
    /// This implements "Ralph" style push-to-completion behavior where the agent
    /// drives work forward, only stopping when:
    /// - All goals are complete
    /// - A blocker requires user input
    /// - Maximum iterations reached (safety limit)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = agent.process_autonomous("Implement user authentication").await?;
    /// match result {
    ///     AutonomousResult::Complete { summary, .. } => println!("Done: {}", summary),
    ///     AutonomousResult::NeedsInput { reason, blockers, .. } => {
    ///         println!("Blocked: {}", reason);
    ///         for blocker in blockers {
    ///             println!("- {}", blocker.reason);
    ///         }
    ///     }
    ///     AutonomousResult::CheckIn { progress, .. } => println!("Progress: {}", progress),
    /// }
    /// ```
    pub async fn process_autonomous(&mut self, initial_request: &str) -> Result<AutonomousResult> {
        info!("Starting autonomous processing: {}...", &initial_request[..initial_request.len().min(50)]);

        // Initialize completion driver
        let mut driver = CompletionDriver::new();

        // Parse initial request into goals
        let goals = self.parse_goals(initial_request).await?;
        driver.set_goals(goals);

        info!("Parsed {} goals from request", driver.goals().len());

        // Main autonomous loop
        loop {
            match driver.should_continue() {
                ContinueDecision::Continue => {
                    // Execute next action
                    let action_result = self.execute_next_action(&mut driver).await;

                    match action_result {
                        Ok(Some(blocker)) => {
                            driver.add_blocker(blocker);
                        }
                        Ok(None) => {
                            // Action completed successfully
                        }
                        Err(e) => {
                            // Error occurred - determine if we should add a blocker
                            warn!("Action error: {}", e);
                            let blocker = self.classify_error_as_blocker(&e);
                            if let Some(b) = blocker {
                                driver.add_blocker(b);
                            } else {
                                // Recoverable error, continue
                                debug!("Error was recoverable, continuing");
                            }
                        }
                    }

                    driver.increment_iteration();
                }
                ContinueDecision::StopForUser { reason, blockers } => {
                    info!("Stopping for user input: {}", reason);
                    return Ok(AutonomousResult::NeedsInput {
                        reason,
                        blockers,
                        progress: driver.format_progress(),
                    });
                }
                ContinueDecision::CheckIn { reason, progress } => {
                    info!("Periodic check-in: {}", reason);
                    return Ok(AutonomousResult::CheckIn { reason, progress });
                }
                ContinueDecision::Complete { summary } => {
                    info!("All goals complete");
                    return Ok(AutonomousResult::Complete {
                        summary,
                        goals_achieved: driver.goals().to_vec(),
                    });
                }
            }
        }
    }

    /// Resume autonomous processing after user provides input.
    ///
    /// Call this after receiving user input that resolves blockers.
    pub async fn resume_autonomous(
        &mut self,
        user_input: &str,
        driver: &mut CompletionDriver,
    ) -> Result<AutonomousResult> {
        info!("Resuming autonomous processing with user input");

        // Clear blockers since user provided input
        driver.clear_blockers();
        driver.reset_iterations();

        // Process the user input to update context
        let context = self.context.clone();
        let _ = self.process(user_input, &context).await?;

        // Continue autonomous processing
        loop {
            match driver.should_continue() {
                ContinueDecision::Continue => {
                    let action_result = self.execute_next_action(driver).await;
                    if let Ok(Some(blocker)) = action_result {
                        driver.add_blocker(blocker);
                    }
                    driver.increment_iteration();
                }
                ContinueDecision::StopForUser { reason, blockers } => {
                    return Ok(AutonomousResult::NeedsInput {
                        reason,
                        blockers,
                        progress: driver.format_progress(),
                    });
                }
                ContinueDecision::CheckIn { reason, progress } => {
                    return Ok(AutonomousResult::CheckIn { reason, progress });
                }
                ContinueDecision::Complete { summary } => {
                    return Ok(AutonomousResult::Complete {
                        summary,
                        goals_achieved: driver.goals().to_vec(),
                    });
                }
            }
        }
    }

    /// Parse a user request into actionable goals.
    async fn parse_goals(&mut self, request: &str) -> Result<Vec<Goal>> {
        // Use the LLM to parse goals from the request
        let goal_prompt = format!(
            r#"Analyze this request and extract actionable goals.
Return goals as a simple numbered list, one goal per line.
Keep goals specific and actionable.

Request: {}

Goals:"#,
            request
        );

        let messages = vec![
            ChatMessage::system("You are a task decomposition assistant. Extract clear, actionable goals from user requests."),
            ChatMessage::user(&goal_prompt),
        ];

        let response = self.client.chat(&self.config, messages, None).await?;

        let content = response
            .message()
            .and_then(|m| m.content.clone())
            .unwrap_or_default();

        // Parse the response into goals
        let goals: Vec<Goal> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                // Remove numbering like "1. " or "- "
                let cleaned = line
                    .trim()
                    .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-' || c == ' ');
                Goal::new(cleaned.trim())
            })
            .filter(|g| !g.description.is_empty())
            .collect();

        // If parsing failed, create a single goal from the original request
        if goals.is_empty() {
            Ok(vec![Goal::new(request)])
        } else {
            Ok(goals)
        }
    }

    /// Execute the next action toward completing goals.
    async fn execute_next_action(&mut self, driver: &mut CompletionDriver) -> Result<Option<Blocker>> {
        // Find the next goal to work on
        let next_goal = if let Some(current) = driver.current_goal() {
            current.description.clone()
        } else if let Some(pending) = driver.next_pending_goal() {
            // Mark it as in progress
            let desc = pending.description.clone();
            driver.update_goal_status(&desc, GoalStatus::InProgress);
            desc
        } else {
            // No more goals
            return Ok(None);
        };

        debug!("Working on goal: {}", next_goal);

        // Generate action for this goal
        let action_prompt = format!(
            r#"You are working on this goal: {}

Current progress:
{}

Determine the next concrete action to take. If you need to use a tool, use it.
If this goal is complete, say "[GOAL COMPLETE]".
If you're blocked and need user input, say "[BLOCKED]" followed by what you need.

What is your next action?"#,
            next_goal,
            driver.format_progress()
        );

        // Process through the normal flow which handles tool calling
        let context = self.context.clone();
        let response = self.process(&action_prompt, &context).await?;

        // Analyze the response
        let content = response.content.to_lowercase();

        if content.contains("[goal complete]") || content.contains("completed") || content.contains("[done]") {
            driver.complete_goal(&next_goal);
            info!("Goal completed: {}", next_goal);
            return Ok(None);
        }

        if content.contains("[blocked]") || content.contains("need your input") || content.contains("cannot proceed") {
            // Extract blocker reason from response
            let reason = self.extract_blocker_reason(&response.content);
            let blocker_type = self.classify_blocker_type(&response.content);
            let options = self.extract_options(&response.content);

            return Ok(Some(Blocker::with_options(reason, blocker_type, options)));
        }

        // Goal still in progress
        Ok(None)
    }

    /// Classify an error to determine if it should create a blocker.
    fn classify_error_as_blocker(&self, error: &AgentError) -> Option<Blocker> {
        match error {
            AgentError::Configuration(msg) => {
                Some(Blocker::external(format!("Configuration error: {}", msg)))
            }
            AgentError::MaxIterationsExceeded(_) => {
                Some(Blocker::new(
                    "Maximum iterations reached - may need guidance",
                    BlockerType::DecisionNeeded,
                ))
            }
            AgentError::ToolExecution { tool_name, message } => {
                // Some tool errors are recoverable
                if message.contains("not found") || message.contains("permission") {
                    Some(Blocker::error_judgment(
                        format!("Tool '{}' failed: {}", tool_name, message),
                        vec!["Retry".into(), "Skip this step".into(), "Try alternative".into()],
                    ))
                } else {
                    None // Recoverable
                }
            }
            _ => None, // Most errors are recoverable
        }
    }

    /// Extract blocker reason from response text.
    fn extract_blocker_reason(&self, content: &str) -> String {
        // Look for text after [BLOCKED] marker
        if let Some(idx) = content.to_lowercase().find("[blocked]") {
            let after = &content[idx + 9..];
            let reason = after
                .lines()
                .next()
                .unwrap_or("User input needed")
                .trim()
                .trim_start_matches(':')
                .trim();
            if !reason.is_empty() {
                return reason.to_string();
            }
        }

        // Look for "need" phrases
        for line in content.lines() {
            let lower = line.to_lowercase();
            if lower.contains("need") && (lower.contains("input") || lower.contains("decision") || lower.contains("information")) {
                return line.trim().to_string();
            }
        }

        "User input needed to proceed".to_string()
    }

    /// Classify the type of blocker from response text.
    fn classify_blocker_type(&self, content: &str) -> BlockerType {
        let lower = content.to_lowercase();

        if lower.contains("decision") || lower.contains("choose") || lower.contains("option") {
            BlockerType::DecisionNeeded
        } else if lower.contains("credential") || lower.contains("api key") || lower.contains("access") {
            BlockerType::ExternalDependency
        } else if lower.contains("error") || lower.contains("failed") {
            BlockerType::ErrorRequiresJudgment
        } else if lower.contains("unclear") || lower.contains("ambiguous") || lower.contains("which") {
            BlockerType::AmbiguousRequirements
        } else {
            BlockerType::InformationNeeded
        }
    }

    /// Extract options from response text.
    fn extract_options(&self, content: &str) -> Vec<String> {
        let mut options = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            // Look for numbered options like "1. " or "1) "
            if trimmed.len() > 2 {
                let first_char = trimmed.chars().next().unwrap_or(' ');
                if first_char.is_ascii_digit() {
                    let rest = trimmed[1..].trim_start_matches(['.', ')', ':', ' '].as_ref());
                    if !rest.is_empty() {
                        options.push(rest.to_string());
                    }
                }
            }
        }

        options
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

    // ==================== Memory Operations ====================

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

    // ==================== Autonomous Behavior Tests ====================

    #[test]
    fn test_extract_blocker_reason() {
        // Create a minimal agent for testing helper methods
        let agent = create_test_agent_struct();

        // Test [BLOCKED] marker extraction
        let content = "[BLOCKED] Need database credentials to proceed";
        let reason = agent.extract_blocker_reason(content);
        assert!(reason.contains("credentials") || reason.contains("database"));

        // Test "need" phrase extraction
        let content = "I need your input on which approach to use";
        let reason = agent.extract_blocker_reason(content);
        assert!(reason.contains("need"));

        // Test fallback
        let content = "Something without clear markers";
        let reason = agent.extract_blocker_reason(content);
        assert_eq!(reason, "User input needed to proceed");
    }

    #[test]
    fn test_classify_blocker_type() {
        let agent = create_test_agent_struct();

        assert_eq!(
            agent.classify_blocker_type("Please make a decision"),
            BlockerType::DecisionNeeded
        );
        assert_eq!(
            agent.classify_blocker_type("I need an API key"),
            BlockerType::ExternalDependency
        );
        assert_eq!(
            agent.classify_blocker_type("An error occurred"),
            BlockerType::ErrorRequiresJudgment
        );
        assert_eq!(
            agent.classify_blocker_type("The requirements are unclear"),
            BlockerType::AmbiguousRequirements
        );
        assert_eq!(
            agent.classify_blocker_type("I need some details"),
            BlockerType::InformationNeeded
        );
    }

    #[test]
    fn test_extract_options() {
        let agent = create_test_agent_struct();

        let content = r#"Options:
1. Use approach A
2. Use approach B
3. Skip this step"#;
        let options = agent.extract_options(content);
        assert_eq!(options.len(), 3);
        assert_eq!(options[0], "Use approach A");
        assert_eq!(options[1], "Use approach B");
        assert_eq!(options[2], "Skip this step");

        // Test with parentheses style
        let content = r#"1) First option
2) Second option"#;
        let options = agent.extract_options(content);
        assert_eq!(options.len(), 2);
    }

    #[test]
    fn test_classify_error_as_blocker() {
        let agent = create_test_agent_struct();

        // Configuration error should create a blocker
        let err = AgentError::Configuration("Missing API key".to_string());
        let blocker = agent.classify_error_as_blocker(&err);
        assert!(blocker.is_some());
        assert_eq!(blocker.unwrap().blocker_type, BlockerType::ExternalDependency);

        // Max iterations should create a blocker
        let err = AgentError::MaxIterationsExceeded(10);
        let blocker = agent.classify_error_as_blocker(&err);
        assert!(blocker.is_some());

        // Tool not found error should create a blocker
        let err = AgentError::ToolExecution {
            tool_name: "test".to_string(),
            message: "file not found".to_string(),
        };
        let blocker = agent.classify_error_as_blocker(&err);
        assert!(blocker.is_some());

        // Generic model invocation error should not create a blocker (recoverable)
        let err = AgentError::ModelInvocation("temporary failure".to_string());
        let blocker = agent.classify_error_as_blocker(&err);
        assert!(blocker.is_none());
    }

    #[test]
    fn test_completion_driver_accessors() {
        let mut agent = create_test_agent_struct();

        // Initially no driver
        assert!(agent.completion_driver().is_none());

        // Set a driver
        let driver = CompletionDriver::new();
        agent.set_completion_driver(driver);
        assert!(agent.completion_driver().is_some());

        // Clear the driver
        agent.clear_completion_driver();
        assert!(agent.completion_driver().is_none());
    }

    /// Helper to create a UserAgent struct for testing helper methods.
    /// This avoids needing a real API key.
    fn create_test_agent_struct() -> UserAgent {
        use std::sync::Arc;

        UserAgent {
            id: "test-user-agent".to_string(),
            config: UserAgent::default_config(),
            memory: Arc::new(MockMemoryStore::new()),
            embedder: EmbeddingGenerator::from_env(),
            tools: UserAgent::default_tools(),
            client: OpenRouterClient::new("fake-key-for-testing"),
            context: AgentContext::new(),
            completion_driver: None,
        }
    }
}
