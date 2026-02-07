//! TUI application state and logic.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use commander_adapters::AdapterRegistry;
use commander_persistence::StateStore;
use commander_tmux::TmuxOrchestrator;

#[cfg(feature = "agents")]
use commander_orchestrator::AgentOrchestrator;
#[cfg(feature = "agents")]
use std::sync::Arc;
#[cfg(feature = "agents")]
use tokio::runtime::Handle as TokioHandle;

/// Direction of a message in the output area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageDirection {
    /// User input sent to project
    Sent,
    /// Response received from AI
    Received,
    /// System/status messages
    System,
}

/// A message displayed in the output area.
#[derive(Debug, Clone)]
pub struct Message {
    /// When the message was created
    pub timestamp: DateTime<Utc>,
    /// Direction (sent, received, system)
    pub direction: MessageDirection,
    /// Project name (or "system" for system messages)
    pub project: String,
    /// Message content
    pub content: String,
}

impl Message {
    /// Create a new message.
    pub fn new(direction: MessageDirection, project: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            direction,
            project: project.into(),
            content: content.into(),
        }
    }

    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(MessageDirection::System, "system", content)
    }

    /// Create a sent message.
    pub fn sent(project: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(MessageDirection::Sent, project, content)
    }

    /// Create a received message.
    pub fn received(project: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(MessageDirection::Received, project, content)
    }
}

/// Input mode for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Normal mode - typing input
    #[default]
    Normal,
    /// Scrolling through output
    Scrolling,
}

/// View mode for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Normal chat mode
    #[default]
    Normal,
    /// Live tmux view (inspect mode)
    Inspect,
    /// Sessions list view
    Sessions,
}

/// Information about a tmux session for the sessions list view.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Session name
    pub name: String,
    /// Whether this is a commander-managed session
    pub is_commander: bool,
    /// Whether this session is currently connected
    pub is_connected: bool,
}

/// TUI application state.
pub struct App {
    // Connection state
    /// Currently connected project name
    pub project: Option<String>,
    /// Currently connected project path
    pub project_path: Option<String>,
    /// Tmux orchestrator for session management
    pub tmux: Option<TmuxOrchestrator>,
    /// Adapter registry
    pub registry: AdapterRegistry,
    /// State store for projects
    pub store: StateStore,
    /// Map of project name to tmux session name
    pub sessions: HashMap<String, String>,

    // UI State
    /// Current input text
    pub input: String,
    /// Cursor position in input
    pub cursor_pos: usize,
    /// Messages in the output area
    pub messages: Vec<Message>,
    /// Scroll offset for output area (0 = bottom)
    pub scroll_offset: usize,
    /// Whether AI is currently working
    pub is_working: bool,
    /// Progress indicator (0.0 - 1.0)
    pub progress: f64,
    /// Current input mode
    pub input_mode: InputMode,

    // Runtime
    /// Whether the app should quit
    pub should_quit: bool,
    /// Last captured output for comparison
    pub last_output: String,

    // Inspect mode
    /// Current view mode (Normal or Inspect)
    pub view_mode: ViewMode,
    /// Cached tmux output for inspect mode
    pub inspect_content: String,
    /// Scroll offset for inspect mode (lines from top)
    pub inspect_scroll: usize,

    // Sessions mode
    /// List of sessions for sessions view
    pub session_list: Vec<SessionInfo>,
    /// Currently selected session index
    pub session_selected: usize,

    // Response summarization
    /// Buffer for collecting raw response lines
    pub(super) response_buffer: Vec<String>,
    /// When output last changed (for idle detection)
    pub(super) last_activity: Option<Instant>,
    /// Receiver for async summarization result
    pub(super) summarizer_rx: Option<mpsc::Receiver<String>>,
    /// Whether we're currently summarizing
    pub(super) is_summarizing: bool,
    /// The user's original query (for context in summarization)
    pub(super) pending_query: Option<String>,

    // Command history
    /// Previous commands
    pub(super) command_history: Vec<String>,
    /// Current position in history (None = new input, Some(i) = browsing history)
    pub(super) history_index: Option<usize>,
    /// Saved input when browsing history
    pub(super) saved_input: String,

