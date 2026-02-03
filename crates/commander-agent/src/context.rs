//! Agent context and message types.
//!
//! This module defines the context passed to agents when processing messages,
//! and the message types used for conversation history.

use chrono::{DateTime, Utc};
use commander_memory::Memory;
use serde::{Deserialize, Serialize};

use crate::tool::{ToolCall, ToolResult};

/// Role of a message in the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// System message (instructions/context).
    System,
    /// User message.
    User,
    /// Assistant (agent) message.
    Assistant,
    /// Tool result message.
    Tool,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
            Self::Tool => write!(f, "tool"),
        }
    }
}

/// A single message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role of the message sender.
    pub role: MessageRole,

    /// Text content of the message.
    pub content: String,

    /// Timestamp when the message was created.
    pub timestamp: DateTime<Utc>,

    /// Tool calls made by the assistant (only for Assistant role).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    /// Tool result (only for Tool role).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<ToolResult>,
}

impl Message {
    /// Create a new message with the current timestamp.
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            timestamp: Utc::now(),
            tool_calls: None,
            tool_result: None,
        }
    }

    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(MessageRole::System, content)
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(MessageRole::User, content)
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }

    /// Create an assistant message with tool calls.
    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            timestamp: Utc::now(),
            tool_calls: Some(tool_calls),
            tool_result: None,
        }
    }

    /// Create a tool result message.
    pub fn tool(result: ToolResult) -> Self {
        Self {
            role: MessageRole::Tool,
            content: result.content.clone(),
            timestamp: Utc::now(),
            tool_calls: None,
            tool_result: Some(result),
        }
    }

    /// Check if this message has tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty())
    }
}

/// Context provided to an agent when processing a message.
///
/// Contains all the information an agent needs to generate a response,
/// including recent conversation history, summarized context, and
/// relevant memories from the vector store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    /// Current task or objective being worked on, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task: Option<String>,

    /// Recent messages from the conversation (typically last 5).
    /// These are passed directly to the LLM for immediate context.
    pub recent_messages: Vec<Message>,

    /// Summarized history of older conversation.
    /// Used to maintain context without exceeding token limits.
    pub summarized_history: String,

    /// Relevant memories retrieved from the vector store.
    /// These provide long-term context and learned information.
    pub relevant_memories: Vec<Memory>,
}

impl Default for AgentContext {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentContext {
    /// Create a new empty context.
    pub fn new() -> Self {
        Self {
            current_task: None,
            recent_messages: Vec::new(),
            summarized_history: String::new(),
            relevant_memories: Vec::new(),
        }
    }

    /// Create a context with a current task.
    pub fn with_task(task: impl Into<String>) -> Self {
        Self {
            current_task: Some(task.into()),
            ..Default::default()
        }
    }

    /// Add a message to recent history.
    pub fn add_message(&mut self, message: Message) {
        self.recent_messages.push(message);
    }

    /// Set the current task.
    pub fn set_task(&mut self, task: impl Into<String>) {
        self.current_task = Some(task.into());
    }

    /// Clear the current task.
    pub fn clear_task(&mut self) {
        self.current_task = None;
    }

    /// Add a relevant memory.
    pub fn add_memory(&mut self, memory: Memory) {
        self.relevant_memories.push(memory);
    }

    /// Set the summarized history.
    pub fn set_summarized_history(&mut self, summary: impl Into<String>) {
        self.summarized_history = summary.into();
    }

    /// Trim recent messages to keep only the last N.
    pub fn trim_recent(&mut self, keep: usize) {
        if self.recent_messages.len() > keep {
            self.recent_messages = self.recent_messages.split_off(self.recent_messages.len() - keep);
        }
    }

