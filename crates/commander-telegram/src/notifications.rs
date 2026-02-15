//! Shared notification file for cross-channel broadcasts.
//!
//! Notifications are stored in `~/.ai-commander/state/notifications.json` so that:
//! - The TUI/REPL can write notifications when sessions need attention
//! - The Telegram bot can poll and broadcast to all authorized users

use std::collections::VecDeque;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use commander_core::config;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Maximum number of notifications to keep in the queue.
const MAX_NOTIFICATIONS: usize = 100;

/// Notification expiry time in seconds (1 hour).
const NOTIFICATION_EXPIRY_SECS: u64 = 3600;

/// A notification to be broadcast to all channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique ID for deduplication
    pub id: String,
    /// The notification message
    pub message: String,
    /// Optional session/project name this relates to
    pub session: Option<String>,
    /// Unix timestamp when created
    pub created_at: u64,
    /// Whether this has been read by each channel (channel_name -> read)
    #[serde(default)]
    pub read_by: std::collections::HashSet<String>,
}

impl Notification {
    /// Create a new notification.
    pub fn new(message: impl Into<String>, session: Option<String>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Generate a simple ID from timestamp and random bits
        let id = format!("{}-{}", now, std::process::id());

        Self {
            id,
            message: message.into(),
            session,
            created_at: now,
            read_by: std::collections::HashSet::new(),
        }
    }

    /// Check if this notification has expired.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.created_at) > NOTIFICATION_EXPIRY_SECS
    }

    /// Check if this notification has been read by a channel.
    pub fn is_read_by(&self, channel: &str) -> bool {
        self.read_by.contains(channel)
    }

    /// Mark as read by a channel.
    pub fn mark_read(&mut self, channel: &str) {
        self.read_by.insert(channel.to_string());
    }
}

/// Notification queue stored in the shared file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationQueue {
    pub notifications: VecDeque<Notification>,
}

/// Load the notification queue from the shared file.
pub fn load_notifications() -> NotificationQueue {
    let path = config::notifications_file();

    if !path.exists() {
        return NotificationQueue::default();
    }

    match fs::read_to_string(&path) {
        Ok(content) => {
            serde_json::from_str(&content).unwrap_or_else(|e| {
                warn!(error = %e, "Failed to parse notifications file");
                NotificationQueue::default()
            })
        }
        Err(e) => {
            warn!(error = %e, "Failed to read notifications file");
            NotificationQueue::default()
        }
    }
}

/// Save the notification queue to the shared file.
fn save_notifications(queue: &NotificationQueue) -> Result<(), std::io::Error> {
    let path = config::notifications_file();

    // Ensure parent directory exists
    config::ensure_runtime_state_dir()?;

    let content = serde_json::to_string_pretty(queue)?;
    fs::write(&path, content)?;
    debug!(path = %path.display(), "Saved notifications file");
    Ok(())
}

/// Push a new notification to the queue.
pub fn push_notification(message: impl Into<String>, session: Option<String>) -> Result<(), std::io::Error> {
    let notification = Notification::new(message, session);

    let mut queue = load_notifications();

    // Clean up expired notifications
    queue.notifications.retain(|n| !n.is_expired());

    // Trim if too many
    while queue.notifications.len() >= MAX_NOTIFICATIONS {
        queue.notifications.pop_front();
    }

    debug!(id = %notification.id, message = %notification.message, "Pushing notification");
    queue.notifications.push_back(notification);

    save_notifications(&queue)
}

/// Get all unread notifications for a channel.
pub fn get_unread_notifications(channel: &str) -> Vec<Notification> {
    let queue = load_notifications();

    queue.notifications
        .iter()
        .filter(|n| !n.is_expired() && !n.is_read_by(channel))
        .cloned()
        .collect()
}

/// Mark notifications as read by a channel.
pub fn mark_notifications_read(channel: &str, notification_ids: &[String]) -> Result<(), std::io::Error> {
    let mut queue = load_notifications();

    // Clean up expired
    queue.notifications.retain(|n| !n.is_expired());

    // Mark as read
    for notification in queue.notifications.iter_mut() {
        if notification_ids.contains(&notification.id) {
            notification.mark_read(channel);
        }
    }

    // Remove notifications that have been read by all known channels
    // For now, keep them until they expire

    save_notifications(&queue)
}