    // Tab completion
    /// Cached completions for current input prefix
    pub(super) completions: Vec<String>,
    /// Current completion index (None = not in completion mode)
    pub(super) completion_index: Option<usize>,

    // Session status monitoring
    /// Last known ready state for each session (true = waiting for input)
    pub(super) session_ready_state: HashMap<String, bool>,
    /// Last time we checked session status (fast check - 2 sec)
    pub(super) last_status_check: Option<Instant>,
    /// Last time we did a full session scan (slow check - 5 min)
    pub(super) last_full_scan: Option<Instant>,
    /// Sessions that were waiting in the last full scan
    pub(super) last_scan_waiting: std::collections::HashSet<String>,

    // Agent orchestration (optional, behind feature flag)
    #[cfg(feature = "agents")]
    /// Agent orchestrator for multi-agent system integration.
    pub(super) orchestrator: Option<AgentOrchestrator>,
    #[cfg(feature = "agents")]
    /// Tokio runtime handle for async operations.
    pub(super) runtime_handle: Option<Arc<TokioHandle>>,
}

impl App {
    /// Create a new App instance.
    pub fn new(state_dir: &std::path::Path) -> Self {
        // Restart Telegram bot if running to ensure it uses latest code
        crate::restart_telegram_if_running();

        let store = StateStore::new(state_dir);
        let registry = AdapterRegistry::new();
        let tmux = TmuxOrchestrator::new().ok();

        let mut app = Self {
            project: None,
            project_path: None,
            tmux,
            registry,
            store,
            sessions: HashMap::new(),

            input: String::new(),
            cursor_pos: 0,
            messages: Vec::new(),
            scroll_offset: 0,
            is_working: false,
            progress: 0.0,
            input_mode: InputMode::Normal,

            should_quit: false,
            last_output: String::new(),

            view_mode: ViewMode::Normal,
            inspect_content: String::new(),
            inspect_scroll: 0,

            session_list: Vec::new(),
            session_selected: 0,

            response_buffer: Vec::new(),
            last_activity: None,
            summarizer_rx: None,
            is_summarizing: false,
            pending_query: None,

            command_history: Vec::new(),
            history_index: None,
            saved_input: String::new(),

            completions: Vec::new(),
            completion_index: None,

            session_ready_state: HashMap::new(),
            last_status_check: None,
            last_full_scan: None,
            last_scan_waiting: std::collections::HashSet::new(),

            #[cfg(feature = "agents")]
            orchestrator: None,
            #[cfg(feature = "agents")]
            runtime_handle: None,
        };

        // Add welcome message
        app.messages.push(Message::system("Welcome to Commander TUI"));
        app.messages.push(Message::system("Type /help for commands, Ctrl+C to quit"));

        if app.tmux.is_none() {
            app.messages.push(Message::system("Warning: tmux not available"));
        }

        app
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = Message::system("test");
        assert_eq!(msg.direction, MessageDirection::System);
        assert_eq!(msg.content, "test");
    }

    #[test]
    fn test_view_mode_default() {
        assert_eq!(ViewMode::default(), ViewMode::Normal);
    }

    #[test]
    fn test_inspect_scroll() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut app = App::new(temp_dir.path());

        // Set up some inspect content
        app.inspect_content = "line1\nline2\nline3\nline4\nline5".to_string();
        app.view_mode = ViewMode::Inspect;

        // Initial scroll should be 0
        assert_eq!(app.inspect_scroll, 0);

        // Scroll up
        app.inspect_scroll_up();
        assert_eq!(app.inspect_scroll, 1);

        // Scroll down
        app.inspect_scroll_down();
        assert_eq!(app.inspect_scroll, 0);

        // Scroll down at 0 should stay at 0
        app.inspect_scroll_down();
        assert_eq!(app.inspect_scroll, 0);

        // Page up
        app.inspect_scroll_page_up(3);
        assert_eq!(app.inspect_scroll, 3);

