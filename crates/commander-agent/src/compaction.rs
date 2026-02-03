//! Memory auto-compaction system for optimized context windows.
//!
//! This module provides automatic memory compaction so each agent request
//! receives optimized context: current task, recent messages, and summarized history.
//!
//! # Context Window Structure
//!
//! Each request receives approximately:
//! - Current Task (if any): ~500 tokens
//! - Last 5 Prompts/Responses (full): ~4000 tokens
//! - Summarized History: ~2000 tokens
//! - Retrieved Memories (semantic): ~1500 tokens
//!
//! # Example
//!
//! ```ignore
//! use commander_agent::compaction::{ContextWindow, SimpleSummarizer};
//! use std::sync::Arc;
//!
//! let summarizer = Arc::new(SimpleSummarizer);
//! let mut window = ContextWindow::new(5, 8000, summarizer);
//!
//! window.add_message(Message::user("Hello")).await?;
//! window.add_message(Message::assistant("Hi there!")).await?;
//!
//! let context = window.build_context(relevant_memories);
//! ```

use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, trace};

use crate::client::OpenRouterClient;
use crate::config::ModelConfig;
use crate::context::{AgentContext, Message};
use crate::error::{AgentError, Result};
use commander_memory::Memory;

/// Default number of recent messages to keep in full.
pub const DEFAULT_MAX_RECENT: usize = 5;

/// Default token budget for the context window.
pub const DEFAULT_TOKEN_BUDGET: usize = 8000;

/// Approximate characters per token for estimation.
const CHARS_PER_TOKEN: usize = 4;

/// Trait for summarizing messages into compact history.
///
/// Implementations can use LLMs for high-quality summarization
/// or simple truncation for testing/fallback.
#[async_trait]
pub trait Summarizer: Send + Sync {
    /// Summarize a list of messages into a compact string.
    ///
    /// The summary should preserve:
    /// - Key facts and decisions
    /// - Action items and outcomes
    /// - Important context for future interactions
    async fn summarize(&self, messages: &[Message]) -> Result<String>;

    /// Estimate the number of tokens in a text.
    ///
    /// Uses a rough heuristic of ~4 characters per token.
    fn estimate_tokens(&self, text: &str) -> usize {
        text.len() / CHARS_PER_TOKEN
    }
}

/// Context window manager with automatic compaction.
///
/// Maintains recent messages in full while summarizing older history
/// to stay within token budget.
pub struct ContextWindow {
    /// Maximum number of recent messages to keep in full.
    max_recent: usize,

    /// Recent messages (full content).
    recent_messages: VecDeque<Message>,

    /// Summarized older history.
    summarized_history: String,

    /// Current task being worked on.
    current_task: Option<String>,

    /// Token budget for context.
    token_budget: usize,

    /// Summarizer for compacting old messages.
    summarizer: Arc<dyn Summarizer>,

    /// Messages pending compaction (accumulated before summarization).
    pending_compaction: Vec<Message>,

    /// Threshold for triggering compaction (number of pending messages).
    compaction_threshold: usize,
}

impl ContextWindow {
    /// Create a new context window with the given configuration.
    ///
    /// # Arguments
    /// * `max_recent` - Maximum number of recent messages to keep in full
    /// * `token_budget` - Total token budget for the context
    /// * `summarizer` - Summarizer implementation for compacting old messages
    pub fn new(max_recent: usize, token_budget: usize, summarizer: Arc<dyn Summarizer>) -> Self {
        Self {
            max_recent,
            recent_messages: VecDeque::with_capacity(max_recent + 1),
            summarized_history: String::new(),
            current_task: None,
            token_budget,
            summarizer,
            pending_compaction: Vec::new(),
            compaction_threshold: max_recent, // Compact when we have max_recent pending
        }
    }

    /// Create a context window with default settings.
    pub fn with_defaults(summarizer: Arc<dyn Summarizer>) -> Self {
        Self::new(DEFAULT_MAX_RECENT, DEFAULT_TOKEN_BUDGET, summarizer)
    }

    /// Add a new message, compacting if necessary.
    ///
    /// When recent messages exceed `max_recent`, the oldest messages
    /// are moved to pending compaction. When pending messages exceed
    /// the threshold, they are summarized into `summarized_history`.
    pub async fn add_message(&mut self, msg: Message) -> Result<()> {
        trace!(
            "Adding message: role={}, content_len={}",
            msg.role,
            msg.content.len()
        );

        self.recent_messages.push_back(msg);

        // Check if we need to move messages to pending compaction
        while self.recent_messages.len() > self.max_recent {
            if let Some(old_msg) = self.recent_messages.pop_front() {
                self.pending_compaction.push(old_msg);
            }
        }

        // Check if we need to trigger compaction
        if self.pending_compaction.len() >= self.compaction_threshold {
            self.compact().await?;
        }

        Ok(())
    }

