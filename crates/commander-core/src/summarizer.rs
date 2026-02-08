//! Response summarization using OpenRouter API.
//!
//! Provides functionality to summarize long Claude Code responses into
//! concise, conversational summaries suitable for mobile/compact displays.

use thiserror::Error;
use tracing::warn;

use crate::output_filter::clean_response;

/// Default model to use for summarization.
pub const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4";

/// OpenRouter API endpoint.
const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// System prompt for the summarizer.
const SYSTEM_PROMPT: &str = r#"You are a response summarizer for Commander, an AI orchestration tool.
Your job is to take raw output from Claude Code and summarize it conversationally.

Rules:
- Be concise but informative (2-4 sentences for simple responses, more for complex ones)
- Focus on what was DONE or LEARNED, not the process
- Skip UI noise, file listings, and verbose tool output
- If code was written, summarize what it does
- If a question was answered, give the key answer
- Use natural language, not bullet points unless listing multiple items
- Never say "Claude Code" or mention the underlying tool"#;

/// Errors that can occur during summarization.
#[derive(Error, Debug)]
pub enum SummarizerError {
    /// OpenRouter API key not set in environment.
    #[error("OpenRouter API key not set")]
    NoApiKey,

    /// API request failed.
    #[error("API request failed: {0}")]
    RequestFailed(String),

    /// Failed to parse API response.
    #[error("Failed to parse response: {0}")]
    ParseError(String),
}

/// Check if summarization is available (API key set).
pub fn is_available() -> bool {
    std::env::var("OPENROUTER_API_KEY").is_ok()
}

/// Get the configured OpenRouter API key.
pub fn get_api_key() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY").ok()
}

/// Get the configured model, or default.
pub fn get_model() -> String {
    std::env::var("OPENROUTER_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string())
}

/// Summarize a response asynchronously.
///
/// # Arguments
/// * `query` - The original user query
/// * `raw_response` - The raw response from Claude Code
/// * `api_key` - OpenRouter API key
/// * `model` - Model to use for summarization (e.g., "anthropic/claude-sonnet-4")
///
/// # Returns
/// A summarized version of the response, or the cleaned raw response on failure.
pub async fn summarize_async(
    query: &str,
    raw_response: &str,
    api_key: &str,
    model: &str,
) -> Result<String, SummarizerError> {
    let user_prompt = format!(
        "User asked: {}\n\nRaw response:\n{}\n\nProvide a conversational summary:",
        query, raw_response
    );

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt}
        ],
        "max_tokens": 500
    });

    let client = reqwest::Client::new();
    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| SummarizerError::RequestFailed(e.to_string()))?;

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| SummarizerError::ParseError(e.to_string()))?;

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| SummarizerError::ParseError("No content in response".to_string()))
}

/// Summarize a response synchronously (blocking).
///
/// # Arguments
/// * `query` - The original user query
/// * `raw_response` - The raw response from Claude Code
/// * `api_key` - OpenRouter API key
/// * `model` - Model to use for summarization
///
/// # Returns
/// A summarized version of the response, or the cleaned raw response on failure.
pub fn summarize_blocking(
    query: &str,
    raw_response: &str,
    api_key: &str,
    model: &str,
) -> Result<String, SummarizerError> {
    let user_prompt = format!(
        "User asked: {}\n\nRaw response:\n{}\n\nProvide a conversational summary:",
        query, raw_response
    );

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt}
        ],
        "max_tokens": 500
    });

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .map_err(|e| SummarizerError::RequestFailed(e.to_string()))?;

    let json: serde_json::Value = response
        .json()
        .map_err(|e| SummarizerError::ParseError(e.to_string()))?;

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| SummarizerError::ParseError("No content in response".to_string()))
}

/// Summarize a response with automatic fallback to cleaned response.
///
/// This is a convenience function that handles API key lookup and fallback.
/// If summarization fails or no API key is set, returns the cleaned raw response.
pub async fn summarize_with_fallback(query: &str, raw_response: &str) -> String {
    let Some(api_key) = get_api_key() else {
        return clean_response(raw_response);
    };

    let model = get_model();

    match summarize_async(query, raw_response, &api_key, &model).await {
        Ok(summary) => summary,
        Err(e) => {
            warn!(error = %e, "Summarization failed, using raw response");
            clean_response(raw_response)
        }
    }
}

/// Summarize a response synchronously with automatic fallback.
///
/// Blocking version of `summarize_with_fallback`.
pub fn summarize_blocking_with_fallback(query: &str, raw_response: &str) -> String {
    let Some(api_key) = get_api_key() else {
        return clean_response(raw_response);
    };

    let model = get_model();

    match summarize_blocking(query, raw_response, &api_key, &model) {
        Ok(summary) => summary,
        Err(_) => clean_response(raw_response),
    }
}

/// System prompt for screen context interpretation.
const SCREEN_INTERPRET_PROMPT: &str = r#"You are analyzing a Claude Code session screen.
The session is currently idle/waiting for user input.
Analyze the screen and tell me in ONE sentence what Claude is asking or waiting for.

Rules:
- If Claude asked a question, quote it briefly (truncate if over 50 chars)
- If Claude completed a task, summarize what was done in past tense
- If Claude is showing an error, mention the error briefly
- Be concise - respond with ONLY the interpretation, no preamble
- Start with an appropriate prefix like "Claude is asking:", "Ready after:", "Waiting for:", "Error:"
- Never mention "the screen shows" or similar meta-language"#;

/// Interpret screen context from a Claude Code session.
///
/// Uses LLM to analyze what Claude is asking/waiting for based on screen content.
/// Returns a human-readable interpretation like "Claude is asking: Should I deploy?"
///
/// # Arguments
/// * `screen_content` - The captured screen content (last N lines)
/// * `is_ready` - Whether the session is ready for input (idle)
///
/// # Returns
/// An interpretation string, or None if LLM is unavailable or fails.
pub fn interpret_screen_context(screen_content: &str, is_ready: bool) -> Option<String> {
    let api_key = get_api_key()?;
    let model = get_model();

    let state_hint = if is_ready {
        "The session IS ready for input (showing prompt)."
    } else {
        "The session is NOT ready - Claude is still processing."
    };

    let user_prompt = format!(
        "{}\n\nScreen content:\n```\n{}\n```",
        state_hint,
        // Limit screen content to avoid huge prompts
        screen_content.chars().take(3000).collect::<String>()
    );

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": SCREEN_INTERPRET_PROMPT},
            {"role": "user", "content": user_prompt}
        ],
        "max_tokens": 100
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .ok()?;

    let json: serde_json::Value = response.json().ok()?;

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_available_without_key() {
        // This test depends on environment, so just ensure it doesn't panic
        let _ = is_available();
    }

    #[test]
    fn test_get_model_default() {
        // Clear env var for test
        std::env::remove_var("OPENROUTER_MODEL");
        assert_eq!(get_model(), DEFAULT_MODEL);
    }

    #[test]
    fn test_fallback_without_api_key() {
        std::env::remove_var("OPENROUTER_API_KEY");
        let result = summarize_blocking_with_fallback("test query", "raw response content");
        assert_eq!(result, "raw response content");
    }

    #[test]
    fn test_interpret_screen_context_no_api_key() {
        // Without API key, should return None
        std::env::remove_var("OPENROUTER_API_KEY");
        let result = interpret_screen_context("some screen content", true);
        assert!(result.is_none());
    }
}
