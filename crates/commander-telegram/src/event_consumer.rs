//! Background event-stream consumer for event-driven adapter sessions.
//!
//! This module provides [`consume_runtime_events`], a reusable helper that
//! consumes a [`commander_adapters::RuntimeEvent`] stream (from e.g. the
//! mpm-sdk adapter) and routes output to Telegram via live message editing
//! and final message dispatch.
//!
//! The pattern mirrors `handlers::spawn_agent_with_streaming`, but consumes a
//! `Stream<RuntimeEvent>` rather than an `mpsc<AgentEvent>` channel. Note that
//! `mpm_sdk::AgentEvent` and `commander_adapters::RuntimeEvent` are parallel
//! enums today — they are intentionally kept separate for Phase 2.

use commander_adapters::{EventStream, RuntimeEvent};
use commander_core::{is_summarization_available, summarize_incremental_tiered, summarize_with_fallback};
use futures::StreamExt;
use teloxide::prelude::*;
use teloxide::types::{ChatId, MessageId, ReplyParameters, ThreadId};
use tracing::{debug, warn};

/// Debounced cadence for live status-message edits (matches streaming UX
/// in `handlers::spawn_agent_with_streaming`).
const EDIT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
/// Live preview truncation cap (sub-4096 to leave room for framing).
const PREVIEW_MAX_CHARS: usize = 3500;
/// Final message truncation cap.
const FINAL_MAX_CHARS: usize = 4000;

/// Character threshold for triggering final summarization on completion.
/// Responses longer than this are summarized before sending to Telegram.
const SUMMARIZE_THRESHOLD: usize = 2000;

/// Character delta between progressive summary snapshots during streaming.
const PROGRESSIVE_SUMMARY_CHARS: usize = 500;

/// Consume a `RuntimeEvent` stream, editing a Telegram status message with
/// streaming progress and dispatching the final reply on completion.
///
/// This function owns the entire response lifecycle for a single turn:
/// - Text chunks accumulate and update the status message at most every 2s
/// - Progressive summaries replace raw preview when content grows large
/// - `Complete` deletes the status message and sends the final reply
///   (optionally summarized via the 3-tier NLP+inference pipeline)
/// - `Error` replaces the status message with an error notice
///
/// # Arguments
///
/// * `bot` - Telegram Bot handle (cheaply cloneable)
/// * `chat_id` - Target chat
/// * `status_msg_id` - Pre-sent status message to edit/delete during streaming
/// * `reply_to` - Optional message id to reply to (the user's original message)
/// * `thread_id` - Optional forum topic thread id (for group-mode topic sessions)
/// * `stream` - Event stream from the event-driven adapter
pub async fn consume_runtime_events(
    bot: Bot,
    chat_id: ChatId,
    status_msg_id: MessageId,
    reply_to: Option<MessageId>,
    thread_id: Option<ThreadId>,
    mut stream: EventStream,
) {
    let mut accumulated = String::new();
    let mut last_edit = std::time::Instant::now();
    let mut chars_since_last_summary: usize = 0;
    let summarization_available = is_summarization_available();

    while let Some(event) = stream.next().await {
        match event {
            RuntimeEvent::TextChunk(chunk) => {
                let chunk_len = chunk.len();
                accumulated.push_str(&chunk);
                chars_since_last_summary += chunk_len;

                if last_edit.elapsed() >= EDIT_INTERVAL && !accumulated.is_empty() {
                    // Try progressive summarization when enough new content has arrived.
                    let preview = if summarization_available
                        && chars_since_last_summary >= PROGRESSIVE_SUMMARY_CHARS
                        && accumulated.len() > PROGRESSIVE_SUMMARY_CHARS
                    {
                        let line_count = accumulated.lines().count();
                        match summarize_incremental_tiered(&accumulated, line_count).await {
                            Ok(summary) => {
                                chars_since_last_summary = 0;
                                truncate_for_telegram(&summary, PREVIEW_MAX_CHARS)
                            }
                            Err(e) => {
                                debug!(
                                    chat_id = %chat_id.0,
                                    error = %e,
                                    "Progressive summary failed, falling back to raw preview"
                                );
                                truncate_for_telegram(&accumulated, PREVIEW_MAX_CHARS)
                            }
                        }
                    } else {
                        truncate_for_telegram(&accumulated, PREVIEW_MAX_CHARS)
                    };

                    let _ = bot
                        .edit_message_text(
                            chat_id,
                            status_msg_id,
                            format!("🤔 thinking...\n\n{}", preview),
                        )
                        .await;
                    last_edit = std::time::Instant::now();
                }
            }
            RuntimeEvent::ToolUse { name } => {
                debug!(chat_id = %chat_id.0, tool = %name, "Event-driven session using tool");
            }
            RuntimeEvent::Complete { summary } => {
                // Prefer the explicit summary if present, otherwise fall back to
                // the accumulated text chunks.
                let final_text = summary.unwrap_or_else(|| accumulated.clone());

                // Summarize long responses via the 3-tier NLP+inference pipeline
                // to stay within Telegram's message limits and improve readability.
                let display = if summarization_available
                    && final_text.len() > SUMMARIZE_THRESHOLD
                {
                    let summarized = summarize_with_fallback("", &final_text).await;
                    if summarized.is_empty() || summarized.len() >= final_text.len() {
                        // Summarization returned nothing useful; fall back to truncation.
                        warn!(
                            chat_id = %chat_id.0,
                            text_len = final_text.len(),
                            "Final summarization did not reduce output, using truncation"
                        );
                        truncate_for_telegram(&final_text, FINAL_MAX_CHARS)
                    } else {
                        debug!(
                            chat_id = %chat_id.0,
                            original_len = final_text.len(),
                            summary_len = summarized.len(),
                            "Summarized event-driven response"
                        );
                        truncate_for_telegram(&summarized, FINAL_MAX_CHARS)
                    }
                } else {
                    truncate_for_telegram(&final_text, FINAL_MAX_CHARS)
                };

                // Delete the status message first (best-effort).
                let _ = bot.delete_message(chat_id, status_msg_id).await;

                let mut send = bot.send_message(chat_id, display);
                if let Some(reply) = reply_to {
                    send = send.reply_parameters(ReplyParameters::new(reply));
                }
                if let Some(tid) = thread_id {
                    send = send.message_thread_id(tid);
                }
                let _ = send.await;
                return;
            }
            RuntimeEvent::Error(e) => {
                let _ = bot
                    .edit_message_text(
                        chat_id,
                        status_msg_id,
                        format!("❌ Error: {}", truncate_for_telegram(&e, FINAL_MAX_CHARS)),
                    )
                    .await;
                return;
            }
        }
    }

    // Stream closed without Complete or Error — flush whatever we have.
    if !accumulated.is_empty() {
        let _ = bot.delete_message(chat_id, status_msg_id).await;
        let display = truncate_for_telegram(&accumulated, FINAL_MAX_CHARS);
        let mut send = bot.send_message(chat_id, display);
        if let Some(reply) = reply_to {
            send = send.reply_parameters(ReplyParameters::new(reply));
        }
        if let Some(tid) = thread_id {
            send = send.message_thread_id(tid);
        }
        let _ = send.await;
    } else {
        let _ = bot
            .edit_message_text(chat_id, status_msg_id, "(no output)")
            .await;
    }
}

/// Truncate a string to fit within Telegram's message length limit.
fn truncate_for_telegram(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{}...\n[truncated]", truncated)
    }
}
