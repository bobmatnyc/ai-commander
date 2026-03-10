//! Premium-aware feature detection for Telegram bot UI.
//!
//! Provides `UserTier` and `FeatureSet` to gate UI features based on whether
//! the interacting user has Telegram Premium. All features degrade gracefully
//! for standard users and older clients.

use teloxide::types::User;

/// Whether a user has Telegram Premium.
#[derive(Debug, Clone, PartialEq)]
pub enum UserTier {
    Standard,
    Premium,
}

impl UserTier {
    pub fn from_user(user: &User) -> Self {
        if user.is_premium {
            UserTier::Premium
        } else {
            UserTier::Standard
        }
    }
}

/// Feature flags computed once per user/context, applied throughout the response pipeline.
#[derive(Debug, Clone)]
pub struct FeatureSet {
    pub tier: UserTier,
    pub is_private_chat: bool,
    /// Use custom emoji for status indicators (requires bot-owner Premium for send; always true here).
    /// Set custom_emoji_id values in `format_status_emoji` once IDs are confirmed.
    pub use_custom_emoji: bool,
    /// Add message effects on completion (private chat only; free effects work for all users).
    pub use_message_effects: bool,
    /// Wrap long <pre> blocks in <blockquote expandable> — gracefully ignored by old clients.
    pub use_expandable_blockquotes: bool,
    /// Add emoji reactions to the original user message on task completion.
    pub use_reactions: bool,
    /// Maximum characters per message before splitting.
    /// Premium users can receive up to 4096 chars per message regardless (the 8192 limit is for
    /// messages *sent by* premium users, not received). We use 4096 for all, but track the field
    /// for future use if the API changes.
    pub max_message_length: usize,
    /// Send progress edit-messages silently (no sound notification).
    pub silent_progress: bool,
}

impl FeatureSet {
    /// Build a `FeatureSet` for the given user and chat context.
    pub fn for_context(user: Option<&User>, is_private: bool) -> Self {
        let tier = user.map(UserTier::from_user).unwrap_or(UserTier::Standard);
        Self {
            tier,
            is_private_chat: is_private,
            use_custom_emoji: true,          // bot owner has Premium
            use_message_effects: is_private, // confetti effect only in private chats
            use_expandable_blockquotes: true, // supported by all recent clients
            use_reactions: true,             // bot reactions work for all
            max_message_length: 4096,        // Telegram hard limit for bot messages
            silent_progress: true,
        }
    }

    /// Fallback for contexts where no user info is available.
    pub fn standard_fallback() -> Self {
        Self::for_context(None, false)
    }
}

/// Threshold (chars) above which a `<pre>` block is wrapped in `<blockquote expandable>`.
pub const EXPANDABLE_BLOCKQUOTE_THRESHOLD: usize = 300;

/// Telegram message effect ID for the free confetti animation (no Premium required).
/// Apply only in private chats via `use_message_effects`.
///
/// To verify: send a message with this effect_id in a private chat and confirm it shows confetti.
/// Source: <https://core.telegram.org/bots/api#sendmessage> effect_id field.
pub const EFFECT_ID_CONFETTI: &str = "5066970843586925436";

/// Wrap `<pre>` blocks longer than `EXPANDABLE_BLOCKQUOTE_THRESHOLD` chars in
/// `<blockquote expandable>` so Telegram clients collapse them by default.
///
/// Older clients that don't support `expandable` render a plain blockquote — safe fallback.
pub fn apply_expandable_blockquotes(html: &str) -> String {
    // Fast path: no <pre> tags at all.
    if !html.contains("<pre>") && !html.contains("<pre ") {
        return html.to_owned();
    }

    let mut result = String::with_capacity(html.len() + 64);
    let mut remaining = html;

    while let Some(pre_start) = remaining.find("<pre") {
        // Append everything before this <pre>.
        result.push_str(&remaining[..pre_start]);

        // Find end of opening tag.
        let tag_end = match remaining[pre_start..].find('>') {
            Some(i) => pre_start + i + 1,
            None => {
                // Malformed — emit as-is.
                result.push_str(&remaining[pre_start..]);
                return result;
            }
        };

        // Find closing </pre>.
        let close_tag = "</pre>";
        let after_open = &remaining[tag_end..];
        match after_open.find(close_tag) {
            Some(close_offset) => {
                let pre_content_end = tag_end + close_offset + close_tag.len();
                let full_pre = &remaining[pre_start..pre_content_end];

                if full_pre.len() > EXPANDABLE_BLOCKQUOTE_THRESHOLD {
                    result.push_str("<blockquote expandable>");
                    result.push_str(full_pre);
                    result.push_str("</blockquote>");
                } else {
                    result.push_str(full_pre);
                }
                remaining = &remaining[pre_content_end..];
            }
            None => {
                // No closing tag — emit remainder as-is.
                result.push_str(&remaining[pre_start..]);
                return result;
            }
        }
    }

    result.push_str(remaining);
    result
}

/// Split a message at newline boundaries so each chunk fits within `max_len` chars.
///
/// The split prefers the last newline before the limit. If no newline exists within
/// `max_len`, the chunk is hard-split at `max_len` to avoid silently truncating.
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_owned()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while remaining.len() > max_len {
        // Try to split on the last newline within max_len.
        let split_at = match remaining[..max_len].rfind('\n') {
            Some(pos) => pos + 1, // include the newline in the first chunk
            None => max_len,      // hard split
        };
        chunks.push(remaining[..split_at].to_owned());
        remaining = &remaining[split_at..];
    }

    if !remaining.is_empty() {
        chunks.push(remaining.to_owned());
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_message_short() {
        let chunks = split_message("hello", 4096);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_split_message_newline_boundary() {
        let line = "a".repeat(100);
        let text = format!("{}\n{}", line, line);
        let chunks = split_message(&text, 150);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].ends_with('\n'));
        assert_eq!(chunks[1], line);
    }

    #[test]
    fn test_split_message_hard_split() {
        let text = "a".repeat(200);
        let chunks = split_message(&text, 100);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 100);
        assert_eq!(chunks[1].len(), 100);
    }

    #[test]
    fn test_expandable_blockquotes_short_pre_unchanged() {
        let input = "<pre><code>short</code></pre>";
        assert_eq!(apply_expandable_blockquotes(input), input);
    }

    #[test]
    fn test_expandable_blockquotes_long_pre_wrapped() {
        let long_code = "x".repeat(400);
        let input = format!("<pre><code>{}</code></pre>", long_code);
        let output = apply_expandable_blockquotes(&input);
        assert!(output.starts_with("<blockquote expandable>"));
        assert!(output.ends_with("</blockquote>"));
    }

    #[test]
    fn test_expandable_blockquotes_no_pre_passthrough() {
        let input = "plain text with <b>bold</b>";
        assert_eq!(apply_expandable_blockquotes(input), input);
    }
}
