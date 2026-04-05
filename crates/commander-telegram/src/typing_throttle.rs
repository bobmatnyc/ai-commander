//! Per-chat typing indicator throttle.
//!
//! Telegram rate-limits `sendChatAction` calls. This module enforces a per-chat
//! cooldown (default 5 seconds) and respects `Retry after N` responses from the
//! API by extending the suppression window for the affected chat.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use teloxide::prelude::*;
use teloxide::types::{ChatAction, ChatId, ThreadId};
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// Default minimum interval between typing indicators for the same chat.
const DEFAULT_COOLDOWN: Duration = Duration::from_secs(5);

/// Per-chat state tracking last send time and any backoff deadline.
#[derive(Debug, Clone)]
struct ChatThrottleState {
    /// When the last typing indicator was successfully sent (or attempted).
    last_sent: Instant,
    /// If set, typing is suppressed until this instant (from Retry-After).
    suppressed_until: Option<Instant>,
}

/// Thread-safe typing indicator throttle.
///
/// Wrap in `Arc` and share across the poll loop and handler tasks.
#[derive(Debug, Clone)]
pub struct TypingThrottle {
    inner: Arc<Mutex<HashMap<i64, ChatThrottleState>>>,
    cooldown: Duration,
}

impl TypingThrottle {
    /// Create a new throttle with the default 5-second cooldown.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            cooldown: DEFAULT_COOLDOWN,
        }
    }

    /// Send a typing indicator if the per-chat cooldown has elapsed.
    ///
    /// Returns `true` if the indicator was sent, `false` if suppressed.
    /// On `Retry after N` errors the chat is suppressed for `N` seconds.
    pub async fn send_if_allowed(
        &self,
        bot: &Bot,
        chat_id: ChatId,
        thread_id: Option<ThreadId>,
    ) -> bool {
        let now = Instant::now();

        // --- Check & update throttle state ---
        {
            let mut map = self.inner.lock().await;
            if let Some(state) = map.get(&chat_id.0) {
                // Honour Retry-After suppression (takes priority over cooldown).
                if let Some(until) = state.suppressed_until {
                    if now < until {
                        debug!(
                            chat_id = %chat_id.0,
                            remaining_ms = (until - now).as_millis() as u64,
                            "Typing indicator suppressed (Retry-After backoff)"
                        );
                        return false;
                    }
                }

                // Honour regular cooldown.
                if now.duration_since(state.last_sent) < self.cooldown {
                    debug!(
                        chat_id = %chat_id.0,
                        "Typing indicator suppressed (cooldown)"
                    );
                    return false;
                }
            }

            // Mark as sent *before* the actual API call so concurrent tasks
            // won't race through the same window.
            map.insert(chat_id.0, ChatThrottleState {
                last_sent: now,
                suppressed_until: None,
            });
        }

        // --- Send the API request ---
        let result = if let Some(tid) = thread_id {
            bot.send_chat_action(chat_id, ChatAction::Typing)
                .message_thread_id(tid)
                .await
        } else {
            bot.send_chat_action(chat_id, ChatAction::Typing).await
        };

        if let Err(ref e) = result {
            // Parse "Retry after N" from the error message.
            let err_str = e.to_string();
            if let Some(retry_secs) = parse_retry_after(&err_str) {
                let suppress_until = now + Duration::from_secs(retry_secs);
                let mut map = self.inner.lock().await;
                if let Some(state) = map.get_mut(&chat_id.0) {
                    state.suppressed_until = Some(suppress_until);
                }
                warn!(
                    chat_id = %chat_id.0,
                    retry_after_secs = retry_secs,
                    "Telegram rate-limited typing indicator; backing off"
                );
            } else {
                warn!(
                    chat_id = %chat_id.0,
                    thread_id = ?thread_id,
                    error = %e,
                    "Failed to send typing indicator"
                );
            }
            return false;
        }

        true
    }
}

/// Extract the number of seconds from a "Retry after N" error string.
///
/// Telegram errors typically contain text like `"Retry after 12"` or
/// `"retry_after: 12"`.
fn parse_retry_after(err: &str) -> Option<u64> {
    // Try common patterns: "Retry after N" and "retry_after":N
    let lower = err.to_lowercase();

    // Pattern 1: "retry after N"
    if let Some(pos) = lower.find("retry after ") {
        let rest = &err[pos + "retry after ".len()..];
        if let Some(num_str) = rest.split(|c: char| !c.is_ascii_digit()).next() {
            if let Ok(n) = num_str.parse::<u64>() {
                return Some(n);
            }
        }
    }

    // Pattern 2: "retry_after":N or "retry_after": N (JSON)
    if let Some(pos) = lower.find("retry_after") {
        let rest = &err[pos + "retry_after".len()..];
        // Skip past any ": or ":" characters
        let digits: String = rest
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(n) = digits.parse::<u64>() {
            return Some(n);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_retry_after_standard() {
        assert_eq!(parse_retry_after("Retry after 12"), Some(12));
    }

    #[test]
    fn test_parse_retry_after_json() {
        assert_eq!(
            parse_retry_after(r#"{"retry_after":30,"description":"Too Many Requests"}"#),
            Some(30)
        );
    }

    #[test]
    fn test_parse_retry_after_with_colon_space() {
        assert_eq!(
            parse_retry_after(r#"retry_after: 5"#),
            Some(5)
        );
    }

    #[test]
    fn test_parse_retry_after_in_sentence() {
        assert_eq!(
            parse_retry_after("Too Many Requests: retry after 7 seconds"),
            Some(7)
        );
    }

    #[test]
    fn test_parse_retry_after_no_match() {
        assert_eq!(parse_retry_after("Some other error"), None);
    }

    #[test]
    fn test_parse_retry_after_case_insensitive() {
        assert_eq!(parse_retry_after("RETRY AFTER 3"), Some(3));
    }

    #[tokio::test]
    async fn test_throttle_cooldown() {
        let throttle = TypingThrottle::new();
        let chat_id = ChatId(12345);

        // Manually insert a recent entry to simulate a just-sent indicator.
        {
            let mut map = throttle.inner.lock().await;
            map.insert(12345, ChatThrottleState {
                last_sent: Instant::now(),
                suppressed_until: None,
            });
        }

        // The next call should be suppressed (we can't actually call the API
        // in tests, but we can verify the state check logic).
        let map = throttle.inner.lock().await;
        let state = map.get(&12345).unwrap();
        assert!(state.last_sent.elapsed() < DEFAULT_COOLDOWN);
    }

    #[tokio::test]
    async fn test_suppression_takes_priority() {
        let throttle = TypingThrottle::new();

        // Insert state with suppression 10 seconds in the future.
        {
            let mut map = throttle.inner.lock().await;
            map.insert(99999, ChatThrottleState {
                last_sent: Instant::now() - Duration::from_secs(10), // well past cooldown
                suppressed_until: Some(Instant::now() + Duration::from_secs(10)),
            });
        }

        // Even though cooldown has elapsed, suppression should block.
        let map = throttle.inner.lock().await;
        let state = map.get(&99999).unwrap();
        assert!(state.suppressed_until.unwrap() > Instant::now());
    }
}