        // Page down
        app.inspect_scroll_page_down(2);
        assert_eq!(app.inspect_scroll, 1);
    }

    #[test]
    fn test_session_info() {
        let session = SessionInfo {
            name: "commander-test".to_string(),
            is_commander: true,
            is_connected: false,
        };
        assert!(session.is_commander);
        assert!(!session.is_connected);
    }

    #[test]
    fn test_session_selection() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut app = App::new(temp_dir.path());

        // Set up some test sessions
        app.session_list = vec![
            SessionInfo {
                name: "commander-proj1".to_string(),
                is_commander: true,
                is_connected: false,
            },
            SessionInfo {
                name: "commander-proj2".to_string(),
                is_commander: true,
                is_connected: true,
            },
            SessionInfo {
                name: "other-session".to_string(),
                is_commander: false,
                is_connected: false,
            },
        ];
        app.view_mode = ViewMode::Sessions;
        app.session_selected = 0;

        // Initial selection
        assert_eq!(app.session_selected, 0);

        // Move down
        app.session_select_down();
        assert_eq!(app.session_selected, 1);

        app.session_select_down();
        assert_eq!(app.session_selected, 2);

        // At bottom, should not go further
        app.session_select_down();
        assert_eq!(app.session_selected, 2);

        // Move up
        app.session_select_up();
        assert_eq!(app.session_selected, 1);

        app.session_select_up();
        assert_eq!(app.session_selected, 0);

        // At top, should not go further
        app.session_select_up();
        assert_eq!(app.session_selected, 0);
    }

    #[test]
    fn test_view_mode_sessions() {
        assert_ne!(ViewMode::Sessions, ViewMode::Normal);
        assert_ne!(ViewMode::Sessions, ViewMode::Inspect);
    }

    #[test]
    fn test_tab_completion_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut app = App::new(temp_dir.path());

        // Type /con and press Tab
        app.input = "/con".to_string();
        app.cursor_pos = 4;
        app.complete_command();

        // Should complete to /connect
        assert_eq!(app.input, "/connect");
        assert_eq!(app.cursor_pos, 8);
    }

    #[test]
    fn test_tab_completion_cycles() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut app = App::new(temp_dir.path());

        // Type /s and press Tab multiple times
        app.input = "/s".to_string();
        app.cursor_pos = 2;

        // First Tab: /send
        app.complete_command();
        assert_eq!(app.input, "/send");

        // Second Tab: /sessions
        app.complete_command();
        assert_eq!(app.input, "/sessions");

        // Third Tab: /status
        app.complete_command();
        assert_eq!(app.input, "/status");

        // Fourth Tab: /stop
        app.complete_command();
        assert_eq!(app.input, "/stop");

        // Fifth Tab: cycles back to /send
        app.complete_command();
        assert_eq!(app.input, "/send");
    }

    #[test]
    fn test_tab_completion_no_match() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut app = App::new(temp_dir.path());

        // Type something that doesn't match any command
        app.input = "/xyz".to_string();
        app.cursor_pos = 4;
        app.complete_command();

        // Should stay unchanged
        assert_eq!(app.input, "/xyz");
        assert!(app.completion_index.is_none());
    }

    #[test]
    fn test_tab_completion_non_slash_ignored() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut app = App::new(temp_dir.path());

        // Type something without /
        app.input = "connect".to_string();
        app.cursor_pos = 7;
        app.complete_command();

        // Should stay unchanged
        assert_eq!(app.input, "connect");
        assert!(app.completions.is_empty());
    }

    #[test]
    fn test_tab_completion_reset_on_char() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut app = App::new(temp_dir.path());

        // Complete /s to /send
        app.input = "/s".to_string();
        app.cursor_pos = 2;
        app.complete_command();
        assert_eq!(app.input, "/send");
        assert!(!app.completions.is_empty());

        // Type a character - should reset completions
        app.reset_completions();
        app.enter_char('x');
        assert!(app.completions.is_empty());
        assert!(app.completion_index.is_none());
    }

    #[test]
    fn test_tab_completion_telegram() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut app = App::new(temp_dir.path());

        // Type /te and press Tab
        app.input = "/te".to_string();
        app.cursor_pos = 3;
        app.complete_command();

        // Should complete to /telegram
        assert_eq!(app.input, "/telegram");
        assert_eq!(app.cursor_pos, 9);
    }
}
