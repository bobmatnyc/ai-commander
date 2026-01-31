//! TUI application state and logic.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use commander_adapters::AdapterRegistry;
use commander_persistence::StateStore;
use commander_tmux::TmuxOrchestrator;

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
}

impl App {
    /// Create a new App instance.
    pub fn new(state_dir: &std::path::Path) -> Self {
        let store = StateStore::new(state_dir);
        let registry = AdapterRegistry::new();
        let tmux = TmuxOrchestrator::new().ok();

        let mut app = Self {
            project: None,
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
        };

        // Add welcome message
        app.messages.push(Message::system("Welcome to Commander TUI"));
        app.messages.push(Message::system("Type /help for commands, Ctrl+C to quit"));

        if app.tmux.is_none() {
            app.messages.push(Message::system("Warning: tmux not available"));
        }

        app
    }

    /// Connect to a project by name.
    pub fn connect(&mut self, name: &str) -> Result<(), String> {
        // Load all projects
        let projects = self.store.load_all_projects()
            .map_err(|e| format!("Failed to load projects: {}", e))?;

        // Find project by name
        let project = projects.values()
            .find(|p| p.name == name || p.id.as_str() == name)
            .ok_or_else(|| format!("Project not found: {}", name))?;

        let session_name = format!("commander-{}", project.name);

        // Check if tmux session exists
        if let Some(ref tmux) = self.tmux {
            if tmux.session_exists(&session_name) {
                self.sessions.insert(project.name.clone(), session_name);
                self.project = Some(project.name.clone());
                self.messages.push(Message::system(format!("Connected to '{}'", project.name)));
                return Ok(());
            }

            // Try to start the project
            let tool_id = project.config.get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("claude-code");

            if let Some(adapter) = self.registry.get(tool_id) {
                let (cmd, cmd_args) = adapter.launch_command(&project.path);
                let full_cmd = if cmd_args.is_empty() {
                    cmd
                } else {
                    format!("{} {}", cmd, cmd_args.join(" "))
                };

                // Create tmux session
                if let Err(e) = tmux.create_session(&session_name) {
                    return Err(format!("Failed to create tmux session: {}", e));
                }

                // Send launch command
                if let Err(e) = tmux.send_line(&session_name, None, &full_cmd) {
                    return Err(format!("Failed to start adapter: {}", e));
                }

                self.sessions.insert(project.name.clone(), session_name);
                self.project = Some(project.name.clone());
                self.messages.push(Message::system(format!("Started and connected to '{}'", project.name)));
                return Ok(());
            }
        }

        Err("Tmux not available".to_string())
    }

    /// Disconnect from current project.
    pub fn disconnect(&mut self) {
        if let Some(project) = self.project.take() {
            self.messages.push(Message::system(format!("Disconnected from '{}'", project)));
        }
    }

    /// Stop a session: commit git changes and destroy tmux session.
    pub fn stop_session(&mut self, name: &str) {
        let session_name = format!("commander-{}", name);

        // Find project path for git operations
        let project_path = {
            if let Ok(projects) = self.store.load_all_projects() {
                projects.values()
                    .find(|p| p.name == name)
                    .map(|p| p.path.clone())
            } else {
                None
            }
        };

        // Step 1: Commit any git changes
        if let Some(path) = &project_path {
            self.messages.push(Message::system(format!("Checking for uncommitted changes in {}...", path)));

            match self.git_commit_changes(path, name) {
                Ok(true) => self.messages.push(Message::system("Changes committed.")),
                Ok(false) => self.messages.push(Message::system("No changes to commit.")),
                Err(e) => self.messages.push(Message::system(format!("Git warning: {}", e))),
            }
        }

        // Step 2: Destroy tmux session
        if let Some(tmux) = &self.tmux {
            match tmux.destroy_session(&session_name) {
                Ok(_) => {
                    self.messages.push(Message::system(format!("Session '{}' stopped.", name)));

                    // Remove from tracking
                    self.sessions.remove(name);

                    // Disconnect if it was current
                    if self.project.as_deref() == Some(name) {
                        self.project = None;
                        self.messages.push(Message::system("Disconnected."));
                    }
                }
                Err(e) => {
                    self.messages.push(Message::system(format!("Failed to stop session: {}", e)));
                }
            }
        } else {
            self.messages.push(Message::system("Tmux not available"));
        }
    }

