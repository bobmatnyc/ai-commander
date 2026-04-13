//! Response summarization using Ollama (primary) and OpenRouter (fallback).
//!
//! Provides functionality to summarize long Claude Code responses into
//! concise, conversational summaries suitable for mobile/compact displays.
//!
//! # Provider strategy
//!
//! 1. **Ollama** (local, free, fast) — tried first whenever the server is reachable.
//! 2. **OpenRouter** — used as fallback when Ollama is unavailable or fails.
//!    The API key is read from `OPENROUTER_API_KEY`; if that variable is unset a
//!    hardcoded fallback key is used so the feature works out of the box.

use thiserror::Error;
use tracing::{info, warn};

use crate::ollama::OllamaClient;
use crate::output_filter::clean_response;

/// Hardcoded OpenRouter fallback key used when `OPENROUTER_API_KEY` is not set.
/// Reading from the env var takes precedence.
const OPENROUTER_FALLBACK_KEY: &str =
    "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";

/// Default limits for fallback truncation when summarization is unavailable.
const FALLBACK_MAX_LINES: usize = 10;
const FALLBACK_MAX_CHARS: usize = 500;

/// Truncate text for fallback when summarization is unavailable.
///
/// Prevents full AI responses from leaking to Telegram when OpenRouter API key
/// is missing or fails. Returns a truncated preview with indication of remaining content.
fn fallback_truncate(text: &str, max_lines: usize, max_chars: usize) -> String {
    let cleaned = clean_response(text);
    let total_lines = cleaned.lines().count();
    let lines: Vec<&str> = cleaned.lines().take(max_lines).collect();
    let preview = lines.join("\n");

    if preview.len() > max_chars {
        let truncated = &preview[..max_chars];
        // Find last complete word/line boundary
        let boundary = truncated.rfind(|c: char| c.is_whitespace()).unwrap_or(max_chars);
        let safe_truncated = &truncated[..boundary];
        let remaining_chars = cleaned.len() - safe_truncated.len();
        format!("{}...\n\n_({} more characters)_", safe_truncated, remaining_chars)
    } else if total_lines > max_lines {
        let remaining_lines = total_lines - max_lines;
        format!("{}...\n\n_({} more lines)_", preview, remaining_lines)
    } else {
        preview
    }
}

/// Default model to use for summarization.
pub const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4";

/// Default tier-2 (cheap/fast) model for mid-confidence summaries.
pub const TIER2_MODEL: &str = "anthropic/claude-haiku-3.5";

/// Get the configured tier-2 model, or default.
pub fn get_tier2_model() -> String {
    std::env::var("SUMMARIZER_TIER2_MODEL").unwrap_or_else(|_| TIER2_MODEL.to_string())
}

