//! Chat client for LLM interaction via OpenRouter.
//!
//! Provides chat functionality when not connected to a project.

use serde::{Deserialize, Serialize};
use std::env;

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4-20250514";

/// A message in the chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
}

/// OpenRouter API request body.
#[derive(Debug, Serialize)]
struct OpenRouterRequest {
    model: String,
    messages: Vec<Message>,
}

/// OpenRouter API response.
#[derive(Debug, Deserialize)]
struct OpenRouterResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

/// Chat client for OpenRouter API.
pub struct ChatClient {
    api_key: Option<String>,
    model: String,
    history: Vec<Message>,
    client: reqwest::Client,
}

impl ChatClient {
    /// Creates a new chat client.
    ///
    /// Reads `OPENROUTER_API_KEY` from environment.
    /// Optionally reads `OPENROUTER_MODEL` for custom model.
    pub fn new() -> Self {
        let api_key = env::var("OPENROUTER_API_KEY").ok();
        let model = env::var("OPENROUTER_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());

        Self {
            api_key,
            model,
            history: Vec::new(),
            client: reqwest::Client::new(),
        }
    }

    /// Returns true if the chat client is available (API key is set).
    pub fn is_available(&self) -> bool {
        self.api_key.is_some()
    }

    /// Clears the conversation history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Sends a message and returns the assistant's response.
    ///
    /// Maintains conversation history for context.
    pub async fn send(&mut self, user_message: &str) -> Result<String, ChatError> {
        let api_key = self.api_key.as_ref().ok_or(ChatError::NoApiKey)?;

        // Add user message to history
        self.history.push(Message::user(user_message));

        // Build request
        let request = OpenRouterRequest {
            model: self.model.clone(),
            messages: self.history.clone(),
        };

        // Send request
        let response = self
            .client
            .post(OPENROUTER_URL)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ChatError::RequestFailed(e.to_string()))?;

        // Check status
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read response".to_string());
            return Err(ChatError::ApiError(status.as_u16(), body));
        }

        // Parse response
        let response: OpenRouterResponse = response
            .json()
            .await
            .map_err(|e| ChatError::ParseError(e.to_string()))?;

        let content = response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        // Add assistant response to history
        self.history.push(Message::assistant(&content));

        Ok(content)
    }
}

impl Default for ChatClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during chat operations.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("OPENROUTER_API_KEY not set")]
    NoApiKey,

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("API error ({0}): {1}")]
    ApiError(u16, String),

    #[error("Failed to parse response: {0}")]
    ParseError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to serialize tests that modify OPENROUTER_API_KEY
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_message_constructors() {
        let user = Message::user("Hello");
        assert_eq!(user.role, "user");
        assert_eq!(user.content, "Hello");

        let assistant = Message::assistant("Hi there");
        assert_eq!(assistant.role, "assistant");
        assert_eq!(assistant.content, "Hi there");

        let system = Message::system("You are helpful");
        assert_eq!(system.role, "system");
        assert_eq!(system.content, "You are helpful");
    }

    #[test]
    fn test_chat_client_no_api_key() {
        let _guard = ENV_MUTEX.lock().unwrap();

        // Clear the env var to test unavailable state
        let original = env::var("OPENROUTER_API_KEY").ok();
        env::remove_var("OPENROUTER_API_KEY");

        let client = ChatClient::new();
        assert!(!client.is_available());

        // Restore original value if it existed
        if let Some(key) = original {
            env::set_var("OPENROUTER_API_KEY", key);
        }
    }

    #[test]
    fn test_chat_client_with_api_key() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let original = env::var("OPENROUTER_API_KEY").ok();
        env::set_var("OPENROUTER_API_KEY", "test-key");

        let client = ChatClient::new();
        assert!(client.is_available());

        // Restore
        if let Some(key) = original {
            env::set_var("OPENROUTER_API_KEY", key);
        } else {
            env::remove_var("OPENROUTER_API_KEY");
        }
    }

    #[test]
    fn test_clear_history() {
        let mut client = ChatClient::new();
        client.history.push(Message::user("Test"));
        assert!(!client.history.is_empty());

        client.clear_history();
        assert!(client.history.is_empty());
    }
}
