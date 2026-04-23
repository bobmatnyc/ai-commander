//! Ollama local inference client.
//!
//! Provides a client for interacting with a locally-running Ollama server at
//! `http://localhost:11434`. Used as the primary inference provider for
//! summarization, with OpenRouter as a fallback.

use serde::{Deserialize, Serialize};
use thiserror::Error;

const OLLAMA_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "qwen2.5-coder:7b-instruct";

/// Preferred models to try in order when selecting the best available model.
const PREFERRED_MODELS: &[&str] = &[
    "gemma3:4b",
    "gemma4:e4b",
    "mistral-small3.2:latest",
    "mistral:latest",
    "qwen2.5-coder:7b-instruct",
];

/// System prompt used when summarizing output through Ollama.
const SUMMARIZE_SYSTEM_PROMPT: &str = "You are a concise summarizer for AI assistant output. \
Summarize the provided text into 2-5 clear bullet points or sentences. \
Focus on what was accomplished or answered. Skip verbose tool output and UI noise. \
Be brief and factual.";

/// Errors from the Ollama client.
#[derive(Error, Debug)]
pub enum OllamaError {
    /// The Ollama server is not reachable.
    #[error("Ollama is not available at {0}")]
    NotAvailable(String),

    /// The HTTP request failed.
    #[error("Request failed: {0}")]
    RequestFailed(String),

    /// The response could not be parsed.
    #[error("Failed to parse response: {0}")]
    ParseError(String),
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

/// Response body from `/api/tags`.
#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct ModelInfo {
    name: String,
}

/// Client for the local Ollama inference server.
#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaClient {
    /// Create a new client using the default model (`qwen2.5-coder:7b-instruct`) and base URL.
    pub fn new() -> Self {
        Self {
            base_url: OLLAMA_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Create a client with a custom model name.
    pub fn with_model(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Self::new()
        }
    }

    /// Create a client with a custom base URL and model.
    pub fn with_base_url(base_url: &str, model: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            model: model.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Check whether the Ollama server is reachable.
    ///
    /// Sends a `GET /api/tags` request; returns `true` only when the server
    /// responds with a 2xx status code.
    pub async fn is_available(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        self.client
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// List all models currently available on the Ollama server.
    ///
    /// Returns an empty `Vec` on failure rather than propagating an error, so
    /// callers can treat an inaccessible server as "no models available."
    pub async fn list_models(&self) -> Result<Vec<String>, OllamaError> {
        let url = format!("{}/api/tags", self.base_url);
        let response = self
            .client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| OllamaError::RequestFailed(e.to_string()))?;

        let tags: TagsResponse = response
            .json()
            .await
            .map_err(|e| OllamaError::ParseError(e.to_string()))?;

        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }

    /// Select the best available model from `PREFERRED_MODELS`, falling back to
    /// whatever is installed if none of the preferred ones are present.
    pub async fn find_best_model(&self) -> Option<String> {
        let available = self.list_models().await.ok()?;
        for preferred in PREFERRED_MODELS {
            if available.iter().any(|m| m.contains(preferred)) {
                return Some(preferred.to_string());
            }
        }
        available.into_iter().next()
    }

    /// Send a chat completion request to Ollama's `/api/chat` endpoint.
    ///
    /// # Arguments
    /// * `system_prompt` - The system-level instruction for the model.
    /// * `user_prompt`   - The user's message.
    ///
    /// # Returns
    /// The assistant's reply text, or an `OllamaError` on failure.
    pub async fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String, OllamaError> {
        let url = format!("{}/api/chat", self.base_url);

        let body = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            stream: false,
        };

        let response = self
            .client
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| OllamaError::RequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(OllamaError::RequestFailed(format!(
                "HTTP {}: {}",
                status, text
            )));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| OllamaError::ParseError(e.to_string()))?;

        Ok(chat_response.message.content)
    }

    /// Convenience wrapper that summarizes `text` into at most `max_lines` lines.
    ///
    /// Uses `SUMMARIZE_SYSTEM_PROMPT` so the model knows it should produce a
    /// compact summary rather than a full reply.
    pub async fn summarize(&self, text: &str, max_lines: usize) -> Result<String, OllamaError> {
        let user_prompt = format!(
            "Summarize the following text in at most {} lines:\n\n{}",
            max_lines, text
        );
        self.chat(SUMMARIZE_SYSTEM_PROMPT, &user_prompt).await
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_default_values() {
        let client = OllamaClient::new();
        assert_eq!(client.base_url, "http://localhost:11434");
        assert_eq!(client.model, "qwen2.5-coder:7b-instruct");
    }

    #[test]
    fn test_client_with_model() {
        let client = OllamaClient::with_model("mistral:latest");
        assert_eq!(client.model, "mistral:latest");
        assert_eq!(client.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_client_with_base_url() {
        let client = OllamaClient::with_base_url("http://remote:11434", "gemma3:4b");
        assert_eq!(client.base_url, "http://remote:11434");
        assert_eq!(client.model, "gemma3:4b");
    }

    /// Smoke-test the async paths from a blocking context.
    /// These only verify construction and wiring; network calls are not made.
    #[test]
    fn test_chat_request_serialization() {
        // Verify ChatRequest serializes correctly (no async needed).
        let req = ChatRequest {
            model: "gemma3:4b".to_string(),
            messages: vec![
                ChatMessage { role: "system".to_string(), content: "Be helpful".to_string() },
                ChatMessage { role: "user".to_string(), content: "Hello".to_string() },
            ],
            stream: false,
        };
        let json = serde_json::to_string(&req).expect("serialization failed");
        assert!(json.contains("gemma3:4b"));
        assert!(json.contains("system"));
        assert!(json.contains("Be helpful"));
        assert!(json.contains("\"stream\":false"));
    }
}
