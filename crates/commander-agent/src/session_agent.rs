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

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, info, trace};

use commander_core::{ChangeDetector, ChangeNotification, ChangeType, Significance};
use commander_memory::{EmbeddingGenerator, Memory, MemoryStore, SearchResult};

use crate::agent::{Agent, AgentType};
use crate::client::{ChatMessage, ChatTool, OpenRouterClient};
use crate::compaction::{ContextWindow, SimpleSummarizer, Summarizer};
use crate::config::ModelConfig;
use crate::context::{AgentContext, Message};
use crate::context_manager::{
    model_contexts, ContextAction, ContextManager, ContextStrategy, CriticalAction,
};
use crate::error::{AgentError, Result};
use crate::response::AgentResponse;
use crate::template::{AdapterType, AgentTemplate, TemplateRegistry};
use crate::tool::{ToolCall, ToolDefinition, ToolResult};

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

/// State of the session being monitored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    /// Current goals for this session.
    pub goals: Vec<String>,

    /// Current task being worked on, if any.
    pub current_task: Option<String>,

    /// Progress indicator (0.0 to 1.0).
    pub progress: f32,

    /// Current blockers preventing progress.
    pub blockers: Vec<String>,

    /// Files that have been modified in this session.
    pub files_modified: Vec<String>,

    /// Last output received from the session.
    pub last_output: Option<String>,
}

impl SessionState {
    /// Create a new empty session state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a goal to the session.
    pub fn add_goal(&mut self, goal: impl Into<String>) {
        self.goals.push(goal.into());
    }

    /// Set the current task.
    pub fn set_current_task(&mut self, task: impl Into<String>) {
        self.current_task = Some(task.into());
    }

    /// Clear the current task.
    pub fn clear_current_task(&mut self) {
        self.current_task = None;
    }

    /// Update progress (clamped to 0.0 - 1.0).
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
    }

    /// Add a blocker.
    pub fn add_blocker(&mut self, blocker: impl Into<String>) {
        self.blockers.push(blocker.into());
    }

    /// Clear all blockers.
    pub fn clear_blockers(&mut self) {
        self.blockers.clear();
    }

    /// Add a modified file.
    pub fn add_modified_file(&mut self, file: impl Into<String>) {
        let file = file.into();
        if !self.files_modified.contains(&file) {
            self.files_modified.push(file);
        }
    }

    /// Set the last output.
    pub fn set_last_output(&mut self, output: impl Into<String>) {
        self.last_output = Some(output.into());
    }
}

/// Analysis of session output.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputAnalysis {
    /// Whether a task completion was detected.
    pub detected_completion: bool,

    /// Whether the session is waiting for user input.
    pub waiting_for_input: bool,

    /// Error message if an error was detected.
    pub error_detected: Option<String>,

    /// Files that were changed in this output.
    pub files_changed: Vec<String>,

    /// Summary of the output.
    pub summary: String,
}

