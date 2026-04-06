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
use futures::StreamExt;
use teloxide::prelude::*;
use teloxide::types::{ChatId, MessageId, ReplyParameters, ThreadId};
use tracing::debug;

/// Debounced cadence for live status-message edits (matches streaming UX
/// in `handlers::spawn_agent_with_streaming`).
const EDIT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
/// Live preview truncation cap (sub-4096 to leave room for framing).
const PREVIEW_MAX_CHARS: usize = 3500;
/// Final message truncation cap.
const FINAL_MAX_CHARS: usize = 4000;

/// Consume a `RuntimeEvent` stream, editing a Telegram status message with
/// streaming progress and dispatching the final reply on completion.
///
/// This function owns the entire response lifecycle for a single turn:
/// - Text chunks accumulate and update the status message at most every 2s
/// - `Complete` deletes the status message and sends the final reply
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

    while let Some(event) = stream.next().await {
        match event {
            RuntimeEvent::TextChunk(chunk) => {
                accumulated.push_str(&chunk);
                if last_edit.elapsed() >= EDIT_INTERVAL && !accumulated.is_empty() {
                    let preview = truncate_for_telegram(&accumulated, PREVIEW_MAX_CHARS);
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
                let display = truncate_for_telegram(&final_text, FINAL_MAX_CHARS);

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