/// Get the confidence threshold for tier-1 structured summaries.
pub fn get_confidence_threshold() -> f32 {
    std::env::var("SUMMARIZER_CONFIDENCE_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.7)
}

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

/// System prompt for incremental summaries (briefer than final summaries).
const INCREMENTAL_SYSTEM_PROMPT: &str = r#"You are providing a brief progress summary for Commander.
Your job is to summarize what has been done SO FAR in the current output stream.

Rules:
- Be VERY concise (2-3 sentences maximum)
- Focus on key findings, progress, and notable patterns
- Say "Found X..." or "Analyzed Y..." or "Completed Z..."
- Skip details - just highlight what's happening
- Use natural language, no bullet points
- Never mention "Claude Code" or the underlying tool"#;

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

/// Check if summarization is available.
///
/// Returns `true` when an OpenRouter API key is configured (either via
/// `OPENROUTER_API_KEY` or the built-in fallback key). Ollama availability
/// cannot be checked synchronously; use [`OllamaClient::is_available`] for that.
pub fn is_available() -> bool {
    // The hardcoded fallback key means OpenRouter is always available as a last
    // resort, so this is effectively always true — kept for API compatibility.
    true
}

/// Get the OpenRouter API key to use.
///
/// Reads `OPENROUTER_API_KEY` from the environment; if unset, returns the
/// hardcoded fallback key. Never returns `None`.
pub fn get_api_key() -> Option<String> {
    let key = std::env::var("OPENROUTER_API_KEY")
        .unwrap_or_else(|_| OPENROUTER_FALLBACK_KEY.to_string());
    Some(key)
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

/// Tiered summarization: tries structured extraction first, then Ollama, then OpenRouter.
/// Returns (summary_text, tier_used) where tier is 1, 2, 3, or 4.
///
/// Pipeline:
/// - Tier 1: Structured extraction (free, instant) — confidence >= threshold
/// - Tier 2: Ollama local inference (free, private) — when server is reachable
/// - Tier 3: Cheap OpenRouter model (Haiku) with pre-digested context — confidence >= 0.4
/// - Tier 4: Full OpenRouter model (Sonnet) with full input — last resort
pub async fn summarize_tiered(query: &str, raw_response: &str) -> (String, u8) {
    use crate::structured_summarizer;

    let lines: Vec<String> = raw_response.lines().map(|l| l.to_string()).collect();
    let extracted = structured_summarizer::extract(&lines);
    let confidence = extracted.confidence();
    let threshold = get_confidence_threshold();

    // Tier 1: Structured extraction (free, instant)
    if confidence >= threshold {
        let summary = extracted.to_summary();
        if !summary.is_empty() {
            info!(confidence = %confidence, tier = 1, "Summarized via structured extraction");
            return (summary, 1);
        }
    }

    // Tier 2: Ollama local inference (primary LLM provider — free, private, fast)
    let ollama = OllamaClient::new();
    if ollama.is_available().await {
        info!(tier = 2, model = "gemma3:4b", "Summarizing via Ollama local inference");
        let user_prompt = format!(
            "User asked: {}\n\nRaw response:\n{}\n\nProvide a conversational summary:",
            query, raw_response
        );
        match ollama.summarize(&user_prompt, 5).await {
            Ok(summary) => return (summary, 2),
            Err(e) => {
                warn!(error = %e, "Ollama summarization failed, falling back to OpenRouter");
            }
        }
    } else {
        info!("Ollama not available, skipping to OpenRouter fallback");
    }

    // Tier 3: Cheap OpenRouter model with pre-digested context
    if confidence >= 0.4 {
        let context = extracted.to_context();
        let key_lines_text = extracted.key_lines.join("\n");
        let enhanced_input = format!(
            "Structured facts:\n{}\n\nRemaining output:\n{}",
            context, key_lines_text
        );

        // get_api_key() always returns Some (fallback key), so unwrap is safe.
        let api_key = get_api_key().expect("get_api_key always returns Some");
        let tier2_model = get_tier2_model();
        info!(confidence = %confidence, tier = 3, model = %tier2_model, "Summarizing via OpenRouter tier-2 model");

        match summarize_async(query, &enhanced_input, &api_key, &tier2_model).await {
            Ok(summary) => return (summary, 3),
            Err(e) => {
                warn!(error = %e, "OpenRouter tier-2 summarization failed, trying full model");
            }
        }
    }

    // Tier 4: Full OpenRouter model with full input (last resort)
    let api_key = get_api_key().expect("get_api_key always returns Some");
    let model = get_model();
    info!(confidence = %confidence, tier = 4, model = %model, "Summarizing via OpenRouter full model");

    match summarize_async(query, raw_response, &api_key, &model).await {
        Ok(summary) => (summary, 4),
        Err(e) => {
            warn!(error = %e, "OpenRouter full-model summarization failed, using fallback truncation");
            (fallback_truncate(raw_response, FALLBACK_MAX_LINES, FALLBACK_MAX_CHARS), 4)
        }
    }
}

/// Tiered incremental summarization for progress updates.
/// Uses structured extraction for most cases, only calls LLM for complex output.
pub async fn summarize_incremental_tiered(content: &str, line_count: usize) -> Result<String, SummarizerError> {
    use crate::structured_summarizer;

    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let extracted = structured_summarizer::extract(&lines);
    let confidence = extracted.confidence();

    // For incremental/progressive summaries, use a moderate threshold
    // since these are ephemeral status updates — structured is usually good enough
    if confidence >= 0.5 {
        let summary = extracted.to_summary();
        if !summary.is_empty() {
            info!(confidence = %confidence, line_count = line_count, "Incremental summary via structured extraction");
            return Ok(format!("📊 Incremental Summary ({} lines):\n{}", line_count, summary));
        }
    }

    // Fall back to existing incremental summarization (uses the configured model)
    info!(confidence = %confidence, line_count = line_count, "Incremental summary via LLM");
    summarize_incremental(content, line_count).await
}

/// Summarize a response with automatic fallback to truncated response.
///
/// This is a convenience function that handles API key lookup and fallback.
/// If summarization fails or no API key is set, returns a truncated preview
/// to prevent full AI responses from leaking to clients.
///
/// Now delegates to `summarize_tiered` for 3-tier pipeline support.
pub async fn summarize_with_fallback(query: &str, raw_response: &str) -> String {
    let (summary, _tier) = summarize_tiered(query, raw_response).await;
    summary
}

/// Summarize a response synchronously with automatic fallback.
///
/// Blocking version of `summarize_with_fallback`. Uses OpenRouter directly
/// (Ollama requires async; use `summarize_with_fallback` for the full provider
/// chain including local Ollama inference).
pub fn summarize_blocking_with_fallback(query: &str, raw_response: &str) -> String {
    // get_api_key() always returns Some (hardcoded fallback key), so unwrap is safe.
    let api_key = get_api_key().expect("get_api_key always returns Some");
    let model = get_model();

    match summarize_blocking(query, raw_response, &api_key, &model) {
        Ok(summary) => summary,
        Err(_) => fallback_truncate(raw_response, FALLBACK_MAX_LINES, FALLBACK_MAX_CHARS),
    }
}

/// Generate an incremental summary of output collected so far.
///
/// This is briefer than a full summary and focuses on progress/findings.
/// Used for sending periodic updates during long-running operations.
///
/// Provider order: Ollama (local) → OpenRouter (fallback).
///
/// # Arguments
/// * `content`    - The content collected so far.
/// * `line_count` - Number of lines collected (for the summary header).
///
/// # Returns
/// A brief summary prefixed with "📊 Incremental Summary (N lines):".
pub async fn summarize_incremental(content: &str, line_count: usize) -> Result<String, SummarizerError> {
    let user_prompt = format!(
        "Output collected so far ({} lines):\n{}\n\nProvide a brief summary of progress and key findings:",
        line_count, content
    );

    // Try Ollama first (local, free)
    let ollama = OllamaClient::new();
    if ollama.is_available().await {
        match ollama.chat(INCREMENTAL_SYSTEM_PROMPT, &user_prompt).await {
            Ok(summary) => {
                return Ok(format!("📊 Incremental Summary ({} lines):\n{}", line_count, summary));
            }
            Err(e) => {
                warn!(error = %e, "Ollama incremental summary failed, falling back to OpenRouter");
            }
        }
    }

    // Fall back to OpenRouter
    let api_key = get_api_key().expect("get_api_key always returns Some");
    let model = get_model();

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": INCREMENTAL_SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt}
        ],
        "max_tokens": 150
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

    let summary = json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| SummarizerError::ParseError("No content in response".to_string()))?;

    Ok(format!("📊 Incremental Summary ({} lines):\n{}", line_count, summary))
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

    // Try Ollama first (local, free, fast) via blocking HTTP
    match interpret_via_ollama(&user_prompt) {
        Some(result) => return Some(result),
        None => {
            info!("Ollama unavailable for interpret_screen_context, falling back to OpenRouter");
        }
    }

    // Fall back to OpenRouter
    let api_key = match get_api_key() {
        Some(key) => key,
        None => {
            warn!("No OpenRouter API key available for interpret_screen_context");
            return None;
        }
    };
    let model = get_model();

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": SCREEN_INTERPRET_PROMPT},
            {"role": "user", "content": user_prompt}
        ],
        "max_tokens": 100
    });

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to build HTTP client for screen interpretation: {}", e);
            return None;
        }
    };

    let response = match client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            warn!("OpenRouter request failed for screen interpretation: {}", e);
            return None;
        }
    };

    let json: serde_json::Value = match response.json() {
        Ok(j) => j,
        Err(e) => {
            warn!("Failed to parse OpenRouter response for screen interpretation: {}", e);
            return None;
        }
    };

    let result = json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if result.is_none() {
        warn!("OpenRouter returned empty content for screen interpretation");
    }

    result
}

