//! OpenRouter API client for chat completions with tool calling.
//!
//! This module provides a client for the OpenRouter API, supporting:
//! - Chat completions with multiple message roles
//! - Tool/function calling
//! - Streaming (future)

use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::config::ModelConfig;
use crate::context::{Message, MessageRole};
use crate::error::{AgentError, Result};
use crate::tool::{ToolCall, ToolDefinition};

/// Environment variable for OpenRouter API key.
pub const OPENROUTER_API_KEY_ENV: &str = "OPENROUTER_API_KEY";

/// OpenRouter chat completions endpoint.
const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// OpenRouter API client for chat completions.
#[derive(Clone)]
pub struct OpenRouterClient {
    client: reqwest::Client,
    api_key: String,
}

impl OpenRouterClient {
    /// Create a new client with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
        }
    }

    /// Create a client from environment variables.
    ///
    /// Uses `OPENROUTER_API_KEY` environment variable.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var(OPENROUTER_API_KEY_ENV).map_err(|_| {
            AgentError::Configuration(format!(
                "Missing {} environment variable",
                OPENROUTER_API_KEY_ENV
            ))
        })?;
        Ok(Self::new(api_key))
    }

    /// Send a chat completion request.
    pub async fn chat(
        &self,
        config: &ModelConfig,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<ChatTool>>,
    ) -> Result<ChatResponse> {
        let request = ChatRequest {
            model: config.model.clone(),
            messages,
            tools,
            max_tokens: Some(config.max_tokens),
            temperature: Some(config.temperature),
        };

        trace!("Sending chat request: {:?}", request);

        let response = self
            .client
            .post(OPENROUTER_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/ezykeys/ai-commander")
            .header("X-Title", "AI Commander")
            .json(&request)
            .send()
            .await
            .map_err(|e| AgentError::ModelInvocation(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(AgentError::ModelInvocation(format!(
                "OpenRouter API error {}: {}",
                status, text
            )));
        }

        let response: ChatResponse = response
            .json()
            .await
            .map_err(|e| AgentError::ResponseParse(format!("Failed to parse response: {}", e)))?;

        debug!(
            "Chat response received: {} tokens used",
            response.usage.as_ref().map_or(0, |u| u.total_tokens)
        );

        Ok(response)
    }
}

/// Chat completion request.
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    /// Model identifier.
    pub model: String,

    /// Conversation messages.
    pub messages: Vec<ChatMessage>,

    /// Available tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatTool>>,

    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Temperature for generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// A message in the chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role of the message sender.
    pub role: String,

    /// Text content of the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Tool calls made by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCall>>,

    /// Tool call ID for tool result messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message with tool calls.
    pub fn assistant_with_tools(content: Option<String>, tool_calls: Vec<ChatToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    /// Convert from internal Message type.
    pub fn from_message(msg: &Message) -> Self {
        match msg.role {
            MessageRole::System => Self::system(&msg.content),
            MessageRole::User => Self::user(&msg.content),
            MessageRole::Assistant => {
                if let Some(ref tool_calls) = msg.tool_calls {
                    let calls: Vec<ChatToolCall> =
                        tool_calls.iter().map(ChatToolCall::from_tool_call).collect();
                    Self::assistant_with_tools(Some(msg.content.clone()), calls)
                } else {
                    Self::assistant(&msg.content)
                }
            }
            MessageRole::Tool => {
                if let Some(ref result) = msg.tool_result {
                    Self::tool(&result.tool_call_id, &result.content)
                } else {
                    Self::tool("unknown", &msg.content)
                }
            }
        }
    }
}

/// Tool call in a chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolCall {
    /// Unique identifier for this tool call.
    pub id: String,

    /// Type of the tool call (always "function").
    #[serde(rename = "type")]
    pub call_type: String,

    /// Function details.
    pub function: ChatToolFunction,
}

impl ChatToolCall {
    /// Convert from internal ToolCall type.
    pub fn from_tool_call(call: &ToolCall) -> Self {
        Self {
            id: call.id.clone(),
            call_type: "function".to_string(),
            function: ChatToolFunction {
                name: call.name.clone(),
                arguments: serde_json::to_string(&call.arguments).unwrap_or_default(),
            },
        }
    }

    /// Convert to internal ToolCall type.
    pub fn to_tool_call(&self) -> Result<ToolCall> {
        let arguments: serde_json::Value =
            serde_json::from_str(&self.function.arguments).map_err(|e| {
                AgentError::ResponseParse(format!("Invalid tool arguments JSON: {}", e))
            })?;

        Ok(ToolCall::with_id(
            &self.id,
            &self.function.name,
            arguments,
        ))
    }
}

/// Function details in a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolFunction {
    /// Name of the function to call.
    pub name: String,

    /// JSON-encoded arguments.
    pub arguments: String,
}

/// Tool definition for the API.
#[derive(Debug, Clone, Serialize)]
pub struct ChatTool {
    /// Type of the tool (always "function").
    #[serde(rename = "type")]
    pub tool_type: String,

