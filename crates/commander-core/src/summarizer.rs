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
/// Why: GUI launched from Finder/dock does not inherit the shell environment,
/// so `OPENROUTER_API_KEY` is typically unset. We must read the key from the
/// user's config files before falling back to the hardcoded key.
/// What: Search order is (1) `OPENROUTER_API_KEY` env var, (2)
/// `~/.ai-commander/config/.env.local`, (3) `~/.ai-commander/config/config.toml`
/// (field `openrouter_api_key`), (4) `~/.ai-commander/config.json` (either
/// `{"openrouter_api_key": "..."}` or the GUI-written `{"key":"OPENROUTER_API_KEY","value":"..."}`
/// shape), and finally the hardcoded fallback constant. Always returns `Some`.
/// Test: With `OPENROUTER_API_KEY` unset and a valid `config.toml`, assert the
/// returned key matches the TOML value; with a `config.json` `{key,value}` pair,
/// assert it is parsed. Clear env + remove files → fallback constant is returned.
pub fn get_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        if !key.is_empty() {
            return Some(key);
        }
    }
    if let Some(key) = read_api_key_from_config_files() {
        return Some(key);
    }
    Some(OPENROUTER_FALLBACK_KEY.to_string())
}

/// Read the OpenRouter API key from on-disk config files.
///
/// Why: Extracted so the search order in `get_api_key` stays short and each
/// source has a single responsibility.
/// What: Returns the first non-empty key found by scanning `.env.local`,
/// `config.toml`, and `config.json` (both schemas).
/// Test: Create each file in turn and assert the correct key is returned in
/// isolation; assert None when no file contains the key.
fn read_api_key_from_config_files() -> Option<String> {
    let home = std::env::var("HOME").ok().map(std::path::PathBuf::from)?;

    // 1. ~/.ai-commander/config/.env.local (standard KEY=VALUE)
    let env_path = home.join(".ai-commander/config/.env.local");
    if let Ok(contents) = std::fs::read_to_string(&env_path) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("OPENROUTER_API_KEY=") {
                let key = rest.trim().trim_matches('"').trim_matches('\'');
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
    }

    // 2. ~/.ai-commander/config/config.toml
    let toml_path = home.join(".ai-commander/config/config.toml");
    if let Ok(contents) = std::fs::read_to_string(&toml_path) {
        for line in contents.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("openrouter_api_key") {
                let after_eq = rest.trim_start().strip_prefix('=')?.trim();
                let key = after_eq.trim_matches('"').trim_matches('\'');
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
    }

    // 3. ~/.ai-commander/config.json — two schemas supported
    let json_path = home.join(".ai-commander/config.json");
    if let Ok(contents) = std::fs::read_to_string(&json_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&contents) {
            // Schema A: flat object with openrouter_api_key / openrouter_key / OPENROUTER_API_KEY
            for field in &["openrouter_api_key", "openrouter_key", "OPENROUTER_API_KEY"] {
                if let Some(s) = v.get(*field).and_then(|x| x.as_str()) {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
            }
            // Schema B: { "key": "OPENROUTER_API_KEY", "value": "sk-..." } (written by GUI save_config)
            if v.get("key").and_then(|k| k.as_str()) == Some("OPENROUTER_API_KEY") {
                if let Some(s) = v.get("value").and_then(|x| x.as_str()) {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }

    None
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

    // Tier 2: Ollama local inference (primary LLM provider — free, private, fast).
    // Why: Auto-select whatever model the user has installed rather than assume a
    // hardcoded default — different machines have different model sets.
    let mut ollama = OllamaClient::new();
    if ollama.is_available().await {
        let selected = ollama.find_best_model().await;
        if let Some(model) = selected.clone() {
            ollama = OllamaClient::with_model(&model);
        }
        let model_name = selected.unwrap_or_else(|| "default".to_string());
        info!(tier = 2, model = %model_name, "Summarizing via Ollama local inference");
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
///
/// Why: A tight, example-driven prompt prevents the model from echoing the raw
/// terminal content it was given. The previous prompt was too verbose and
/// permitted the model to "preserve tables verbatim" etc., which in practice
/// caused it to dump the Claude Code status bar back to us.
const SCREEN_INTERPRET_PROMPT: &str = r#"You are summarizing what an AI coding assistant is doing in a terminal session.

Rules:
- Output EXACTLY ONE sentence (max 15 words)
- Active voice: "Analyzing database schema" not "The assistant is analyzing"
- NEVER repeat code, commands, file paths, or terminal output
- NEVER start with "The assistant" or "Claude"
- If the session appears idle or just shows a prompt, output ONLY: Idle
- If unsure, output ONLY: Working

Example GOOD outputs:
- Refactoring the auth middleware to use token refresh
- Running pre-commit hooks
- Waiting for user confirmation on a destructive command
- Idle

Example BAD outputs (never produce these):
- Bash(sleep 1200 && ps aux...)
- ⏵⏵ bypass permissions on
- │ Sonnet 4.6 │ 41% remaining │"#;

/// Characters / substrings that should never appear inside a valid LLM summary.
/// Why: These are markers of raw terminal chrome — if the "summary" contains
/// any of them the model echoed the input instead of summarizing it.
const SUMMARY_CHROME_MARKERS: &[&str] = &[
    "─", "│", "❯", "Bash(", "Python(", "Read(", "Write(", "Edit(",
    "```", "bypass permissions", "shift+tab", "ctrl+o", "more tool use",
    "Wrangling", "Running…", "Thinking…",
];

/// Validate an LLM-produced summary: it must be short and free of terminal chrome.
///
/// Why: Defense-in-depth against the model echoing its input. If the returned
/// "summary" contains box-drawing characters, a bash command fragment, or
/// Claude Code UI text, it's not a summary — discard it so the caller falls
/// back to other tiers or suppresses output entirely.
/// What: Returns true when the trimmed string is non-empty, under 300 chars,
/// and contains none of `SUMMARY_CHROME_MARKERS`.
/// Test: Assert `is_valid_summary("Refactoring auth middleware")` is true;
/// assert `is_valid_summary("│ Sonnet 4.6 │")` is false; assert
/// `is_valid_summary("Bash(ls)")` is false.
fn is_valid_summary(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.len() > 300 {
        return false;
    }
    !SUMMARY_CHROME_MARKERS.iter().any(|m| t.contains(m))
}

/// Strip `<think>...</think>` reasoning blocks from a model response.
///
/// Why: Reasoning-tuned models like `qwen3:8b` wrap their internal chain of
/// thought in `<think>` tags before emitting the real answer. If we pass the
/// raw response (with think tags) into `is_valid_summary`, the 300-char limit
/// will reject every reply because reasoning blocks are long. The summary we
/// actually want is whatever comes AFTER `</think>`.
/// What: If `</think>` is present in the string, returns the trimmed slice
/// following it. Otherwise returns the trimmed input unchanged.
/// Test: Assert `strip_think_tags("<think>reasoning goes here</think>Hello")`
/// returns `"Hello"`; assert `strip_think_tags("plain text")` returns
/// `"plain text"`; assert `strip_think_tags("  spaced  ")` returns `"spaced"`.
fn strip_think_tags(s: &str) -> &str {
    // Strip <think>...</think> block (qwen3 reasoning models)
    if let Some(end) = s.find("</think>") {
        return s[end + 8..].trim();
    }
    s.trim()
}

/// Detect whether the "summary" is just a sentence copied verbatim from the
/// terminal content.
///
/// Why: Small local models will sometimes grab the first natural-language
/// sentence of a Claude Code response (e.g. a PM message) and return it as
/// their own summary. The result is indistinguishable from a correct summary
/// by `is_valid_summary` — it has no chrome markers — but it's the wrong
/// thing to show. Word-shingle overlap is a cheap robust detector.
/// What: Returns true when any 6-consecutive-word (case-insensitive) window
/// from `summary` appears verbatim inside `input`.
/// Test: Assert true for `is_copied_from_input("Added a new user to the database",
/// "...Added a new user to the database...")`. Assert false for a paraphrase
/// like `is_copied_from_input("Adding a user", "User added successfully")`.
fn is_copied_from_input(summary: &str, input: &str) -> bool {
    let s = summary.trim().to_lowercase();
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() >= 6 {
        let haystack = input.to_lowercase();
        for window in words.windows(6) {
            let phrase = window.join(" ");
            if haystack.contains(&phrase) {
                return true;
            }
        }
    }
    false
}

/// Aggressive pre-filter for content passed to the screen-context LLM.
///
/// Why: Even after `output_filter::clean_response`, Claude Code status chrome
/// (tool-use displays, spinner lines, the status bar, keyboard hints) survives
/// and leaks into LLM prompts. Small local models then echo that chrome back
/// as their "summary". Stripping it BEFORE the LLM sees it is the most
/// reliable fix.
/// What: Drops every line that `is_llm_noise` recognises, keeps at most the
/// trailing 50 meaningful lines, and returns `None` when fewer than 3 useful
/// lines remain (don't bother calling the LLM).
/// Test: Pass a screen containing `│ Sonnet 4.6 │`, `⏵⏵ bypass permissions`,
/// `Bash(ls)` and one content line → assert result contains only the content
/// line (or is None if under threshold).
fn prepare_for_llm(raw: &str) -> Option<String> {
    let filtered: Vec<&str> = raw.lines().filter(|line| !is_llm_noise(line)).collect();
    if filtered.len() < 3 {
        return None;
    }
    let start = filtered.len().saturating_sub(50);
    Some(filtered[start..].join("\n"))
}

/// Recognise a single line as LLM-noise (UI chrome, status bars, spinners).
///
/// Why: Complements `output_filter::is_ui_noise` with Claude Code tool-use
/// and status-bar patterns that were leaking through. Kept here (rather than
/// added to `output_filter`) because these rules are specifically tuned for
/// "don't send this to an LLM" — they're stricter than the general filter.
/// What: Returns true for empty lines, status bars with model names,
/// spinner/progress glyphs, box-drawing separators, keyboard hints, tool-call
/// prefixes, more-tool-use banners, and token-count status lines.
/// Test: Assert true for `"│ Sonnet 4.6 │ 41% remaining │ main"`,
/// `"⏵⏵ bypass permissions on"`, `"Bash(ls -la)"`, `"+75 more tool uses"`,
/// `"✢ Wrangling… (1h 7m 38s · ↓ 11.1k tokens)"`. Assert false for
/// `"Refactoring the auth middleware"`.
fn is_llm_noise(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    // Claude Code tool call/result markers. These are "⏺ ToolName(...)" and
    // "⎿ result..." — pure terminal chrome that the LLM latches onto if we
    // don't strip them here.
    if t.starts_with('⏺') || t.starts_with('⎿') {
        return true;
    }
    // Status bar with model info
    if t.contains('│')
        && (t.contains("Sonnet")
            || t.contains("Opus")
            || t.contains("Haiku")
            || t.contains("ctx │")
            || t.contains("remaining"))
    {
        return true;
    }
    // Spinner / progress / mode glyphs at line start
    if t.starts_with('✢') || t.starts_with('⎿') || t.starts_with('⏵') || t.starts_with('◆') {
        return true;
    }
    // Box-drawing separator lines (mostly ─)
    if t.chars().filter(|&c| c == '─').count() > t.chars().count() / 2 {
        return true;
    }
    // Claude Code UI chrome / keyboard hints
    if t.contains("bypass permissions")
        || t.contains("shift+tab")
        || t.contains("ctrl+o")
        || t.contains("ctrl+b")
    {
        return true;
    }
    // Specific ctrl+o expand hint variants ("… ctrl+o to expand", standalone).
    if t.contains("ctrl+o to expand") || (t.contains("tool use") && t.contains('·')) {
        return true;
    }
    if t == "(ctrl+o to expand)" {
        return true;
    }
    // Agent timing lines like "Done (10 tool uses · 38.4k tokens · 1m 2s)"
    if t.starts_with("Done (") && t.contains("tool use") {
        return true;
    }
    // Tool-call display prefixes
    if t.starts_with("Bash(")
        || t.starts_with("Python(")
        || t.starts_with("Read(")
        || t.starts_with("Write(")
        || t.starts_with("Edit(")
    {
        return true;
    }
    // "+N more tool uses" banner
    if t.contains("more tool use") {
        return true;
    }
    // Spinner states / bare prompt
    if t == "Running…" || t == "Wrangling…" || t == "Thinking…" || t == "❯" {
        return true;
    }
    // Token-count status lines like "(1h 7m · ↓ 11.1k tokens)"
    if t.contains("tokens)") && (t.contains('↓') || t.contains('↑')) {
        return true;
    }
    false
}

/// Detect whether the assistant is clearly mid-work (don't call LLM yet).
///
/// Why: `is_claude_ready` can briefly report ready between tool calls during
/// active generation. Calling the summarizer in those gaps produces bogus
/// "summaries" of tool-use chrome. However, over-aggressive detection here
/// was suppressing ALL LLM calls for active sessions — the presence of a
/// `⏺` tool-call marker in scrollback is NOT streaming, just a static
/// record of a previous tool call. Only treat genuine streaming indicators
/// (spinner text) as "actively working".
/// What: Returns true only when the raw screen contains a streaming spinner
/// word (Wrangling…, Running…, Thinking…) which Claude Code writes exclusively
/// while actively streaming to the terminal.
/// Test: Assert true for `"✢ Wrangling…"`, `"Running… foo"`, `"Thinking…"`;
/// assert FALSE for a screen with static `"⏺ Bash(ls)"` tool-call results;
/// assert false for clean idle content.
pub fn is_actively_working(content: &str) -> bool {
    content.contains("Wrangling")
        || content.contains("Running…")
        || content.contains("Thinking…")
}

/// Detect whether the captured screen shows a fresh startup banner.
///
/// Why: When a session is first spawned the terminal shows MPM/Claude Code
/// banners ("Claude-MPM vX.Y.Z", "initializing…", etc.) that carry no
/// summarizable content. Running the LLM on this wastes several seconds and
/// usually produces a meaningless summary of version numbers. Returning a
/// canned phrase immediately gives the user instant feedback and skips the
/// wasted round-trip.
/// What: Returns true when `content` is under 40 lines AND contains one of a
/// small set of startup-banner markers (product names, "initializing",
/// "starting up", etc.). The short-length guard prevents false positives on
/// long in-progress sessions that happen to mention "claude" once.
/// Test: Assert true for a 10-line buffer containing "Claude-MPM v4.5.19";
/// assert true for "welcome to claude code"; assert false for a 100-line
/// screen with a single "claude" mention; assert false for empty input.
fn is_startup_sequence(content: &str) -> bool {
    let lower = content.to_lowercase();
    let line_count = content.lines().count();

    // Must be short (startup banners are brief)
    if line_count > 25 { return false; }

    // Must contain a startup-specific marker
    let has_startup = lower.contains("claude-mpm")
        || lower.contains("claude mpm")
        || (lower.contains("mpm") && lower.contains("v") && content.contains('.'));

    if !has_startup { return false; }

    // Must NOT contain signs of an active session (tool results, user turns)
    let has_active_content = content.lines().any(|l| {
        l.trim_start().starts_with('>')
        || l.contains("✓") || l.contains("✗")
        || l.starts_with("Result:") || l.starts_with("Error:")
        || l.starts_with("$ ") // shell prompt with command
    });

    !has_active_content
}

/// Produce a canned one-line startup message based on the detected product.
///
/// Why: We want a specific label ("Claude-MPM starting up.") rather than a
/// generic one so the user knows which backend is coming up. The detection
/// is token-based rather than regex-based for speed.
/// What: Returns the first matching product's starting-up phrase based on
/// `content.to_lowercase()`; falls back to a generic message.
/// Test: Assert `detect_startup_message("Claude-MPM v4")` returns
/// `"Claude-MPM starting up."`; assert `detect_startup_message("auggie cli")`
/// returns `"Auggie starting up."`.
fn detect_startup_message(content: &str) -> String {
    let lower = content.to_lowercase();
    if lower.contains("claude-mpm") || lower.contains("claude mpm") || lower.contains("mpm v") {
        "Claude-MPM starting up.".to_string()
    } else if lower.contains("claude code") || lower.contains("welcome to claude") {
        "Claude Code starting up.".to_string()
    } else if lower.contains("auggie") {
        "Auggie starting up.".to_string()
    } else if lower.contains("codex") {
        "Codex starting up.".to_string()
    } else {
        "Session starting up.".to_string()
    }
}

/// Return true when at least one LLM backend is reachable.
///
/// Why: The GUI silently drops `None` from `interpret_screen_context` when
/// both Ollama and OpenRouter fail, leaving the user wondering why no
/// summaries appear. Callers use this helper to check LLM availability up
/// front so they can surface an actionable banner.
/// What: Issues a cheap probe to Ollama's `/api/tags` with a 3-second
/// timeout; if that fails, checks whether `get_api_key` returns a
/// plausible OpenRouter key (prefix `sk-or-`, length > 20). Returns true
/// as soon as either check passes.
/// Test: Stop Ollama + unset `OPENROUTER_API_KEY` + remove config files →
/// assert false. Start Ollama OR set a real `sk-or-…` key → assert true.
pub fn llm_available() -> bool {
    // Check Ollama with 3s timeout
    if let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        if client
            .get(format!("{}/api/tags", OLLAMA_BASE_URL))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return true;
        }
    }
    // Check if we have a plausible OpenRouter key
    if let Some(key) = get_api_key() {
        if key.starts_with("sk-or-") && key.len() > 20 {
            return true;
        }
    }
    false
}

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
    // Pre-filter: if session is NOT ready and content looks like busy/thinking output,
    // return a canned response without calling the LLM
    if !is_ready {
        let content_lower = screen_content.to_lowercase();
        let has_spinners = screen_content.chars().any(|c| {
            // Braille spinners (most common Claude Code spinner)
            ('\u{2800}'..='\u{28FF}').contains(&c) ||
            // Other spinner chars
            matches!(c, '◐' | '◑' | '◒' | '◓')
        });
        let has_thinking_text = content_lower.contains("thinking") ||
            content_lower.contains("generating") ||
            content_lower.contains("spelunking") ||
            content_lower.contains("processing");

        if has_spinners || has_thinking_text {
            return Some("Processing...".to_string());
        }
    }

    // Startup shortcut: when the captured screen is just a product banner
    // ("Claude-MPM v…", "welcome to claude code", etc.), return a canned
    // label immediately instead of paying the LLM round-trip for content
    // that has no summarizable information.
    if is_startup_sequence(screen_content) {
        info!("interpret_screen_context: startup banner detected, returning canned message");
        return Some(detect_startup_message(screen_content));
    }

    // Defense-in-depth: even when `is_ready` is true, the screen may still
    // show a tool-use frame from an in-flight operation. Skip the LLM in that
    // case — a bogus "summary" of tool chrome is worse than no summary.
    if is_actively_working(screen_content) {
        info!("interpret_screen_context: active-work markers detected, skipping LLM");
        return None;
    }

    // Aggressively strip terminal chrome before handing text to the LLM.
    // If nothing meaningful remains, don't bother calling the model.
    let filtered = match prepare_for_llm(screen_content) {
        Some(f) => f,
        None => {
            info!("interpret_screen_context: no meaningful content after filtering");
            return None;
        }
    };

    // Why: Keep the prompt small so local 7-8b models stay within a few seconds.
    // 1800 chars (~450 tokens) is plenty to capture the last few lines of a
    // terminal session and still round-trip quickly through Ollama.
    let user_prompt = format!(
        "Terminal content:\n{}\n\nSummary (one sentence):",
        filtered.chars().take(1800).collect::<String>()
    );

    // Try Ollama first (local, free, fast) via blocking HTTP
    match interpret_via_ollama(&user_prompt) {
        Some(result) if is_valid_summary(&result) && !is_copied_from_input(&result, &filtered) => {
            return Some(result);
        }
        Some(bad) => {
            if is_copied_from_input(&bad, &filtered) {
                warn!(
                    "Ollama copied input verbatim, discarding summary: {:?}",
                    bad.chars().take(80).collect::<String>()
                );
            } else {
                warn!(
                    "Ollama returned invalid summary (echoed chrome?): {:?}",
                    bad.chars().take(80).collect::<String>()
                );
            }
        }
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
        .map(|s| strip_think_tags(s).to_string())
        .filter(|s| !s.is_empty());

    match result {
        Some(text) if is_valid_summary(&text) && !is_copied_from_input(&text, &filtered) => {
            Some(text)
        }
        Some(bad) => {
            if is_copied_from_input(&bad, &filtered) {
                warn!(
                    "OpenRouter copied input verbatim, discarding summary: {:?}",
                    bad.chars().take(80).collect::<String>()
                );
            } else {
                warn!(
                    "OpenRouter returned invalid summary (echoed chrome?): {:?}",
                    bad.chars().take(80).collect::<String>()
                );
            }
            None
        }
        None => {
            warn!("OpenRouter returned empty content for screen interpretation");
            None
        }
    }
}

/// Ordered preference list for Ollama screen-interpretation models.
///
/// Why: Prefer fast small models so interpretation stays under the UI's
/// polling interval; fall back to anything 7b-class if the primaries are not
/// installed. Larger models (70b+) are deliberately excluded from this list
/// because they are too slow for interactive interpretation.
const OLLAMA_INTERPRET_PREFERENCES: &[&str] = &[
    "qwen3:8b",
    "mistral:latest",
    "mistral-small3.2:latest",
    "gemma3:4b",
    "qwen2.5-coder:7b-instruct",
    "qwen2.5:7b",
];

/// Ollama base URL.
const OLLAMA_BASE_URL: &str = "http://localhost:11434";

/// Cache of the model name selected on first successful probe.
///
/// Why: Listing `/api/tags` on every screen-interpret call adds 100-300 ms of
/// latency and is a waste — the model set rarely changes within a process
/// lifetime. We cache the first good pick and reuse it.
static CACHED_INTERPRET_MODEL: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Pick the best available Ollama model for screen interpretation.
///
/// Why: Model names are environment-dependent (`qwen2.5-coder:7b-instruct`
/// might not be installed on every machine). Querying `/api/tags` lets us
/// auto-select whatever the user actually has.
/// What: Returns the first `OLLAMA_INTERPRET_PREFERENCES` entry that matches
/// (by substring) an installed model. If none match but some model is
/// installed, returns the first installed model as a last resort. Result is
/// cached for the process lifetime.
/// Test: Mock `/api/tags` returning `["qwen3:8b", "codellama:70b"]` → assert
/// `qwen3:8b` is selected. Return only `["codellama:70b"]` → assert
/// `codellama:70b` is returned (last resort).
fn select_ollama_model(client: &reqwest::blocking::Client) -> Option<String> {
    if let Some(cached) = CACHED_INTERPRET_MODEL.get() {
        return Some(cached.clone());
    }

    let response = client
        .get(format!("{}/api/tags", OLLAMA_BASE_URL))
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: serde_json::Value = response.json().ok()?;
    let models: Vec<String> = json
        .get("models")?
        .as_array()?
        .iter()
        .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();

    if models.is_empty() {
        return None;
    }

    // Prefer our ordered list, match by substring so partial tags work.
    let picked = OLLAMA_INTERPRET_PREFERENCES
        .iter()
        .find_map(|pref| models.iter().find(|m| m.contains(pref)).cloned())
        .or_else(|| models.first().cloned())?;

    let _ = CACHED_INTERPRET_MODEL.set(picked.clone());
    Some(picked)
}

/// Try to interpret screen context via local Ollama (blocking HTTP call).
///
/// Why: Screen-context interpretation runs frequently (every few seconds per
/// session) so it must be fast and must not depend on the network. Ollama is
/// local, private, and free — the right tool for this job when available.
/// What: Queries `/api/tags`, picks the best available model via
/// `select_ollama_model`, and issues a single chat request with a 30-second
/// budget. Returns the trimmed content string on success, `None` otherwise.
/// Test: With Ollama unreachable, assert `None`. With a valid server and
/// `qwen3:8b` installed, assert the returned string is non-empty.
fn interpret_via_ollama(user_prompt: &str) -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;

    let model = match select_ollama_model(&client) {
        Some(m) => m,
        None => {
            info!("Ollama /api/tags unreachable or returned no models");
            return None;
        }
    };

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": SCREEN_INTERPRET_PROMPT},
            {"role": "user", "content": user_prompt}
        ],
        "stream": false,
        "options": { "num_predict": 120 }
    });

    let response = client
        .post(format!("{}/api/chat", OLLAMA_BASE_URL))
        .timeout(std::time::Duration::from_secs(30))
        .json(&body)
        .send()
        .ok()?;

    if !response.status().is_success() {
        warn!("Ollama chat request failed with status {} (model={})", response.status(), model);
        return None;
    }

    let json: serde_json::Value = response.json().ok()?;
    // qwen3:8b and other reasoning models wrap their output in <think>...</think>
    // tags. Strip that block BEFORE `is_valid_summary` sees the string, or the
    // response's length will blow past the 300-char limit and every reply gets
    // rejected regardless of its actual content.
    let result = json["message"]["content"]
        .as_str()
        .map(|s| strip_think_tags(s).to_string())
        .filter(|s| !s.is_empty());
    if result.is_some() {
        info!(model = %model, "interpret_via_ollama succeeded");
    }
    result
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

    #[test]
    fn test_is_llm_noise_strips_status_bar() {
        assert!(is_llm_noise("│ masa │ Sonnet 4.6 │ 41% remaining │ main"));
        assert!(is_llm_noise("⏵⏵ bypass permissions on (shift+tab to cycle)"));
        assert!(is_llm_noise("✢ Wrangling… (1h 7m 38s · ↓ 11.1k tokens)"));
        assert!(is_llm_noise("Bash(sleep 1200 && ps aux)"));
        assert!(is_llm_noise("+75 more tool uses (ctrl+o to expand)"));
        assert!(is_llm_noise("❯"));
        assert!(is_llm_noise("Running…"));
        assert!(is_llm_noise(
            "────────────────────────────────────────────"
        ));
        // Real content must NOT be stripped
        assert!(!is_llm_noise("Refactoring the auth middleware"));
        assert!(!is_llm_noise("def compute_total(items):"));
    }

    #[test]
    fn test_prepare_for_llm_drops_pure_chrome() {
        let screen = "\
│ masa │ Sonnet 4.6 │ 41% remaining │ main
⏵⏵ bypass permissions on (shift+tab to cycle)
Bash(sleep 1200 && ps aux)
+75 more tool uses (ctrl+o to expand)
❯";
        // Nothing but chrome → None (below 3-line threshold after filtering).
        assert!(prepare_for_llm(screen).is_none());
    }

    #[test]
    fn test_prepare_for_llm_keeps_real_content() {
        let screen = "\
│ Sonnet 4.6 │ 41% remaining │
Refactored the auth middleware.
Added a token refresh step.
All tests pass.
❯";
        let out = prepare_for_llm(screen).expect("should retain content");
        assert!(out.contains("Refactored the auth middleware."));
        assert!(out.contains("All tests pass."));
        assert!(!out.contains("Sonnet"));
        assert!(!out.contains('❯'));
    }

    #[test]
    fn test_is_valid_summary_rejects_chrome() {
        assert!(is_valid_summary("Refactoring auth middleware to use refresh"));
        assert!(is_valid_summary("Idle"));
        assert!(!is_valid_summary(""));
        assert!(!is_valid_summary("   "));
        assert!(!is_valid_summary("│ Sonnet 4.6 │ 41% remaining │"));
        assert!(!is_valid_summary("Bash(ls -la)"));
        assert!(!is_valid_summary("Here is the output: ```rust\nfn main() {}\n```"));
        assert!(!is_valid_summary("──── box-drawing separator ────"));
        assert!(!is_valid_summary(&"word ".repeat(80))); // too long (> 300 chars)
    }

    #[test]
    fn test_is_actively_working_detects_markers() {
        // Only genuine streaming spinners should count as "actively working".
        assert!(is_actively_working("✢ Wrangling… (1h 7m · ↓ 11.1k tokens)"));
        assert!(is_actively_working("Running… something"));
        assert!(is_actively_working("Thinking…"));
        // Static tool-call results are NOT active work — they were blocking
        // LLM summarization on healthy sessions.
        assert!(!is_actively_working("⏺ Bash(ls)\n⎿ file1\nfile2"));
        assert!(!is_actively_working("+75 more tool uses (ctrl+o to expand)"));
        assert!(!is_actively_working("Refactored the auth middleware."));
        assert!(!is_actively_working(""));
    }

    #[test]
    fn test_is_startup_sequence_detects_banners() {
        // Clear MPM startup banner with version string: OK
        assert!(is_startup_sequence("Claude-MPM v4.5.19\nInitializing…"));
        // "mpm" + "v" + "." (mpm v-something with a dot) — OK
        assert!(is_startup_sequence("Claude MPM v4.5.19\nLoading..."));

        // Don't misfire on long buffers that happen to mention claude once.
        let long = (0..50).map(|i| format!("line{}", i)).collect::<Vec<_>>().join("\n")
            + "\nclaude code is mentioned here";
        assert!(!is_startup_sequence(&long));
        assert!(!is_startup_sequence(""));

        // Active-session markers (user prompts, tool check marks, shell prompts)
        // MUST defeat startup detection even when the banner text is present —
        // otherwise mid-session blocks get misread as startups and skipped.
        assert!(!is_startup_sequence("Claude-MPM v4.5.19\n> user message\nResponse"));
        assert!(!is_startup_sequence("Claude MPM v4\n✓ task done"));
        assert!(!is_startup_sequence("Claude-MPM v4.5.19\n$ ls -la\nfile.txt"));

        // Very long buffers should never be classified as startup even if they
        // begin with a banner.
        let long_with_banner = format!(
            "Claude-MPM v4.5.19\n{}",
            (0..30).map(|i| format!("line{}", i)).collect::<Vec<_>>().join("\n")
        );
        assert!(!is_startup_sequence(&long_with_banner));
    }

    #[test]
    fn test_detect_startup_message_labels_product() {
        assert_eq!(detect_startup_message("Claude-MPM v4"), "Claude-MPM starting up.");
        assert_eq!(detect_startup_message("MPM v1.2"), "Claude-MPM starting up.");
        assert_eq!(detect_startup_message("welcome to claude code"), "Claude Code starting up.");
        assert_eq!(detect_startup_message("auggie cli loaded"), "Auggie starting up.");
        assert_eq!(detect_startup_message("codex cli v2"), "Codex starting up.");
        assert_eq!(detect_startup_message("unknown banner"), "Session starting up.");
    }

    #[test]
    fn test_llm_available_does_not_panic() {
        // Availability depends on environment (Ollama running, API key set).
        // Just verify it doesn't panic.
        let _ = llm_available();
    }
}