    /// Commit any uncommitted git changes in the project directory.
    fn git_commit_changes(&self, path: &str, project_name: &str) -> Result<bool, String> {
        use std::process::Command;

        // Check if there are changes
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to run git status: {}", e))?;

        let changes = String::from_utf8_lossy(&status.stdout);
        if changes.trim().is_empty() {
            return Ok(false); // No changes
        }

        // Stage all changes
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to stage changes: {}", e))?;

        // Commit with message
        let message = format!("WIP: Auto-commit from Commander session '{}'", project_name);
        let commit = Command::new("git")
            .args(["commit", "-m", &message])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to commit: {}", e))?;

        if commit.status.success() {
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&commit.stderr);
            Err(format!("Commit failed: {}", stderr))
        }
    }

    /// Send a message to the connected project.
    pub fn send_message(&mut self, message: &str) -> Result<(), String> {
        let project = self.project.as_ref()
            .ok_or_else(|| "Not connected to any project".to_string())?;

        let session = self.sessions.get(project)
            .ok_or_else(|| "Session not found".to_string())?;

        let tmux = self.tmux.as_ref()
            .ok_or_else(|| "Tmux not available".to_string())?;

        // Capture initial output for comparison
        self.last_output = tmux.capture_output(session, None, Some(200))
            .unwrap_or_default();

        // Send the message
        tmux.send_line(session, None, message)
            .map_err(|e| format!("Failed to send: {}", e))?;

        // Add sent message to output
        self.messages.push(Message::sent(project.clone(), message));
        self.is_working = true;
        self.progress = 0.0;
        self.scroll_to_bottom();

        Ok(())
    }

    /// Poll for new output from tmux.
    pub fn poll_output(&mut self) {
        if !self.is_working {
            return;
        }

        let Some(project) = &self.project else { return };
        let Some(session) = self.sessions.get(project) else { return };
        let Some(tmux) = &self.tmux else { return };

        // Capture current output
        let current_output = match tmux.capture_output(session, None, Some(200)) {
            Ok(output) => output,
            Err(_) => return,
        };

        // Find new lines
        if current_output != self.last_output {
            let new_lines = find_new_lines(&self.last_output, &current_output);
            for line in new_lines {
                if !line.trim().is_empty() {
                    self.messages.push(Message::received(project.clone(), line));
                }
            }
            self.last_output = current_output;
            self.scroll_to_bottom();
        }

        // Update progress animation
        self.progress = (self.progress + 0.05) % 1.0;
    }

    /// Stop the working indicator.
    pub fn stop_working(&mut self) {
        self.is_working = false;
        self.progress = 0.0;
    }

    /// Toggle inspect mode (live tmux view).
    pub fn toggle_inspect_mode(&mut self) {
        match self.view_mode {
            ViewMode::Normal | ViewMode::Sessions => {
                if self.project.is_some() {
                    self.view_mode = ViewMode::Inspect;
                    self.inspect_scroll = 0;
                    self.refresh_inspect_content();
                    self.messages.push(Message::system("Entering inspect mode (F2 to exit)"));
                } else {
                    self.messages.push(Message::system("Connect to a project first"));
                }
            }
            ViewMode::Inspect => {
                self.view_mode = ViewMode::Normal;
                self.messages.push(Message::system("Exited inspect mode"));
            }
        }
    }

    /// Refresh the inspect content from tmux.
    pub fn refresh_inspect_content(&mut self) {
        if let (Some(project), Some(tmux)) = (&self.project, &self.tmux) {
            if let Some(session) = self.sessions.get(project) {
                // Capture more lines for full view
                if let Ok(output) = tmux.capture_output(session, None, Some(200)) {
                    self.inspect_content = output;
                }
            }
        }
    }

    /// Scroll up in inspect mode.
    pub fn inspect_scroll_up(&mut self) {
        let max_scroll = self.inspect_content.lines().count().saturating_sub(1);
        if self.inspect_scroll < max_scroll {
            self.inspect_scroll += 1;
        }
    }

    /// Scroll down in inspect mode.
    pub fn inspect_scroll_down(&mut self) {
        if self.inspect_scroll > 0 {
            self.inspect_scroll -= 1;
        }
    }

    /// Scroll up by a page in inspect mode.
    pub fn inspect_scroll_page_up(&mut self, page_size: usize) {
        let max_scroll = self.inspect_content.lines().count().saturating_sub(1);
        self.inspect_scroll = self.inspect_scroll.saturating_add(page_size).min(max_scroll);
    }

    /// Scroll down by a page in inspect mode.
    pub fn inspect_scroll_page_down(&mut self, page_size: usize) {
        self.inspect_scroll = self.inspect_scroll.saturating_sub(page_size);
    }

    // ==================== Sessions Mode ====================

    /// Show the sessions list view.
    pub fn show_sessions(&mut self) {
        self.refresh_session_list();
        self.view_mode = ViewMode::Sessions;
        self.session_selected = 0;
    }

    /// Refresh the list of tmux sessions.
    pub fn refresh_session_list(&mut self) {
        if let Some(tmux) = &self.tmux {
            if let Ok(sessions) = tmux.list_sessions() {
                self.session_list = sessions.iter().map(|s| {
                    let is_commander = s.name.starts_with("commander-");
                    let is_connected = self.sessions.values().any(|n| n == &s.name);
                    SessionInfo {
                        name: s.name.clone(),
                        is_commander,
                        is_connected,
                    }
                }).collect();
            }
        }
    }

    /// Move selection up in sessions list.
    pub fn session_select_up(&mut self) {
        if self.session_selected > 0 {
            self.session_selected -= 1;
        }
    }

    /// Move selection down in sessions list.
    pub fn session_select_down(&mut self) {
        if self.session_selected < self.session_list.len().saturating_sub(1) {
            self.session_selected += 1;
        }
    }

    /// Connect to the currently selected session.
    pub fn connect_selected_session(&mut self) {
        if let Some(session) = self.session_list.get(self.session_selected) {
            if session.is_commander {
                // Extract project name from "commander-{name}"
                let project_name = session.name.strip_prefix("commander-")
                    .unwrap_or(&session.name).to_string();

                self.sessions.insert(project_name.clone(), session.name.clone());
                self.project = Some(project_name.clone());
                self.messages.push(Message::system(format!("Connected to '{}'", project_name)));
                self.view_mode = ViewMode::Normal;
            } else {
                self.messages.push(Message::system("Cannot connect to external session"));
            }
        }
    }

    /// Delete the currently selected session.
    pub fn delete_selected_session(&mut self) {
        if let Some(session) = self.session_list.get(self.session_selected).cloned() {
            if let Some(tmux) = &self.tmux {
                if let Err(e) = tmux.destroy_session(&session.name) {
                    self.messages.push(Message::system(format!("Failed to delete: {}", e)));
                } else {
                    // Remove from tracking if it was ours
                    if let Some(proj) = session.name.strip_prefix("commander-") {
                        self.sessions.remove(proj);
                        if self.project.as_deref() == Some(proj) {
                            self.project = None;
                        }
                    }
                    self.refresh_session_list();
                    // Adjust selection if needed
                    if self.session_selected >= self.session_list.len() && self.session_selected > 0 {
                        self.session_selected -= 1;
                    }
                }
            }
        }
    }

    /// Scroll to the bottom of the output.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Scroll up by one line.
    pub fn scroll_up(&mut self) {
        if self.scroll_offset < self.messages.len().saturating_sub(1) {
            self.scroll_offset += 1;
        }
    }

    /// Scroll down by one line.
    pub fn scroll_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    /// Scroll up by a page.
    pub fn scroll_page_up(&mut self, page_size: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(page_size)
            .min(self.messages.len().saturating_sub(1));
    }

    /// Scroll down by a page.
    pub fn scroll_page_down(&mut self, page_size: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(page_size);
    }

    /// Handle character input.
    pub fn enter_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
    }

    /// Delete character before cursor.
    pub fn delete_char(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.input.remove(self.cursor_pos);
        }
    }

    /// Move cursor left.
    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    /// Move cursor right.
    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos += 1;
        }
    }

    /// Clear the input.
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_pos = 0;
    }

    /// Submit the current input.
    pub fn submit(&mut self) {
        let input = std::mem::take(&mut self.input);
        self.cursor_pos = 0;

        if input.is_empty() {
            return;
        }

        // Handle commands
        if let Some(cmd) = input.strip_prefix('/') {
            self.handle_command(cmd);
        } else if self.project.is_some() {
            // Send to connected project
            if let Err(e) = self.send_message(&input) {
                self.messages.push(Message::system(format!("Error: {}", e)));
            }
        } else {
            self.messages.push(Message::system("Not connected. Use /connect <project>"));
        }
    }

    /// Handle a slash command.
    fn handle_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts[0].to_lowercase();
        let arg = parts.get(1).map(|s| s.trim());

        match command.as_str() {
            "help" | "h" | "?" => {
                self.messages.push(Message::system("Commands:"));
                self.messages.push(Message::system("  /connect <project>  - Connect to project"));
                self.messages.push(Message::system("  /disconnect         - Disconnect from project"));
                self.messages.push(Message::system("  /list               - List projects"));
                self.messages.push(Message::system("  /sessions           - Session picker (F3)"));
                self.messages.push(Message::system("  /inspect            - Toggle inspect mode (F2)"));
                self.messages.push(Message::system("  /stop [session]     - Stop session (commits changes, ends tmux)"));
                self.messages.push(Message::system("  /clear              - Clear output"));
                self.messages.push(Message::system("  /quit               - Exit TUI"));
                self.messages.push(Message::system(""));
                self.messages.push(Message::system("Keys: Up/Down scroll, F2 inspect, F3 sessions, Ctrl+C quit"));
            }
            "connect" | "c" => {
                if let Some(project) = arg {
                    if let Err(e) = self.connect(project) {
                        self.messages.push(Message::system(format!("Error: {}", e)));
                    }
                } else {
                    self.messages.push(Message::system("Usage: /connect <project>"));
                }
            }
            "disconnect" | "dc" => {
                self.disconnect();
            }
            "list" | "ls" | "l" => {
                match self.store.load_all_projects() {
                    Ok(projects) => {
                        if projects.is_empty() {
                            self.messages.push(Message::system("No projects found."));
                        } else {
                            self.messages.push(Message::system("Projects:"));
                            for project in projects.values() {
                                let marker = if Some(&project.name) == self.project.as_ref() {
                                    "*"
                                } else {
                                    " "
                                };
                                self.messages.push(Message::system(format!(
                                    "  {} {} ({:?})",
                                    marker, project.name, project.state
                                )));
                            }
                        }
                    }
                    Err(e) => {
                        self.messages.push(Message::system(format!("Error: {}", e)));
                    }
                }
            }
            "clear" => {
                self.messages.clear();
                self.messages.push(Message::system("Output cleared"));
            }
            "quit" | "q" | "exit" => {
                self.should_quit = true;
            }
            "stop" => {
                // Stop a session (commit git changes and destroy tmux)
                let target = arg.map(|s| s.to_string())
                    .or_else(|| self.project.clone());

                if let Some(name) = target {
                    self.stop_session(&name);
                } else {
                    self.messages.push(Message::system("Usage: /stop [session] or connect to a session first"));
                }
            }
            "inspect" => {
                self.toggle_inspect_mode();
            }
            "sessions" => {
                if self.tmux.is_some() {
                    self.show_sessions();
                } else {
                    self.messages.push(Message::system("Tmux not available"));
                }
            }
            _ => {
                self.messages.push(Message::system(format!("Unknown command: /{}", command)));
            }
        }
        self.scroll_to_bottom();
    }

    /// List available projects.
    pub fn list_projects(&self) -> Vec<String> {
        self.store.load_all_projects()
            .map(|p| p.values().map(|proj| proj.name.clone()).collect())
            .unwrap_or_default()
    }
}

/// Find new lines in tmux output by comparing previous and current captures.
fn find_new_lines(prev: &str, current: &str) -> Vec<String> {
    use std::collections::HashSet;

    let prev_lines: HashSet<&str> = prev.lines().collect();
    let mut new_lines = Vec::new();

    for line in current.lines() {
        let trimmed = line.trim();
        if !prev_lines.contains(line) && !prev_lines.contains(trimmed) && !trimmed.is_empty() {
            new_lines.push(line.to_string());
        }
    }

    new_lines
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
    fn test_find_new_lines() {
        let prev = "line1\nline2\n";
        let current = "line1\nline2\nline3\n";
        let new = find_new_lines(prev, current);
        assert_eq!(new, vec!["line3"]);
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
}