    /// Build context for an API request.
    ///
    /// Combines current task, recent messages, summarized history,
    /// and relevant memories into an `AgentContext`.
    pub fn build_context(&self, relevant_memories: Vec<Memory>) -> AgentContext {
        let mut context = AgentContext::new();

        // Set current task
        if let Some(ref task) = self.current_task {
            context.set_task(task.clone());
        }

        // Add recent messages
        for msg in &self.recent_messages {
            context.add_message(msg.clone());
        }

        // Set summarized history
        if !self.summarized_history.is_empty() {
            context.set_summarized_history(&self.summarized_history);
        }

        // Add relevant memories
        for memory in relevant_memories {
            context.add_memory(memory);
        }

        debug!(
            "Built context: task={}, recent={}, history_len={}, memories={}",
            self.current_task.is_some(),
            self.recent_messages.len(),
            self.summarized_history.len(),
            context.relevant_memories.len()
        );

        context
    }

    /// Set the current task.
    pub fn set_task(&mut self, task: Option<String>) {
        self.current_task = task;
    }

    /// Get the current task.
    pub fn current_task(&self) -> Option<&str> {
        self.current_task.as_deref()
    }

    /// Get recent messages.
    pub fn recent_messages(&self) -> &VecDeque<Message> {
        &self.recent_messages
    }

    /// Get summarized history.
    pub fn summarized_history(&self) -> &str {
        &self.summarized_history
    }

    /// Get the number of messages pending compaction.
    pub fn pending_count(&self) -> usize {
        self.pending_compaction.len()
    }

    /// Estimate the current token usage.
    pub fn estimated_tokens(&self) -> usize {
        let recent_tokens: usize = self
            .recent_messages
            .iter()
            .map(|m| self.summarizer.estimate_tokens(&m.content))
            .sum();

        let history_tokens = self.summarizer.estimate_tokens(&self.summarized_history);
        let task_tokens = self
            .current_task
            .as_ref()
            .map_or(0, |t| self.summarizer.estimate_tokens(t));

        recent_tokens + history_tokens + task_tokens
    }

    /// Check if the context is within budget.
    pub fn within_budget(&self) -> bool {
        self.estimated_tokens() <= self.token_budget
    }

    /// Force compaction of pending messages into summary.
    pub async fn compact(&mut self) -> Result<()> {
        if self.pending_compaction.is_empty() {
            return Ok(());
        }

        debug!(
            "Compacting {} messages into summary",
            self.pending_compaction.len()
        );

        // Summarize pending messages
        let new_summary = self.summarizer.summarize(&self.pending_compaction).await?;

        // Merge with existing summary
        if self.summarized_history.is_empty() {
            self.summarized_history = new_summary;
        } else {
            // Prepend existing summary context, then add new summary
            self.summarized_history = format!(
                "{}\n\n[Later in conversation]\n{}",
                self.summarized_history, new_summary
            );
        }

        // Clear pending messages
        self.pending_compaction.clear();

        // Ensure we're within budget after compaction
        self.trim_to_budget().await?;

        Ok(())
    }

    /// Trim the context to fit within budget.
    async fn trim_to_budget(&mut self) -> Result<()> {
        while !self.within_budget() && !self.summarized_history.is_empty() {
            // Progressively truncate the history
            let current_len = self.summarized_history.len();
            let target_len = (current_len * 3) / 4; // Reduce by 25%

            if target_len < 100 {
                // History too small to truncate further
                self.summarized_history.clear();
            } else {
                // Find a good break point (end of sentence or paragraph)
                let truncated = &self.summarized_history[..target_len];
                if let Some(break_pos) = truncated.rfind(". ").or_else(|| truncated.rfind('\n')) {
                    self.summarized_history = self.summarized_history[..break_pos + 1].to_string();
                } else {
                    self.summarized_history = truncated.to_string();
                }
            }

            debug!(
                "Trimmed history to {} chars, {} estimated tokens",
                self.summarized_history.len(),
                self.estimated_tokens()
            );
        }

        Ok(())
    }

    /// Clear all messages and history.
    pub fn clear(&mut self) {
        self.recent_messages.clear();
        self.pending_compaction.clear();
        self.summarized_history.clear();
        self.current_task = None;
    }
}

/// Simple summarizer that truncates messages for testing/fallback.
///
/// This summarizer concatenates messages with truncation, suitable for
/// testing or environments without LLM access.
pub struct SimpleSummarizer;

