//! Agent response types.
//!
//! This module defines the response structure returned by agents after
//! processing a message.

use serde::{Deserialize, Serialize};

use crate::tool::ToolCall;

/// Response from an agent after processing a message.
///
/// Contains the agent's output text, any tool calls it wants to make,
/// optional structured data, and a flag indicating if the conversation
/// should continue (e.g., for tool execution loops).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// Text content of the response.
    pub content: String,

    /// Tool calls the agent wants to execute.
    /// Empty if no tools need to be called.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,

    /// Optional structured output (e.g., parsed JSON result).
    /// Used when the agent needs to return structured data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<serde_json::Value>,

    /// Whether the conversation should continue after this response.
    /// Set to true when tool calls need to be executed and results returned.
    #[serde(default)]
    pub should_continue: bool,
}

impl Default for AgentResponse {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentResponse {
    /// Create a new empty response.
    pub fn new() -> Self {
        Self {
            content: String::new(),
            tool_calls: Vec::new(),
            structured_output: None,
            should_continue: false,
        }
    }

    /// Create a response with text content.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            tool_calls: Vec::new(),
            structured_output: None,
            should_continue: false,
        }
    }

    /// Create a response with tool calls.
    pub fn with_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            content: content.into(),
            tool_calls,
            structured_output: None,
            should_continue: true, // Continue to execute tools
        }
    }

    /// Create a response with structured output.
    pub fn structured(content: impl Into<String>, output: serde_json::Value) -> Self {
        Self {
            content: content.into(),
            tool_calls: Vec::new(),
            structured_output: Some(output),
            should_continue: false,
        }
    }

    /// Add a tool call to the response.
    pub fn add_tool_call(&mut self, call: ToolCall) {
        self.tool_calls.push(call);
        self.should_continue = true;
    }

    /// Set the structured output.
    pub fn set_structured_output(&mut self, output: serde_json::Value) {
        self.structured_output = Some(output);
    }

    /// Set whether the conversation should continue.
    pub fn set_should_continue(&mut self, should_continue: bool) {
        self.should_continue = should_continue;
    }

    /// Check if the response has tool calls.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    /// Check if the response has structured output.
    pub fn has_structured_output(&self) -> bool {
        self.structured_output.is_some()
    }

    /// Get the number of tool calls.
    pub fn tool_call_count(&self) -> usize {
        self.tool_calls.len()
    }
}

impl std::fmt::Display for AgentResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.content)?;
        if !self.tool_calls.is_empty() {
            write!(f, " [+{} tool calls]", self.tool_calls.len())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_response_default() {
        let response = AgentResponse::new();

        assert!(response.content.is_empty());
        assert!(response.tool_calls.is_empty());
        assert!(response.structured_output.is_none());
        assert!(!response.should_continue);
    }

    #[test]
    fn test_response_text() {
        let response = AgentResponse::text("Hello, world!");

        assert_eq!(response.content, "Hello, world!");
        assert!(!response.has_tool_calls());
        assert!(!response.should_continue);
    }

    #[test]
    fn test_response_with_tool_calls() {
        use crate::tool::ToolCall;

        let tool_call = ToolCall::new("read_file", json!({"path": "/tmp/test"}));
        let response =
            AgentResponse::with_tool_calls("Let me read that file.", vec![tool_call]);

        assert_eq!(response.content, "Let me read that file.");
        assert!(response.has_tool_calls());
        assert_eq!(response.tool_call_count(), 1);
        assert!(response.should_continue);
    }

    #[test]
    fn test_response_structured() {
        let output = json!({"status": "success", "count": 42});
        let response = AgentResponse::structured("Analysis complete.", output.clone());

        assert!(response.has_structured_output());
        assert_eq!(response.structured_output, Some(output));
        assert!(!response.should_continue);
    }

    #[test]
    fn test_response_add_tool_call() {
        use crate::tool::ToolCall;

        let mut response = AgentResponse::text("Let me help.");
        assert!(!response.has_tool_calls());
        assert!(!response.should_continue);

        response.add_tool_call(ToolCall::new("search", json!({})));
        assert!(response.has_tool_calls());
        assert!(response.should_continue);
    }

    #[test]
    fn test_response_display() {
        let response = AgentResponse::text("Hello!");
        assert_eq!(response.to_string(), "Hello!");

        use crate::tool::ToolCall;
        let response = AgentResponse::with_tool_calls(
            "Working...",
            vec![
                ToolCall::new("tool1", json!({})),
                ToolCall::new("tool2", json!({})),
            ],
        );
        assert_eq!(response.to_string(), "Working... [+2 tool calls]");
    }

    #[test]
    fn test_serialization() {
        use crate::tool::ToolCall;

        let tool_call = ToolCall::new("test", json!({"key": "value"}));
        let response = AgentResponse::with_tool_calls("Content", vec![tool_call]);

        let json = serde_json::to_string(&response).unwrap();
        let parsed: AgentResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.content, parsed.content);
        assert_eq!(response.tool_calls.len(), parsed.tool_calls.len());
        assert_eq!(response.should_continue, parsed.should_continue);
    }
}
