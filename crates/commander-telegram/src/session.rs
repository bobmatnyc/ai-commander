//! User session management for Telegram bot.

use std::time::Instant;

use teloxide::types::{ChatId, MessageId};

/// A user's session with a connected project.
#[derive(Debug)]
pub struct UserSession {
    /// Telegram chat ID.
    pub chat_id: ChatId,
    /// Path to the connected project.
    pub project_path: String,
    /// Name of the project.
    pub project_name: String,
    /// Name of the tmux session.
    pub tmux_session: String,
    /// Buffer for collecting response lines.
    pub response_buffer: Vec<String>,
    /// When output last changed (for idle detection).
    pub last_output_time: Option<Instant>,
    /// Last captured tmux output (for detecting new lines).
    pub last_output: String,
    /// The user's original query (for context in summarization).
    pub pending_query: Option<String>,
    /// Whether we're currently waiting for a response.
    pub is_waiting: bool,
    /// Message ID of the user's pending query (for reply threading).
    pub pending_message_id: Option<MessageId>,
}

impl UserSession {
    /// Create a new user session.
    pub fn new(
        chat_id: ChatId,
        project_path: String,
        project_name: String,
        tmux_session: String,
    ) -> Self {
        Self {
            chat_id,
            project_path,
            project_name,
            tmux_session,
            response_buffer: Vec::new(),
            last_output_time: None,
            last_output: String::new(),
            pending_query: None,
            is_waiting: false,
            pending_message_id: None,
        }
    }

    /// Reset the response collection state.
    pub fn reset_response_state(&mut self) {
        self.response_buffer.clear();
        self.last_output_time = None;
        self.pending_query = None;
        self.is_waiting = false;
        self.pending_message_id = None;
    }

    /// Start collecting a response for a query.
    pub fn start_response_collection(&mut self, query: &str, current_output: String, message_id: Option<MessageId>) {
        self.response_buffer.clear();
        self.last_output = current_output;
        self.last_output_time = Some(Instant::now());
        self.pending_query = Some(query.to_string());
        self.is_waiting = true;
        self.pending_message_id = message_id;
    }

    /// Add new lines to the response buffer.
    pub fn add_response_lines(&mut self, lines: Vec<String>) {
        for line in lines {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                self.response_buffer.push(trimmed.to_string());
            }
        }
        self.last_output_time = Some(Instant::now());
    }

    /// Check if enough time has passed since last activity (idle detection).
    pub fn is_idle(&self, idle_threshold_ms: u128) -> bool {
        self.last_output_time
            .map(|t| t.elapsed().as_millis() > idle_threshold_ms)
            .unwrap_or(false)
    }

    /// Get the collected response as a single string.
    pub fn get_response(&self) -> String {
        self.response_buffer.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_session() {
        let session = UserSession::new(
            ChatId(12345),
            "/path/to/project".to_string(),
            "my-project".to_string(),
            "commander-my-project".to_string(),
        );

        assert_eq!(session.chat_id.0, 12345);
        assert_eq!(session.project_path, "/path/to/project");
        assert_eq!(session.project_name, "my-project");
        assert_eq!(session.tmux_session, "commander-my-project");
        assert!(session.response_buffer.is_empty());
        assert!(!session.is_waiting);
    }

    #[test]
    fn test_response_collection() {
        let mut session = UserSession::new(
            ChatId(12345),
            "/path".to_string(),
            "proj".to_string(),
            "session".to_string(),
        );

        session.start_response_collection("hello", "initial output".to_string(), Some(MessageId(42)));
        assert!(session.is_waiting);
        assert_eq!(session.pending_query, Some("hello".to_string()));
        assert_eq!(session.pending_message_id, Some(MessageId(42)));

        session.add_response_lines(vec!["line 1".to_string(), "line 2".to_string()]);
        assert_eq!(session.response_buffer.len(), 2);
        assert_eq!(session.get_response(), "line 1\nline 2");

        session.reset_response_state();
        assert!(!session.is_waiting);
        assert!(session.response_buffer.is_empty());
    }
}