#[async_trait]
impl Summarizer for SimpleSummarizer {
    async fn summarize(&self, messages: &[Message]) -> Result<String> {
        let summary = messages
            .iter()
            .map(|m| {
                let truncated = if m.content.len() > 100 {
                    format!("{}...", &m.content[..100])
                } else {
                    m.content.clone()
                };
                format!("{}: {}", m.role, truncated)
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(summary)
    }
}

/// LLM-based summarizer using OpenRouter API.
///
/// Uses a fast, cost-effective model (Haiku) for summarization.
pub struct LlmSummarizer {
    client: OpenRouterClient,
    model: String,
}

impl LlmSummarizer {
    /// Create a new LLM summarizer with the default model.
    pub fn new(client: OpenRouterClient) -> Self {
        Self {
            client,
            model: "anthropic/claude-3-5-haiku-20241022".to_string(),
        }
    }

    /// Create a new LLM summarizer with a custom model.
    pub fn with_model(client: OpenRouterClient, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
        }
    }

    /// Build the summarization prompt.
    fn build_prompt(messages: &[Message]) -> String {
        let conversation = messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        format!(
            r#"Summarize this conversation concisely, preserving:
- Key facts and information shared
- Decisions made and their rationale
- Action items and their outcomes
- Important context for future interactions

Be brief but comprehensive. Use bullet points where appropriate.

Conversation:
{}

Summary:"#,
            conversation
        )
    }
}

#[async_trait]
impl Summarizer for LlmSummarizer {
    async fn summarize(&self, messages: &[Message]) -> Result<String> {
        if messages.is_empty() {
            return Ok(String::new());
        }

        let prompt = Self::build_prompt(messages);

        let config = ModelConfig {
            model: self.model.clone(),
            max_tokens: 500,
            temperature: 0.3, // Low temperature for consistent summaries
            ..Default::default()
        };

        use crate::client::ChatMessage;
        let chat_messages = vec![ChatMessage::user(prompt)];

        let response = self.client.chat(&config, chat_messages, None).await?;

        response
            .message()
            .and_then(|m| m.content.clone())
            .ok_or_else(|| AgentError::ResponseParse("No content in summarization response".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MessageRole;

    fn create_test_message(role: MessageRole, content: &str) -> Message {
        Message::new(role, content)
    }

    #[tokio::test]
    async fn test_context_window_creation() {
        let summarizer = Arc::new(SimpleSummarizer);
        let window = ContextWindow::new(5, 8000, summarizer);

        assert_eq!(window.max_recent, 5);
        assert_eq!(window.token_budget, 8000);
        assert!(window.recent_messages.is_empty());
        assert!(window.summarized_history.is_empty());
        assert!(window.current_task.is_none());
    }

    #[tokio::test]
    async fn test_add_messages_within_limit() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(5, 8000, summarizer);

        for i in 0..5 {
            let msg = create_test_message(MessageRole::User, &format!("Message {}", i));
            window.add_message(msg).await.unwrap();
        }

        assert_eq!(window.recent_messages.len(), 5);
        assert!(window.pending_compaction.is_empty());
    }

    #[tokio::test]
    async fn test_messages_move_to_pending() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(3, 8000, summarizer);

        for i in 0..5 {
            let msg = create_test_message(MessageRole::User, &format!("Message {}", i));
            window.add_message(msg).await.unwrap();
        }

        // Should have 3 recent and 2 pending
        assert_eq!(window.recent_messages.len(), 3);
        assert_eq!(window.pending_compaction.len(), 2);

        // Recent should be messages 2, 3, 4
        assert!(window.recent_messages[0].content.contains("Message 2"));
        assert!(window.recent_messages[2].content.contains("Message 4"));
    }

    #[tokio::test]
    async fn test_auto_compaction() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(3, 8000, summarizer);
        window.compaction_threshold = 3;

        // Add 6 messages to trigger compaction
        for i in 0..6 {
            let msg = create_test_message(MessageRole::User, &format!("Message {}", i));
            window.add_message(msg).await.unwrap();
        }

        // Should have compacted messages 0, 1, 2 into history
        assert_eq!(window.recent_messages.len(), 3);
        assert!(!window.summarized_history.is_empty());
        assert!(window.summarized_history.contains("Message 0"));
    }

    #[tokio::test]
    async fn test_build_context() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(5, 8000, summarizer);

        window.set_task(Some("Test task".into()));
        window
            .add_message(create_test_message(MessageRole::User, "Hello"))
            .await
            .unwrap();
        window
            .add_message(create_test_message(MessageRole::Assistant, "Hi there!"))
            .await
            .unwrap();