/// Convenience function to broadcast a session ready notification.
///
/// Uses conversational language instead of raw session output.
pub fn notify_session_ready(session_name: &str, preview: Option<&str>) -> Result<(), std::io::Error> {
    use commander_core::notification_parser::{parse_session_preview, strip_ansi};

    let display_name = session_name.strip_prefix("commander-").unwrap_or(session_name);

    let mut message = if let Some(prev) = preview {
        let clean_preview = strip_ansi(prev);
        if clean_preview.is_empty() {
            format!("Session \"{}\" is ready for input", display_name)
        } else {
            // Parse and convert to conversational format
            let status = parse_session_preview(display_name, &clean_preview);
            let brief = status.to_brief();
            if brief.is_empty() || brief == display_name {
                format!("Session \"{}\" is ready for input", display_name)
            } else {
                format!("Session \"{}\" is ready: {}", display_name, brief)
            }
        }
    } else {
        format!("Session \"{}\" is ready for input", display_name)
    };

    // Add clickable connect link
    message.push_str(&format!("\n\n/connect {}", display_name));

    push_notification(message, Some(session_name.to_string()))
}

/// Convenience function to broadcast a session resumed notification.
///
/// Uses conversational language.
pub fn notify_session_resumed(session_name: &str) -> Result<(), std::io::Error> {
    let display_name = session_name.strip_prefix("commander-").unwrap_or(session_name);
    let message = format!("Session \"{}\" resumed work", display_name);

    push_notification(message, Some(session_name.to_string()))
}

/// Convenience function to broadcast multiple new sessions waiting.
///
/// Uses conversational language with clean, human-readable summaries.
pub fn notify_sessions_waiting(sessions: &[(String, String)]) -> Result<(), std::io::Error> {
    use commander_core::notification_parser::{parse_session_preview, strip_ansi};

    if sessions.is_empty() {
        return Ok(());
    }

    // Conversational header
    let mut message = if sessions.len() == 1 {
        "A session is waiting for your input:".to_string()
    } else {
        format!("{} sessions are waiting for your input:", sessions.len())
    };

    for (name, preview) in sessions.iter() {
        let display_name = name.strip_prefix("commander-").unwrap_or(name);
        let clean_preview = strip_ansi(preview);

        if clean_preview.is_empty() {
            message.push_str(&format!("\n  - \"{}\"", display_name));
        } else {
            // Parse and convert to conversational format
            let status = parse_session_preview(display_name, &clean_preview);
            let brief = status.to_brief();
            if brief.is_empty() || brief == display_name {
                message.push_str(&format!("\n  - \"{}\"", display_name));
            } else {
                message.push_str(&format!("\n  - \"{}\": {}", display_name, brief));
            }
        }
    }

    // Add clickable connect links
    message.push_str("\n\nChat with: ");
    let connect_commands: Vec<String> = sessions
        .iter()
        .map(|(name, _)| {
            let display_name = name.strip_prefix("commander-").unwrap_or(name);
            format!("/connect {}", display_name)
        })
        .collect();
    message.push_str(&connect_commands.join(" | "));

    push_notification(message, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_creation() {
        let notification = Notification::new("Test message", Some("test-session".to_string()));
        assert!(!notification.is_expired());
        assert!(!notification.is_read_by("telegram"));
        assert!(notification.message.contains("Test message"));
    }

    #[test]
    fn test_notification_read_tracking() {
        let mut notification = Notification::new("Test", None);
        assert!(!notification.is_read_by("telegram"));
        assert!(!notification.is_read_by("tui"));

        notification.mark_read("telegram");
        assert!(notification.is_read_by("telegram"));
        assert!(!notification.is_read_by("tui"));
    }

    #[test]
    fn test_notification_expiry() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Fresh notification
        let fresh = Notification {
            id: "test".to_string(),
            message: "test".to_string(),
            session: None,
            created_at: now,
            read_by: std::collections::HashSet::new(),
        };
        assert!(!fresh.is_expired());

        // Expired notification (2 hours ago)
        let expired = Notification {
            id: "test".to_string(),
            message: "test".to_string(),
            session: None,
            created_at: now - 7200,
            read_by: std::collections::HashSet::new(),
        };
        assert!(expired.is_expired());
    }
}
