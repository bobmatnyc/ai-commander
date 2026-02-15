//! User session management for Telegram bot.

use std::time::Instant;

use teloxide::types::{ChatId, MessageId, ThreadId};

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
    /// Line count at last progress update (to avoid spamming updates).
    pub last_progress_line_count: usize,
    /// Whether we've signaled that summarization is starting.
    pub is_summarizing: bool,
    /// Line count at last incremental summary (to detect 50-line thresholds).
    pub last_incremental_summary_line_count: usize,
    /// Forum topic thread ID (for group mode).
    pub thread_id: Option<ThreadId>,
    /// Worktree info (if session uses git worktree).
    pub worktree_info: Option<WorktreeInfo>,
}

/// Worktree information for sessions created with /connect-tree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Path to the worktree directory.
    pub worktree_path: String,
    /// Branch name for this worktree.
    pub branch_name: String,
    /// Original project path (parent repository).
    pub parent_repo: String,
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
            last_progress_line_count: 0,
            is_summarizing: false,
            last_incremental_summary_line_count: 0,
            thread_id: None,
            worktree_info: None,
        }
    }

    /// Create a new user session with forum topic thread ID.
    pub fn with_thread_id(
        chat_id: ChatId,
        project_path: String,
        project_name: String,
        tmux_session: String,
        thread_id: ThreadId,
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
            last_progress_line_count: 0,
            is_summarizing: false,
            last_incremental_summary_line_count: 0,
            thread_id: Some(thread_id),
            worktree_info: None,
        }
    }

    /// Reset the response collection state.
    pub fn reset_response_state(&mut self) {
        self.response_buffer.clear();
        self.last_output_time = None;
        self.pending_query = None;
        self.is_waiting = false;
        self.pending_message_id = None;
        self.last_progress_line_count = 0;
        self.is_summarizing = false;
        self.last_incremental_summary_line_count = 0;
    }

    /// Start collecting a response for a query.
    pub fn start_response_collection(&mut self, query: &str, current_output: String, message_id: Option<MessageId>) {
        self.response_buffer.clear();
        self.last_output = current_output;
        self.last_output_time = Some(Instant::now());
        self.pending_query = Some(query.to_string());
        self.is_waiting = true;
        self.pending_message_id = message_id;
        self.last_progress_line_count = 0;
        self.is_summarizing = false;
        self.last_incremental_summary_line_count = 0;
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

    /// Check if a progress update should be emitted.
    /// Updates every 5 lines to prevent rate limiting.
    pub fn should_emit_progress(&self) -> bool {
        let current_lines = self.response_buffer.len();
        current_lines > 0 && current_lines >= self.last_progress_line_count + 5
    }

    /// Generate a progress message and update the tracking counter.
    pub fn get_progress_message(&mut self) -> String {
        let line_count = self.response_buffer.len();
        self.last_progress_line_count = line_count;
        format!("📥 Receiving...{} lines captured", line_count)
    }

    /// Check if an incremental summary should be emitted.
    /// Emits every 50 lines (at 50, 100, 150, etc.).
    pub fn should_emit_incremental_summary(&self) -> bool {
        let current_lines = self.response_buffer.len();
        current_lines > 0 &&
        current_lines >= self.last_incremental_summary_line_count + 50
    }

    /// Mark that an incremental summary was sent at the current line count.
    pub fn mark_incremental_summary_sent(&mut self) {
        self.last_incremental_summary_line_count = self.response_buffer.len();
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
        assert_eq!(session.last_progress_line_count, 0);
        assert!(!session.is_summarizing);
        assert_eq!(session.last_incremental_summary_line_count, 0);
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
        assert!(!session.is_summarizing);

        session.add_response_lines(vec!["line 1".to_string(), "line 2".to_string()]);
        assert_eq!(session.response_buffer.len(), 2);
        assert_eq!(session.get_response(), "line 1\nline 2");

        session.reset_response_state();
        assert!(!session.is_waiting);
        assert!(session.response_buffer.is_empty());
        assert_eq!(session.last_progress_line_count, 0);
        assert!(!session.is_summarizing);
        assert_eq!(session.last_incremental_summary_line_count, 0);
    }

    #[test]
    fn test_progress_messages() {
        let mut session = UserSession::new(
            ChatId(12345),
            "/path".to_string(),
            "proj".to_string(),
            "session".to_string(),
        );

        // No progress initially
        assert!(!session.should_emit_progress());

        // Add 5 lines - should emit progress
        for i in 1..=5 {
            session.add_response_lines(vec![format!("line {}", i)]);
        }
        assert!(session.should_emit_progress());

        let progress = session.get_progress_message();
        assert_eq!(progress, "📥 Receiving...5 lines captured");
        assert_eq!(session.last_progress_line_count, 5);

        // Add 4 more lines - should not emit yet (need 5 more)
        for i in 6..=9 {
            session.add_response_lines(vec![format!("line {}", i)]);
        }
        assert!(!session.should_emit_progress());

        // Add 1 more line to reach threshold
        session.add_response_lines(vec!["line 10".to_string()]);
        assert!(session.should_emit_progress());

        let progress2 = session.get_progress_message();
        assert_eq!(progress2, "📥 Receiving...10 lines captured");
    }

    #[test]
    fn test_incremental_summaries() {
        let mut session = UserSession::new(
            ChatId(12345),
            "/path".to_string(),
            "proj".to_string(),
            "session".to_string(),
        );

        // No incremental summary initially
        assert!(!session.should_emit_incremental_summary());

        // Add 49 lines - should not emit yet
        for i in 1..=49 {
            session.add_response_lines(vec![format!("line {}", i)]);
        }
        assert!(!session.should_emit_incremental_summary());

        // Add 1 more line to reach 50 - should emit
        session.add_response_lines(vec!["line 50".to_string()]);
        assert!(session.should_emit_incremental_summary());
        assert_eq!(session.response_buffer.len(), 50);

        // Mark as sent
        session.mark_incremental_summary_sent();
        assert_eq!(session.last_incremental_summary_line_count, 50);

        // Add 49 more lines - should not emit yet
        for i in 51..=99 {
            session.add_response_lines(vec![format!("line {}", i)]);
        }
        assert!(!session.should_emit_incremental_summary());

        // Add 1 more line to reach 100 - should emit again
        session.add_response_lines(vec!["line 100".to_string()]);
        assert!(session.should_emit_incremental_summary());
        assert_eq!(session.response_buffer.len(), 100);
    }

    #[test]
    fn test_session_with_thread_id() {
        use teloxide::types::MessageId;

        let thread_id = ThreadId(MessageId(999));
        let session = UserSession::with_thread_id(
            ChatId(12345),
            "/path/to/project".to_string(),
            "my-project".to_string(),
            "commander-my-project".to_string(),
            thread_id,
        );

        assert_eq!(session.chat_id.0, 12345);
        assert_eq!(session.project_path, "/path/to/project");
        assert_eq!(session.project_name, "my-project");
        assert_eq!(session.tmux_session, "commander-my-project");
        assert_eq!(session.thread_id, Some(thread_id));
        assert!(session.response_buffer.is_empty());
        assert!(!session.is_waiting);
    }

    #[test]
    fn test_session_without_thread_id() {
        let session = UserSession::new(
            ChatId(12345),
            "/path/to/project".to_string(),
            "my-project".to_string(),
            "commander-my-project".to_string(),
        );

        assert!(session.thread_id.is_none());
    }
}