/// Try to interpret screen context via local Ollama (blocking HTTP call).
fn interpret_via_ollama(user_prompt: &str) -> Option<String> {
    let ollama_url = "http://localhost:11434";

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;

    // Quick availability check
    if client
        .get(&format!("{}/api/tags", ollama_url))
        .send()
        .ok()
        .map(|r| r.status().is_success())
        != Some(true)
    {
        return None;
    }

    // Use a small model for fast interpretation
    let body = serde_json::json!({
        "model": "qwen2.5-coder:7b-instruct",
        "messages": [
            {"role": "system", "content": SCREEN_INTERPRET_PROMPT},
            {"role": "user", "content": user_prompt}
        ],
        "stream": false
    });

    let response = client
        .post(&format!("{}/api/chat", ollama_url))
        .timeout(std::time::Duration::from_secs(15))
        .json(&body)
        .send()
        .ok()?;

    if !response.status().is_success() {
        warn!("Ollama chat request failed with status {}", response.status());
        return None;
    }

    let json: serde_json::Value = response.json().ok()?;
    json["message"]["content"]
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
    fn test_get_api_key_uses_fallback_when_env_unset() {
        std::env::remove_var("OPENROUTER_API_KEY");
        // get_api_key should always return Some now (hardcoded fallback key).
        let key = get_api_key();
        assert!(key.is_some(), "expected Some with hardcoded fallback key");
        assert!(!key.unwrap().is_empty());
    }

    #[test]
    fn test_is_available_always_true() {
        // With the hardcoded fallback key, summarization is always available.
        assert!(is_available());
    }

    #[test]
    fn test_fallback_truncate() {
        // Short content - no truncation
        let short = "hello world";
        let result = fallback_truncate(short, 10, 500);
        assert_eq!(result, "hello world");

        // More lines than limit
        let many_lines = (1..=15).map(|i| format!("line{}", i)).collect::<Vec<_>>().join("\n");
        let result = fallback_truncate(&many_lines, 10, 500);
        assert!(result.contains("line1"));
        assert!(result.contains("line10"));
        assert!(!result.contains("line11"));
        assert!(result.contains("more lines)_"));

        // More chars than limit
        let long_line = "x".repeat(600);
        let result = fallback_truncate(&long_line, 10, 500);
        assert!(result.len() < 600);
        assert!(result.contains("more characters)_"));
    }

    #[test]
    fn test_interpret_screen_context_does_not_panic() {
        // interpret_screen_context now always has an API key (hardcoded fallback).
        // It may succeed or fail (network dependent), but must not panic.
        std::env::remove_var("OPENROUTER_API_KEY");
        let _result = interpret_screen_context("some screen content", true);
        // Result may be Some or None depending on network — both are acceptable.
    }
}