    /// Get the total token estimate for context.
    /// This is a rough estimate using ~4 characters per token.
    pub fn estimated_tokens(&self) -> usize {
        let message_chars: usize = self
            .recent_messages
            .iter()
            .map(|m| m.content.len())
            .sum();
        let memory_chars: usize = self
            .relevant_memories
            .iter()
            .map(|m| m.content.len())
            .sum();
        let summary_chars = self.summarized_history.len();
        let task_chars = self.current_task.as_ref().map_or(0, |t| t.len());

        (message_chars + memory_chars + summary_chars + task_chars) / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_role_display() {
        assert_eq!(MessageRole::System.to_string(), "system");
        assert_eq!(MessageRole::User.to_string(), "user");
        assert_eq!(MessageRole::Assistant.to_string(), "assistant");
        assert_eq!(MessageRole::Tool.to_string(), "tool");
    }

    #[test]
    fn test_message_constructors() {
        let system = Message::system("You are helpful.");
        assert_eq!(system.role, MessageRole::System);
        assert_eq!(system.content, "You are helpful.");

        let user = Message::user("Hello");
        assert_eq!(user.role, MessageRole::User);

        let assistant = Message::assistant("Hi there!");
        assert_eq!(assistant.role, MessageRole::Assistant);
        assert!(!assistant.has_tool_calls());
    }

    #[test]
    fn test_message_with_tools() {
        use crate::tool::ToolCall;
        use serde_json::json;

        let tool_call = ToolCall::new("read_file", json!({"path": "/tmp/test"}));
        let msg = Message::assistant_with_tools("Let me read that file.", vec![tool_call]);

        assert!(msg.has_tool_calls());
        assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_tool_message() {
        use crate::tool::ToolResult;

        let result = ToolResult::success("call-123", "file contents");
        let msg = Message::tool(result);

        assert_eq!(msg.role, MessageRole::Tool);
        assert!(msg.tool_result.is_some());
        assert_eq!(msg.tool_result.as_ref().unwrap().tool_call_id, "call-123");
    }

    #[test]
    fn test_agent_context_default() {
        let ctx = AgentContext::new();

        assert!(ctx.current_task.is_none());
        assert!(ctx.recent_messages.is_empty());
        assert!(ctx.summarized_history.is_empty());
        assert!(ctx.relevant_memories.is_empty());
    }

    #[test]
    fn test_agent_context_with_task() {
        let ctx = AgentContext::with_task("Implement feature X");

        assert_eq!(ctx.current_task, Some("Implement feature X".into()));
    }

    #[test]
    fn test_agent_context_add_message() {
        let mut ctx = AgentContext::new();
        ctx.add_message(Message::user("Hello"));
        ctx.add_message(Message::assistant("Hi!"));

        assert_eq!(ctx.recent_messages.len(), 2);
        assert_eq!(ctx.recent_messages[0].content, "Hello");
        assert_eq!(ctx.recent_messages[1].content, "Hi!");
    }

    #[test]
    fn test_agent_context_trim() {
        let mut ctx = AgentContext::new();
        for i in 0..10 {
            ctx.add_message(Message::user(format!("Message {}", i)));
        }

        assert_eq!(ctx.recent_messages.len(), 10);

        ctx.trim_recent(5);
        assert_eq!(ctx.recent_messages.len(), 5);
        assert_eq!(ctx.recent_messages[0].content, "Message 5");
        assert_eq!(ctx.recent_messages[4].content, "Message 9");
    }

    #[test]
    fn test_agent_context_token_estimate() {
        let mut ctx = AgentContext::new();
        ctx.add_message(Message::user("This is a test message with some content.")); // ~44 chars
        ctx.set_summarized_history("Some summarized history here."); // ~29 chars
        ctx.set_task("Current task"); // ~12 chars

        // Total ~85 chars, /4 = ~21 tokens
        let estimate = ctx.estimated_tokens();
        assert!(estimate > 15 && estimate < 30);
    }

    #[test]
    fn test_serialization() {
        let msg = Message::user("Hello, world!");
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(msg.role, parsed.role);
        assert_eq!(msg.content, parsed.content);

        let ctx = AgentContext::with_task("Test task");
        let json = serde_json::to_string(&ctx).unwrap();
        let parsed: AgentContext = serde_json::from_str(&json).unwrap();

        assert_eq!(ctx.current_task, parsed.current_task);
    }
}