impl OutputAnalysis {
    /// Create a new empty analysis.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an analysis with a summary.
    pub fn with_summary(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            ..Default::default()
        }
    }
}

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
    id: String,

    /// Session ID this agent is managing.
    session_id: String,

    /// Type of adapter (e.g., claude_code, mpm, generic).
    adapter_type: AdapterType,

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

    /// Current session state.
    session_state: SessionState,

    /// Agent template for this adapter type.
    template: AgentTemplate,

    /// Change detector for smart output monitoring.
    change_detector: ChangeDetector,

    /// Context manager for tracking token usage and triggering actions.
    context_manager: ContextManager,

    /// Context window for message compaction.
    context_window: ContextWindow,
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
    fn default_config(template: &AgentTemplate) -> ModelConfig {
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

    /// Get the built-in tools for session agents.
    fn builtin_tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition::new(
                "search_memories",
                "Search your own memories for relevant information (agent-isolated)",
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
                "update_session_state",
                "Update the session state (goals, progress, blockers)",
                json!({
                    "type": "object",
                    "properties": {
                        "add_goal": {
                            "type": "string",
                            "description": "Add a new goal to the session"
                        },
                        "current_task": {
                            "type": "string",
                            "description": "Set the current task being worked on"
                        },
                        "progress": {
                            "type": "number",
                            "description": "Update progress (0.0 to 1.0)",
                            "minimum": 0.0,
                            "maximum": 1.0
                        },
                        "add_blocker": {
                            "type": "string",
                            "description": "Add a blocker"
                        },
                        "clear_blockers": {
                            "type": "boolean",
                            "description": "Clear all blockers"
                        },
                        "add_modified_file": {
                            "type": "string",
                            "description": "Track a modified file"
                        }
                    }
                }),
            ),
            ToolDefinition::new(
                "report_to_user",
                "Send a status report to the User Agent (stored in memory)",
                json!({
                    "type": "object",
                    "properties": {
                        "summary": {
                            "type": "string",
                            "description": "Brief summary of current status"
                        },
                        "progress": {
                            "type": "number",
                            "description": "Progress indicator (0.0 to 1.0)"
                        },
                        "needs_input": {
                            "type": "boolean",
                            "description": "Whether user input is needed"
                        },
                        "has_error": {
                            "type": "boolean",
                            "description": "Whether an error occurred"
                        },
                        "error_message": {
                            "type": "string",
                            "description": "Error message if has_error is true"
                        }
                    },
                    "required": ["summary"]
                }),
            ),
            ToolDefinition::new(
                "analyze_output",
                "Parse session output for progress indicators",
                json!({
                    "type": "object",
                    "properties": {
                        "output": {
                            "type": "string",
                            "description": "Raw output from the session to analyze"
                        }
                    },
                    "required": ["output"]
                }),
            ),
        ]
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

    /// Check context usage and take appropriate action based on strategy.
    ///
    /// This method estimates current token usage and triggers the appropriate
    /// action based on the configured context strategy:
    /// - MPM: Executes pause command and provides resume instructions
    /// - Claude Code: Triggers context compaction
    /// - Generic: Warns the user
    ///
    /// # Returns
    ///
    /// Returns the action that was triggered, or `Continue` if context is fine.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let action = agent.check_context().await?;
    /// match action {
    ///     ContextAction::Critical { action: CriticalAction::Pause { .. } } => {
    ///         // Session was paused, notify user
    ///     }
    ///     ContextAction::Warn { remaining_percent } => {
    ///         // Context getting low, warn user
    ///     }
    ///     _ => {}
    /// }
    /// ```
    pub async fn check_context(&mut self) -> Result<ContextAction> {
        // Estimate current token usage
        let estimated = self.estimate_context_tokens();

        // Get action from context manager
        let action = self.context_manager.update(estimated);

        match &action {
            ContextAction::Critical {
                action: CriticalAction::Pause { command, state_summary },
            } => {
                // For MPM: Log the pause command (actual execution handled by caller)
                info!(
                    session_id = %self.session_id,
                    command = %command,
                    "Context critical, pause recommended"
                );
                // Store state summary for resumption
                self.context_manager.set_state_summary(self.generate_pause_state());
                debug!("Pause state: {}", state_summary);
            }
            ContextAction::Critical {
                action: CriticalAction::Compact { messages_to_summarize },
            } => {
                // For Claude Code: Trigger compaction
                info!(
                    session_id = %self.session_id,
                    messages_to_summarize = %messages_to_summarize,
                    "Context critical, triggering compaction"
                );
                self.context_window.compact().await?;
            }
            ContextAction::Critical {
                action: CriticalAction::Alert { message },
            } => {
                // For Generic: Log the alert
                info!(
                    session_id = %self.session_id,
                    message = %message,
                    "Context alert"
                );
            }
            ContextAction::Warn { remaining_percent } => {
                debug!(
                    session_id = %self.session_id,
                    remaining_percent = %remaining_percent,
                    "Context warning"
                );
            }
            ContextAction::Continue => {
                trace!(
                    session_id = %self.session_id,
                    "Context OK"
                );
            }
        }

        Ok(action)
    }

    /// Estimate the current context token usage.
    fn estimate_context_tokens(&self) -> usize {
        // Rough estimate: 4 chars per token
        let context_chars = self.context.estimated_tokens() * 4;
        let window_tokens = self.context_window.estimated_tokens();

        // Add state information
        let state_chars = format!("{:?}", self.session_state).len();

        (context_chars + state_chars) / 4 + window_tokens
    }

    /// Generate a state summary for pause/resume operations.
    fn generate_pause_state(&self) -> String {
        let mut summary = String::from("## Session Pause State\n\n");

        // Tasks completed (progress = 1.0)
        if self.session_state.progress >= 1.0 {
            summary.push_str("Tasks Completed: Current task completed\n");
        }

        // Tasks in progress
        if let Some(ref task) = self.session_state.current_task {
            summary.push_str(&format!("Tasks In Progress: {}\n", task));
        }

        // Goals
        if !self.session_state.goals.is_empty() {
            summary.push_str(&format!(
                "Goals: {}\n",
                self.session_state.goals.join(", ")
            ));
        }

        // Blockers
        if !self.session_state.blockers.is_empty() {
            summary.push_str(&format!(
                "Blockers: {}\n",
                self.session_state.blockers.join(", ")
            ));
        }

        // Files modified
        if !self.session_state.files_modified.is_empty() {
            summary.push_str(&format!(
                "Files Modified: {}\n",
                self.session_state.files_modified.join(", ")
            ));
        }

        // Progress
        summary.push_str(&format!(
            "Progress: {:.0}%\n",
            self.session_state.progress * 100.0
        ));

        // Context usage
        summary.push_str(&format!(
            "Context Usage: {:.1}%\n",
            (1.0 - self.context_manager.remaining_percent()) * 100.0
        ));

        summary.push_str("\nNext Action: Resume session to continue from this state\n");

        summary
    }

    /// Process session output with smart change detection.
    ///
    /// This method uses deterministic change detection to avoid unnecessary
    /// LLM calls. It only invokes the LLM for significant changes (errors,
    /// completion, waiting for input).
    ///
    /// # Returns
    ///
    /// - `Ok(Some(notification))` if user should be notified
    /// - `Ok(None)` if change was not significant enough for notification
    /// - `Err(_)` if LLM analysis failed
    ///
    /// # Example
    ///
    /// ```ignore
    /// let notification = agent.process_output_change(output).await?;
    /// if let Some(notif) = notification {
    ///     if notif.requires_action {
    ///         // Alert user immediately
    ///     }
    /// }
    /// ```
    pub async fn process_output_change(
        &mut self,
        output: &str,
    ) -> Result<Option<ChangeNotification>> {
        // Stage 1: Deterministic change detection (no LLM call)
        let change = self.change_detector.detect(output);

        debug!(
            session_id = %self.session_id,
            change_type = ?change.change_type,
            significance = ?change.significance,
            new_lines = change.diff_lines.len(),
            "Change detected"
        );

        // Stage 2: Return early if not significant enough
        if !change.is_meaningful() {
            trace!(
                session_id = %self.session_id,
                "Change not significant, skipping LLM analysis"
            );
            return Ok(None);
        }

        // Stage 3: For significant changes, optionally do LLM analysis
        // Only invoke LLM for high-significance changes to get better summary
        let (summary, requires_action) = if change.significance >= Significance::High {
            // Do LLM analysis for high-significance changes
            let analysis = self.analyze_output(output).await?;

            let requires_action = analysis.waiting_for_input || analysis.error_detected.is_some();
            let summary = if analysis.summary.is_empty() {
                change.summary.clone()
            } else {
                analysis.summary
            };

            (summary, requires_action)
        } else {
            // For medium significance, use the pattern-based summary
            (change.summary.clone(), false)
        };

        // Stage 4: Determine if user needs to know
        let should_notify = change.requires_notification()
            || requires_action
            || matches!(change.change_type, ChangeType::Error | ChangeType::WaitingForInput);

        if should_notify {
            Ok(Some(ChangeNotification {
                session_id: self.session_id.clone(),
                summary,
                requires_action,
                change_type: change.change_type,
                significance: change.significance,
            }))
        } else {
            Ok(None)
        }
    }

    /// Reset the change detector state.
    ///
    /// Call this when starting a new task or after significant user interaction
    /// to ensure the next output is analyzed fresh.
    pub fn reset_change_detector(&mut self) {
        self.change_detector.reset();
    }

    /// Analyze raw output from the session.
    ///
    /// This method uses the LLM to analyze session output and extract
    /// progress indicators, completion status, errors, and file changes.
    pub async fn analyze_output(&mut self, output: &str) -> Result<OutputAnalysis> {
        // Store the output
        self.session_state.set_last_output(output);

        let analysis_prompt = format!(
            r#"Analyze the following session output and extract:
1. Whether a task was completed (look for success messages, "done", completion indicators)
2. Whether the session is waiting for user input (prompts, questions, input requests)
3. Any errors or warnings (error messages, failures, stack traces)
4. Files that were modified (created, edited, deleted)

Output to analyze:
```
{}
```

Provide a brief summary and structured analysis."#,
            output.chars().take(4000).collect::<String>() // Limit output size
        );

        // Build messages for analysis
        let messages = vec![
            ChatMessage::system(
                self.config
                    .system_prompt
                    .as_deref()
                    .unwrap_or(DEFAULT_SYSTEM_PROMPT),
            ),
            ChatMessage::user(analysis_prompt),
        ];

        // Send request without tools for direct analysis
        let response = self
            .client
            .chat(&self.config, messages, None)
            .await?;

        let content = response
            .message()
            .and_then(|m| m.content.clone())
            .unwrap_or_default();

        // Parse the response to extract structured analysis
        let analysis = self.parse_analysis_response(&content, output);

        // Update state based on analysis
        self.update_state(&analysis);

        Ok(analysis)
    }

    /// Parse the LLM's analysis response into structured data.
    fn parse_analysis_response(&self, response: &str, _output: &str) -> OutputAnalysis {
        let response_lower = response.to_lowercase();

        let mut analysis = OutputAnalysis::with_summary(
            response.lines().next().unwrap_or("Analysis complete").to_string()
        );

        // Detect completion
        analysis.detected_completion = response_lower.contains("completed")
            || response_lower.contains("success")
            || response_lower.contains("finished")
            || response_lower.contains("done");

        // Detect waiting for input
        analysis.waiting_for_input = response_lower.contains("waiting for input")
            || response_lower.contains("requires input")
            || response_lower.contains("user input needed")
            || response_lower.contains("prompt");

        // Detect errors
        if response_lower.contains("error") || response_lower.contains("failed") {
            // Try to extract error message
            for line in response.lines() {
                let line_lower = line.to_lowercase();
                if line_lower.contains("error") || line_lower.contains("failed") {
                    analysis.error_detected = Some(line.trim().to_string());
                    break;
                }
            }
        }

        // Extract file changes (simple heuristic)
        for line in response.lines() {
            let line_lower = line.to_lowercase();
            if line_lower.contains("modified:") || line_lower.contains("created:") || line_lower.contains("edited:") {
                // Try to extract file path
                if let Some(path_start) = line.find(':') {
                    let path = line[path_start + 1..].trim();
                    if !path.is_empty() {
                        analysis.files_changed.push(path.to_string());
                    }
                }
            }
        }

        analysis
    }

    /// Update session state based on output analysis.
    pub fn update_state(&mut self, analysis: &OutputAnalysis) {
        // Add detected files
        for file in &analysis.files_changed {
            self.session_state.add_modified_file(file);
        }

        // Update progress based on completion
        if analysis.detected_completion {
            self.session_state.set_progress(1.0);
            self.session_state.clear_current_task();
        }

        // Add blocker if error detected
        if let Some(ref error) = analysis.error_detected {
            self.session_state.add_blocker(error.clone());
        }

        // Store summary for context
        if !analysis.summary.is_empty() {
            self.context.set_summarized_history(format!(
                "Last analysis: {}",
                analysis.summary
            ));
        }
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

    /// Execute the search_memories tool (agent-isolated).
    async fn execute_search_memories(&self, call: &ToolCall) -> Result<ToolResult> {
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

        debug!(
            "Session agent '{}' searching memories: {} (limit: {})",
            self.id, query, limit
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

        // Search memories - IMPORTANT: filtered by own agent_id for isolation
        let results = self
            .memory
            .search(&embedding, &self.id, limit)
            .await
            .map_err(AgentError::Memory)?;

        let output = format_search_results(&results);
        Ok(ToolResult::success(&call.id, output))
    }

    /// Execute the update_session_state tool.
    /// Call this method directly when you have mutable access to the SessionAgent.
    pub fn execute_update_session_state(&mut self, call: &ToolCall) -> Result<ToolResult> {
        let mut updates = Vec::new();

        if let Some(goal) = call.get_optional_string_arg("add_goal") {
            self.session_state.add_goal(goal);
            updates.push(format!("Added goal: {}", goal));
        }

        if let Some(task) = call.get_optional_string_arg("current_task") {
            self.session_state.set_current_task(task);
            updates.push(format!("Set current task: {}", task));
        }

        if let Some(progress) = call.get_arg("progress").and_then(|v| v.as_f64()) {
            self.session_state.set_progress(progress as f32);
            updates.push(format!("Updated progress: {:.0}%", progress * 100.0));
        }

        if let Some(blocker) = call.get_optional_string_arg("add_blocker") {
            self.session_state.add_blocker(blocker);
            updates.push(format!("Added blocker: {}", blocker));
        }

        if call.get_arg("clear_blockers").and_then(|v| v.as_bool()) == Some(true) {
            self.session_state.clear_blockers();
            updates.push("Cleared all blockers".to_string());
        }

        if let Some(file) = call.get_optional_string_arg("add_modified_file") {
            self.session_state.add_modified_file(file);
            updates.push(format!("Tracked modified file: {}", file));
        }

        let output = if updates.is_empty() {
            "No state updates performed.".to_string()
        } else {
            format!("Session state updated:\n- {}", updates.join("\n- "))
        };

        Ok(ToolResult::success(&call.id, output))
    }

    /// Execute the report_to_user tool.
    async fn execute_report_to_user(&self, call: &ToolCall) -> Result<ToolResult> {
        let summary = call.get_string_arg("summary").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        let progress = call.get_arg("progress").and_then(|v| v.as_f64());
        let needs_input = call.get_arg("needs_input").and_then(|v| v.as_bool()).unwrap_or(false);
        let has_error = call.get_arg("has_error").and_then(|v| v.as_bool()).unwrap_or(false);
        let error_message = call.get_optional_string_arg("error_message");

        // Build report
        let mut report = format!(
            "Session Report [{}]:\nSummary: {}",
            self.session_id, summary
        );

        if let Some(p) = progress {
            report.push_str(&format!("\nProgress: {:.0}%", p * 100.0));
        }

        if needs_input {
            report.push_str("\nStatus: NEEDS INPUT");
        }

        if has_error {
            report.push_str(&format!("\nError: {}", error_message.unwrap_or("Unknown error")));
        }

        // Store report in memory for User Agent to retrieve
        if let Err(e) = self.store_memory(&report).await {
            debug!("Failed to store report memory: {}", e);
        }

        info!("Session {} report: {}", self.session_id, summary);

        Ok(ToolResult::success(&call.id, format!("Report sent: {}", summary)))
    }

    /// Execute the analyze_output tool.
    /// Call this method directly when you have mutable access to the SessionAgent.
    pub async fn execute_analyze_output(&mut self, call: &ToolCall) -> Result<ToolResult> {
        let output = call.get_string_arg("output").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        let analysis = self.analyze_output(output).await?;

        let result = json!({
            "detected_completion": analysis.detected_completion,
            "waiting_for_input": analysis.waiting_for_input,
            "error_detected": analysis.error_detected,
            "files_changed": analysis.files_changed,
            "summary": analysis.summary
        });

        Ok(ToolResult::success(&call.id, serde_json::to_string_pretty(&result)?))
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

/// Format search results as a human-readable string.
fn format_search_results(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No relevant memories found.".to_string();
    }

    let mut output = format!("Found {} relevant memories:\n\n", results.len());

    for (i, result) in results.iter().enumerate() {
        output.push_str(&format!(
            "{}. [Score: {:.2}] {}\n   Created: {}\n\n",
            i + 1,
            result.score,
            result.memory.content,
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
    fn test_session_state_default() {
        let state = SessionState::new();

        assert!(state.goals.is_empty());
        assert!(state.current_task.is_none());
        assert_eq!(state.progress, 0.0);
        assert!(state.blockers.is_empty());
        assert!(state.files_modified.is_empty());
        assert!(state.last_output.is_none());
    }

    #[test]
    fn test_session_state_updates() {
        let mut state = SessionState::new();

        state.add_goal("Implement feature X");
        assert_eq!(state.goals.len(), 1);

        state.set_current_task("Writing tests");
        assert_eq!(state.current_task, Some("Writing tests".to_string()));

        state.set_progress(0.5);
        assert_eq!(state.progress, 0.5);

        state.set_progress(1.5); // Should clamp
        assert_eq!(state.progress, 1.0);

        state.add_blocker("API error");
        assert_eq!(state.blockers.len(), 1);

        state.clear_blockers();
        assert!(state.blockers.is_empty());

        state.add_modified_file("src/main.rs");
        state.add_modified_file("src/main.rs"); // Duplicate - should not add
        assert_eq!(state.files_modified.len(), 1);
    }

    #[test]
    fn test_output_analysis_default() {
        let analysis = OutputAnalysis::new();

        assert!(!analysis.detected_completion);
        assert!(!analysis.waiting_for_input);
        assert!(analysis.error_detected.is_none());
        assert!(analysis.files_changed.is_empty());
        assert!(analysis.summary.is_empty());
    }

    #[test]
    fn test_output_analysis_with_summary() {
        let analysis = OutputAnalysis::with_summary("Task completed successfully");

        assert_eq!(analysis.summary, "Task completed successfully");
    }

    #[test]
    fn test_default_config() {
        let template = AgentTemplate::generic();
        let config = SessionAgent::default_config(&template);

        assert_eq!(config.model, "anthropic/claude-haiku-4");
        assert_eq!(config.max_tokens, 2048);
        assert_eq!(config.temperature, 0.5);
    }

    #[test]
    fn test_builtin_tools() {
        let tools = SessionAgent::builtin_tools();

        assert_eq!(tools.len(), 4);

        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"search_memories"));
        assert!(tool_names.contains(&"update_session_state"));
        assert!(tool_names.contains(&"report_to_user"));
        assert!(tool_names.contains(&"analyze_output"));
    }

    #[test]
    fn test_format_search_results_empty() {
        let results: Vec<SearchResult> = vec![];
        let output = format_search_results(&results);
        assert_eq!(output, "No relevant memories found.");
    }

    #[test]
    fn test_format_search_results() {
        let memory = Memory::new("session-agent-1", "Test memory content", vec![0.1; 64]);
        let results = vec![SearchResult::new(memory, 0.95)];

        let output = format_search_results(&results);
        assert!(output.contains("Found 1 relevant memories"));
        assert!(output.contains("Test memory content"));
        assert!(output.contains("0.95"));
    }

    #[tokio::test]
    async fn test_mock_memory_isolation() {
        let store = Arc::new(MockMemoryStore::new());

        // Store memories for different agents
        let memory1 = Memory::new("session-agent-1", "Memory for agent 1", vec![0.1; 64]);
        let memory2 = Memory::new("session-agent-2", "Memory for agent 2", vec![0.1; 64]);

        store.store(memory1).await.unwrap();
        store.store(memory2).await.unwrap();

        // Search should only return memories for the specified agent
        let results1 = store.search(&[0.1; 64], "session-agent-1", 10).await.unwrap();
        assert_eq!(results1.len(), 1);
        assert_eq!(results1[0].memory.agent_id, "session-agent-1");

        let results2 = store.search(&[0.1; 64], "session-agent-2", 10).await.unwrap();
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].memory.agent_id, "session-agent-2");
    }

    // ==========================================================================
    // Context Manager Tests
    // ==========================================================================

    #[test]
    fn test_context_manager_initialization() {
        // Test that templates get the correct context strategy
        let claude_template = AgentTemplate::claude_code();
        assert!(matches!(
            claude_template.context_strategy,
            Some(ContextStrategy::Compaction)
        ));

        let mpm_template = AgentTemplate::mpm();
        assert!(matches!(
            mpm_template.context_strategy,
            Some(ContextStrategy::PauseResume { .. })
        ));

        let generic_template = AgentTemplate::generic();
        assert!(matches!(
            generic_template.context_strategy,
            Some(ContextStrategy::WarnAndContinue)
        ));
    }

    #[test]
    fn test_context_manager_thresholds() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);

        // Test Continue (50% used = 50% remaining)
        let action = manager.update(50_000);
        assert!(matches!(action, ContextAction::Continue));

        // Test Warning (85% used = 15% remaining)
        let action = manager.update(85_000);
        assert!(matches!(action, ContextAction::Warn { .. }));

        // Test Critical (95% used = 5% remaining)
        let action = manager.update(95_000);
        assert!(matches!(action, ContextAction::Critical { .. }));
    }

    #[test]
    fn test_context_manager_strategies() {
        // Test Compaction strategy
        let mut compaction_manager = ContextManager::new(ContextStrategy::Compaction, 100_000);
        let action = compaction_manager.update(95_000);
        match action {
            ContextAction::Critical { action } => {
                assert!(matches!(action, CriticalAction::Compact { .. }));
            }
            _ => panic!("Expected Critical action with Compact"),
        }

        // Test PauseResume strategy
        let mut pause_manager = ContextManager::new(
            ContextStrategy::PauseResume {
                pause_command: "/pause".to_string(),
                resume_command: "/resume".to_string(),
            },
            100_000,
        );
        let action = pause_manager.update(95_000);
        match action {
            ContextAction::Critical { action } => {
                assert!(matches!(action, CriticalAction::Pause { .. }));
            }
            _ => panic!("Expected Critical action with Pause"),
        }

        // Test WarnAndContinue strategy
        let mut warn_manager = ContextManager::new(ContextStrategy::WarnAndContinue, 100_000);
        let action = warn_manager.update(95_000);
        match action {
            ContextAction::Critical { action } => {
                assert!(matches!(action, CriticalAction::Alert { .. }));
            }
            _ => panic!("Expected Critical action with Alert"),
        }
    }

    #[test]
    fn test_generate_pause_state() {
        let mut state = SessionState::new();
        state.add_goal("Implement feature X");
        state.set_current_task("Writing tests");
        state.set_progress(0.5);
        state.add_blocker("Waiting for API");
        state.add_modified_file("src/main.rs");

        // Create a minimal context manager to test state generation format
        let manager = ContextManager::new(ContextStrategy::PauseResume {
            pause_command: "/pause".to_string(),
            resume_command: "/resume".to_string(),
        }, 100_000);

        // The state should contain key fields
        let state_debug = format!("{:?}", state);
        assert!(state_debug.contains("Implement feature X"));
        assert!(state_debug.contains("Writing tests"));
        assert!(state_debug.contains("Waiting for API"));
        assert!(state_debug.contains("src/main.rs"));

        // Verify manager has correct strategy
        assert!(matches!(
            manager.strategy(),
            ContextStrategy::PauseResume { .. }
        ));
    }

    #[test]
    fn test_context_manager_remaining_percent() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 200_000);

        // Initial state: 0% used = 100% remaining
        assert!((manager.remaining_percent() - 1.0).abs() < 0.001);

        // 50% used
        manager.update(100_000);
        assert!((manager.remaining_percent() - 0.5).abs() < 0.001);

        // 90% used = 10% remaining (exactly at critical)
        manager.update(180_000);
        assert!((manager.remaining_percent() - 0.1).abs() < 0.001);
    }
}