    /// Function definition.
    pub function: ChatToolDefinition,
}

impl ChatTool {
    /// Create from internal ToolDefinition.
    pub fn from_definition(def: &ToolDefinition) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: ChatToolDefinition {
                name: def.name.clone(),
                description: def.description.clone(),
                parameters: def.parameters.clone(),
            },
        }
    }
}

/// Function definition in a tool.
#[derive(Debug, Clone, Serialize)]
pub struct ChatToolDefinition {
    /// Name of the function.
    pub name: String,

    /// Description of what the function does.
    pub description: String,

    /// JSON Schema for parameters.
    pub parameters: serde_json::Value,
}

/// Chat completion response.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    /// Unique identifier for this completion.
    pub id: String,

    /// Completion choices.
    pub choices: Vec<ChatChoice>,

    /// Token usage information.
    pub usage: Option<ChatUsage>,
}

impl ChatResponse {
    /// Get the first choice's message.
    pub fn message(&self) -> Option<&ResponseMessage> {
        self.choices.first().map(|c| &c.message)
    }

    /// Check if the response has tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.choices
            .first()
            .is_some_and(|c| c.message.tool_calls.is_some())
    }

    /// Get tool calls from the response.
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.choices
            .first()
            .and_then(|c| c.message.tool_calls.as_ref())
            .map_or(Vec::new(), |calls| {
                calls
                    .iter()
                    .filter_map(|c| c.to_tool_call().ok())
                    .collect()
            })
    }
}

/// A choice in the completion response.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    /// Index of this choice.
    pub index: u32,

    /// The message for this choice.
    pub message: ResponseMessage,

    /// Finish reason (stop, tool_calls, length, etc.).
    pub finish_reason: Option<String>,
}

/// Message in a completion response.
#[derive(Debug, Clone, Deserialize)]
pub struct ResponseMessage {
    /// Role (always "assistant" for responses).
    pub role: String,

    /// Text content of the response.
    pub content: Option<String>,

    /// Tool calls the model wants to make.
    pub tool_calls: Option<Vec<ChatToolCall>>,
}

/// Token usage information.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatUsage {
    /// Tokens in the prompt.
    pub prompt_tokens: u32,

    /// Tokens in the completion.
    pub completion_tokens: u32,

    /// Total tokens used.
    pub total_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_chat_message_constructors() {
        let system = ChatMessage::system("You are helpful.");
        assert_eq!(system.role, "system");
        assert_eq!(system.content, Some("You are helpful.".to_string()));

        let user = ChatMessage::user("Hello");
        assert_eq!(user.role, "user");

        let assistant = ChatMessage::assistant("Hi there!");
        assert_eq!(assistant.role, "assistant");

        let tool = ChatMessage::tool("call-123", "result");
        assert_eq!(tool.role, "tool");
        assert_eq!(tool.tool_call_id, Some("call-123".to_string()));
    }

    #[test]
    fn test_chat_tool_call_conversion() {
        let tool_call = ToolCall::with_id("call-1", "test_tool", json!({"arg": "value"}));

        let chat_call = ChatToolCall::from_tool_call(&tool_call);
        assert_eq!(chat_call.id, "call-1");
        assert_eq!(chat_call.call_type, "function");
        assert_eq!(chat_call.function.name, "test_tool");

        let converted = chat_call.to_tool_call().unwrap();
        assert_eq!(converted.id, tool_call.id);
        assert_eq!(converted.name, tool_call.name);
        assert_eq!(converted.arguments, tool_call.arguments);
    }

    #[test]
    fn test_chat_tool_from_definition() {
        let def = ToolDefinition::new(
            "search",
            "Search for information",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        );

        let chat_tool = ChatTool::from_definition(&def);
        assert_eq!(chat_tool.tool_type, "function");
        assert_eq!(chat_tool.function.name, "search");
        assert_eq!(chat_tool.function.description, "Search for information");
    }

    #[test]
    fn test_request_serialization() {
        let request = ChatRequest {
            model: "anthropic/claude-opus-4".to_string(),
            messages: vec![
                ChatMessage::system("You are helpful."),
                ChatMessage::user("Hello"),
            ],
            tools: None,
            max_tokens: Some(4096),
            temperature: Some(0.7),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("claude-opus-4"));
        assert!(json.contains("You are helpful."));
    }

    #[test]
    fn test_response_deserialization() {
        let json = r#"{
            "id": "gen-123",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "gen-123");
        assert_eq!(
            response.message().unwrap().content,
            Some("Hello! How can I help?".to_string())
        );
        assert!(!response.has_tool_calls());
    }

    #[test]
    fn test_response_with_tool_calls() {
        let json = r#"{
            "id": "gen-456",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "search_memories",
                            "arguments": "{\"query\": \"test\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": null
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(response.has_tool_calls());

        let tool_calls = response.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "search_memories");
    }
}