        let context = window.build_context(vec![]);

        assert_eq!(context.current_task, Some("Test task".into()));
        assert_eq!(context.recent_messages.len(), 2);
    }

    #[tokio::test]
    async fn test_build_context_with_memories() {
        let summarizer = Arc::new(SimpleSummarizer);
        let window = ContextWindow::new(5, 8000, summarizer);

        let memory = Memory::new("test-agent", "Important fact", vec![0.1; 64]);
        let context = window.build_context(vec![memory]);

        assert_eq!(context.relevant_memories.len(), 1);
        assert_eq!(context.relevant_memories[0].content, "Important fact");
    }

    #[tokio::test]
    async fn test_set_and_get_task() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(5, 8000, summarizer);

        assert!(window.current_task().is_none());

        window.set_task(Some("Implement feature".into()));
        assert_eq!(window.current_task(), Some("Implement feature"));

        window.set_task(None);
        assert!(window.current_task().is_none());
    }

    #[tokio::test]
    async fn test_estimated_tokens() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(5, 8000, summarizer);

        // Add a message with 40 characters (~10 tokens)
        let msg = create_test_message(MessageRole::User, "This is a test message with 40 chars!!");
        window.add_message(msg).await.unwrap();

        let tokens = window.estimated_tokens();
        assert!(tokens >= 8 && tokens <= 12); // Rough estimate
    }

    #[tokio::test]
    async fn test_within_budget() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(5, 100, summarizer); // Small budget

        assert!(window.within_budget());

        // Add a large message
        let msg = create_test_message(MessageRole::User, &"x".repeat(1000));
        window.add_message(msg).await.unwrap();

        assert!(!window.within_budget());
    }

    #[tokio::test]
    async fn test_clear() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(5, 8000, summarizer);

        window.set_task(Some("Task".into()));
        window
            .add_message(create_test_message(MessageRole::User, "Hello"))
            .await
            .unwrap();
        window.summarized_history = "Some history".into();

        window.clear();

        assert!(window.recent_messages.is_empty());
        assert!(window.summarized_history.is_empty());
        assert!(window.current_task.is_none());
    }

    #[tokio::test]
    async fn test_simple_summarizer() {
        let summarizer = SimpleSummarizer;

        let messages = vec![
            create_test_message(MessageRole::User, "Hello, how are you?"),
            create_test_message(MessageRole::Assistant, "I'm doing well, thank you!"),
        ];

        let summary = summarizer.summarize(&messages).await.unwrap();

        assert!(summary.contains("user:"));
        assert!(summary.contains("assistant:"));
        assert!(summary.contains("Hello"));
        assert!(summary.contains("well"));
    }

    #[tokio::test]
    async fn test_simple_summarizer_truncation() {
        let summarizer = SimpleSummarizer;
        let long_content = "x".repeat(200);

        let messages = vec![create_test_message(MessageRole::User, &long_content)];

        let summary = summarizer.summarize(&messages).await.unwrap();

        // Should be truncated with "..."
        assert!(summary.contains("..."));
        assert!(summary.len() < 200);
    }

    #[test]
    fn test_estimate_tokens() {
        let summarizer = SimpleSummarizer;

        // 40 characters should be ~10 tokens
        assert_eq!(summarizer.estimate_tokens("This is exactly forty characters long!!"), 9);

        // Empty string should be 0 tokens
        assert_eq!(summarizer.estimate_tokens(""), 0);
    }

    #[tokio::test]
    async fn test_force_compact() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(5, 8000, summarizer);
        window.compaction_threshold = 100; // High threshold to prevent auto-compaction

        // Add messages to pending
        for i in 0..3 {
            window
                .pending_compaction
                .push(create_test_message(MessageRole::User, &format!("Old msg {}", i)));
        }

        assert_eq!(window.pending_count(), 3);
        assert!(window.summarized_history.is_empty());

        window.compact().await.unwrap();

        assert_eq!(window.pending_count(), 0);
        assert!(!window.summarized_history.is_empty());
    }

    #[tokio::test]
    async fn test_merge_summaries() {
        let summarizer = Arc::new(SimpleSummarizer);
        let mut window = ContextWindow::new(2, 8000, summarizer);
        window.compaction_threshold = 2;

        // Add 6 messages to trigger multiple compactions
        for i in 0..6 {
            let msg = create_test_message(MessageRole::User, &format!("Message {}", i));
            window.add_message(msg).await.unwrap();
        }

        // Should have merged summaries
        assert!(window.summarized_history.contains("[Later in conversation]"));
    }
}
