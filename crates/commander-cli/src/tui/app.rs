//! TUI application state and logic.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use commander_adapters::AdapterRegistry;
use commander_persistence::StateStore;
use commander_tmux::TmuxOrchestrator;

use crate::filesystem;

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

/// Parsed connect command arguments.
#[derive(Debug)]
enum ConnectArgs {
    /// Connect to existing project by name
    Existing(String),
    /// Create and connect to new project
    New { path: String, adapter: String, name: String },
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
    response_buffer: Vec<String>,
    /// When output last changed (for idle detection)
    last_activity: Option<Instant>,
    /// Receiver for async summarization result
    summarizer_rx: Option<mpsc::Receiver<String>>,
    /// Whether we're currently summarizing
    is_summarizing: bool,
    /// The user's original query (for context in summarization)
    pending_query: Option<String>,

    // Command history
    /// Previous commands
    command_history: Vec<String>,
    /// Current position in history (None = new input, Some(i) = browsing history)
    history_index: Option<usize>,
    /// Saved input when browsing history
    saved_input: String,

    // Tab completion
    /// Cached completions for current input prefix
    completions: Vec<String>,
    /// Current completion index (None = not in completion mode)
    completion_index: Option<usize>,
}

impl App {
    /// Create a new App instance.
    pub fn new(state_dir: &std::path::Path) -> Self {
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
                self.project_path = Some(project.path.clone());
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

                // Create tmux session in project directory
                if let Err(e) = tmux.create_session_in_dir(&session_name, Some(&project.path)) {
                    return Err(format!("Failed to create tmux session: {}", e));
                }

                // Send launch command
                if let Err(e) = tmux.send_line(&session_name, None, &full_cmd) {
                    return Err(format!("Failed to start adapter: {}", e));
                }

                self.sessions.insert(project.name.clone(), session_name);
                self.project = Some(project.name.clone());
                self.project_path = Some(project.path.clone());
                self.messages.push(Message::system(format!("Started and connected to '{}'", project.name)));
                return Ok(());
            }
        }

        Err("Tmux not available".to_string())
    }

    /// Parse connect command arguments.
    fn parse_connect_args(&self, arg: &str) -> Result<ConnectArgs, String> {
        let parts: Vec<&str> = arg.split_whitespace().collect();

        if parts.is_empty() {
            return Err("connect requires arguments".to_string());
        }

        // Check if this has -a or -n flags (new project syntax)
        if parts.iter().any(|&p| p == "-a" || p == "-n") {
            let path = shellexpand::tilde(parts[0]).to_string();
            let mut adapter = None;
            let mut name = None;

            let mut i = 1;
            while i < parts.len() {
                match parts[i] {
                    "-a" => {
                        if i + 1 < parts.len() {
                            adapter = Some(parts[i + 1].to_string());
                            i += 2;
                        } else {
                            return Err("-a requires an adapter (cc, mpm)".to_string());
                        }
                    }
                    "-n" => {
                        if i + 1 < parts.len() {
                            name = Some(parts[i + 1].to_string());
                            i += 2;
                        } else {
                            return Err("-n requires a project name".to_string());
                        }
                    }
                    _ => {
                        return Err(format!("unknown flag: {}", parts[i]));
                    }
                }
            }

            match (adapter, name) {
                (Some(a), Some(n)) => Ok(ConnectArgs::New { path, adapter: a, name: n }),
                (None, _) => Err("missing -a <adapter> (cc, mpm)".to_string()),
                (_, None) => Err("missing -n <name>".to_string()),
            }
        } else if parts.len() == 1 {
            // Existing project by name
            Ok(ConnectArgs::Existing(parts[0].to_string()))
        } else {
            Err("use '/connect <name>' or '/connect <path> -a <adapter> -n <name>'".to_string())
        }
    }

    /// Connect to a new project (create and start).
    pub fn connect_new(&mut self, path: &str, adapter: &str, name: &str) -> Result<(), String> {
        // Resolve adapter alias
        let tool_id = self.registry.resolve(adapter)
            .ok_or_else(|| format!("Unknown adapter: {}. Use: cc (claude-code), mpm", adapter))?
            .to_string();

        // Check if project already exists
        let projects = self.store.load_all_projects()
            .map_err(|e| format!("Failed to load projects: {}", e))?;

        if projects.values().any(|p| p.name == name) {
            return Err(format!("Project '{}' already exists. Use /connect {}", name, name));
        }

        // Create project
        let mut project = commander_models::Project::new(path, name);
        project.config.insert("tool".to_string(), serde_json::json!(tool_id));

        // Save project
        self.store.save_project(&project)
            .map_err(|e| format!("Failed to save project: {}", e))?;

        // Connect to the new project
        self.connect(name)
    }

    /// Disconnect from current project.
    pub fn disconnect(&mut self) {
        if let Some(project) = self.project.take() {
            self.project_path = None;
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
                Ok(Some(true)) => self.messages.push(Message::system("Changes committed.")),
                Ok(Some(false)) => self.messages.push(Message::system("No changes to commit.")),
                Ok(None) => self.messages.push(Message::system("Not a git repository, skipping commit.")),
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

    /// Check if path is inside a git worktree.
    fn is_git_worktree(path: &str) -> bool {
        std::process::Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false)
    }

    /// Commit any uncommitted git changes in the project directory.
    /// Returns Ok(None) if not a git repository, Ok(Some(true)) if committed,
    /// Ok(Some(false)) if no changes, or Err on failure.
    fn git_commit_changes(&self, path: &str, project_name: &str) -> Result<Option<bool>, String> {
        use std::process::Command;

        // Skip git operations if not in a git worktree
        if !Self::is_git_worktree(path) {
            return Ok(None);
        }

        // Check if there are changes
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to run git status: {}", e))?;

        let changes = String::from_utf8_lossy(&status.stdout);
        if changes.trim().is_empty() {
            return Ok(Some(false)); // No changes
        }

        // Stage all changes
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to stage changes: {}", e))?;

        // Commit with message (may fail if pre-commit hooks modify files)
        let message = format!("WIP: Auto-commit from Commander session '{}'", project_name);
        let commit = Command::new("git")
            .args(["commit", "-m", &message])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to commit: {}", e))?;

        if commit.status.success() {
            return Ok(Some(true));
        }

        // Pre-commit hooks may have modified files - re-stage and retry
        let stdout = String::from_utf8_lossy(&commit.stdout);
        if stdout.contains("Passed") || stdout.contains("Fixed") || stdout.contains("trailing whitespace") {
            // Hooks ran and fixed things - re-stage and commit again
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(path)
                .output()
                .map_err(|e| format!("Failed to re-stage changes: {}", e))?;

            let retry = Command::new("git")
                .args(["commit", "-m", &message])
                .current_dir(path)
                .output()
                .map_err(|e| format!("Failed to commit after hooks: {}", e))?;

            if retry.status.success() {
                return Ok(Some(true));
            }

            // Check if nothing to commit after hooks
            let status2 = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(path)
                .output()
                .ok();

            if let Some(s) = status2 {
                if String::from_utf8_lossy(&s.stdout).trim().is_empty() {
                    return Ok(Some(false)); // Hooks fixed everything, nothing to commit
                }
            }

            let stderr = String::from_utf8_lossy(&retry.stderr);
            Err(format!("Commit failed after hooks: {}", stderr))
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

        // Add sent message to output and reset response collection
        self.messages.push(Message::sent(project.clone(), message));
        self.pending_query = Some(message.to_string());
        self.response_buffer.clear();
        self.last_activity = Some(Instant::now());
        self.is_working = true;
        self.is_summarizing = false;
        self.progress = 0.0;
        self.scroll_to_bottom();

        Ok(())
    }

    /// Poll for new output from tmux and trigger summarization when idle.
    pub fn poll_output(&mut self) {
        // Check for summarization results first
        if let Some(rx) = &self.summarizer_rx {
            if let Ok(summary) = rx.try_recv() {
                // Got summary result
                if let Some(project) = &self.project {
                    self.messages.push(Message::received(project.clone(), summary));
                }
                self.summarizer_rx = None;
                self.is_summarizing = false;
                self.is_working = false;
                self.response_buffer.clear();
                self.pending_query = None;
                self.scroll_to_bottom();
                return;
            }
        }

        if !self.is_working || self.is_summarizing {
            // Update progress animation if summarizing
            if self.is_summarizing {
                self.progress = (self.progress + 0.03) % 1.0;
            }
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

        // Check for new content
        if current_output != self.last_output {
            let new_lines = find_new_lines(&self.last_output, &current_output);
            for line in new_lines {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    self.response_buffer.push(trimmed.to_string());
                }
            }
            self.last_output = current_output.clone();
            self.last_activity = Some(Instant::now());
        }

        // Check if Claude Code is idle (prompt visible and no activity for 1.5s)
        let is_idle = self.last_activity
            .map(|t| t.elapsed().as_millis() > 1500)
            .unwrap_or(false);

        // Check for various Claude Code idle patterns
        let has_prompt = is_claude_code_ready(&current_output);

        if is_idle && has_prompt && !self.response_buffer.is_empty() {
            // Trigger summarization
            self.trigger_summarization();
        }

        // Update progress animation
        self.progress = (self.progress + 0.05) % 1.0;
    }

    /// Trigger async summarization of the response buffer.
    fn trigger_summarization(&mut self) {
        let raw_response = self.response_buffer.join("\n");
        let query = self.pending_query.clone().unwrap_or_default();

        // Set summarizing state (status bar will show it)
        self.is_summarizing = true;

        // Create channel for result
        let (tx, rx) = mpsc::channel();
        self.summarizer_rx = Some(rx);

        // Spawn thread for blocking HTTP call
        std::thread::spawn(move || {
            let summary = summarize_response_blocking(&query, &raw_response);
            let _ = tx.send(summary);
        });
    }

    /// Stop the working indicator.
    pub fn stop_working(&mut self) {
        self.is_working = false;
        self.is_summarizing = false;
        self.progress = 0.0;
        self.response_buffer.clear();
        self.pending_query = None;
    }

    /// Check if currently summarizing.
    pub fn is_summarizing(&self) -> bool {
        self.is_summarizing
    }

    /// Get the number of lines in the response buffer.
    pub fn response_buffer_len(&self) -> usize {
        self.response_buffer.len()
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

                // Look up project path
                let path = self.store.load_all_projects().ok()
                    .and_then(|projects| {
                        projects.values()
                            .find(|p| p.name == project_name)
                            .map(|p| p.path.clone())
                    });

                self.sessions.insert(project_name.clone(), session.name.clone());
                self.project = Some(project_name.clone());
                self.project_path = path;
                self.messages.push(Message::system(format!("Connected to '{}'", project_name)));
                self.view_mode = ViewMode::Normal;
            } else {
                // Connect to external session (use session name as project name)
                let project_name = session.name.clone();
                self.sessions.insert(project_name.clone(), session.name.clone());
                self.project = Some(project_name.clone());
                self.project_path = None;  // No project path for external sessions
                self.messages.push(Message::system(format!("Connected to external session '{}'", project_name)));
                self.view_mode = ViewMode::Normal;
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
        self.history_index = None;
        self.saved_input.clear();

        if input.is_empty() {
            return;
        }

        // Add to history (avoid duplicates of last entry)
        if self.command_history.last() != Some(&input) {
            self.command_history.push(input.clone());
        }

        // Handle commands
        if let Some(cmd) = input.strip_prefix('/') {
            self.handle_command(cmd);
        } else if self.project.is_some() {
            // Check for filesystem commands first
            let working_dir = self.project_path.as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

            if let Some(fs_cmd) = filesystem::parse_command(&input, &working_dir) {
                // Execute filesystem command locally
                let result = filesystem::execute(&fs_cmd, &working_dir);
                let project = self.project.clone().unwrap_or_default();

                self.messages.push(Message::sent(project.clone(), input.clone()));

                if result.success {
                    self.messages.push(Message::received(project.clone(), result.message));
                    if let Some(details) = result.details {
                        for line in details.lines() {
                            self.messages.push(Message::received(project.clone(), line.to_string()));
                        }
                    }
                } else {
                    self.messages.push(Message::system(format!("Error: {}", result.message)));
                }
                self.scroll_to_bottom();
            } else {
                // Send to connected project
                if let Err(e) = self.send_message(&input) {
                    self.messages.push(Message::system(format!("Error: {}", e)));
                }
            }
        } else {
            self.messages.push(Message::system("Not connected. Use /connect <project>"));
        }
    }

    /// Navigate to previous command in history (Up arrow).
    pub fn history_prev(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // First time pressing up - save current input and go to last history item
                self.saved_input = std::mem::take(&mut self.input);
                self.history_index = Some(self.command_history.len() - 1);
                self.input = self.command_history.last().cloned().unwrap_or_default();
            }
            Some(idx) if idx > 0 => {
                // Move to earlier history
                self.history_index = Some(idx - 1);
                self.input = self.command_history.get(idx - 1).cloned().unwrap_or_default();
            }
            _ => {
                // Already at oldest entry
            }
        }
        self.cursor_pos = self.input.len();
    }

    /// Navigate to next command in history (Down arrow).
    pub fn history_next(&mut self) {
        match self.history_index {
            Some(idx) => {
                if idx + 1 < self.command_history.len() {
                    // Move to more recent history
                    self.history_index = Some(idx + 1);
                    self.input = self.command_history.get(idx + 1).cloned().unwrap_or_default();
                } else {
                    // Return to saved input
                    self.history_index = None;
                    self.input = std::mem::take(&mut self.saved_input);
                }
                self.cursor_pos = self.input.len();
            }
            None => {
                // Not in history mode
            }
        }
    }

    /// Handle a slash command.
    fn handle_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts[0].to_lowercase();
        let arg = parts.get(1).map(|s| s.trim());

        match command.as_str() {
            "help" | "h" | "?" => {
                self.messages.push(Message::system("═══ TUI Commands ═══"));
                self.messages.push(Message::system("  /connect <name>                    Connect to existing project"));
                self.messages.push(Message::system("  /connect <path> -a <adapter> -n <name>  Start new project"));
                self.messages.push(Message::system("  /disconnect                        Disconnect from project"));
                self.messages.push(Message::system("  /list                              List projects"));
                self.messages.push(Message::system("  /status [name]                     Show project status"));
                self.messages.push(Message::system("  /sessions                          Session picker (F3)"));
                self.messages.push(Message::system("  /inspect                           Toggle inspect mode (F2)"));
                self.messages.push(Message::system("  /stop [session]                    Stop session (commits git, ends tmux)"));
                self.messages.push(Message::system("  /send <msg>                        Send message to connected session"));
                self.messages.push(Message::system("  /telegram                          Generate Telegram pairing code"));
                self.messages.push(Message::system("  /clear                             Clear output"));
                self.messages.push(Message::system("  /quit                              Exit TUI"));
                self.messages.push(Message::system(""));
                self.messages.push(Message::system("═══ Adapters ═══"));
                self.messages.push(Message::system("  cc, claude-code    Claude Code CLI"));
                self.messages.push(Message::system("  mpm                Claude MPM (multi-project manager)"));
                self.messages.push(Message::system(""));
                self.messages.push(Message::system("═══ Filesystem (when connected) ═══"));
                self.messages.push(Message::system("  ls, list [path]    List directory"));
                self.messages.push(Message::system("  cat, read <file>   Read file contents"));
                self.messages.push(Message::system("  head/tail <file>   First/last lines"));
                self.messages.push(Message::system("  find <pattern>     Search for files"));
                self.messages.push(Message::system("  mkdir [-p] <dir>   Create directory"));
                self.messages.push(Message::system("  touch <file>       Create empty file"));
                self.messages.push(Message::system("  mv <src> <dst>     Move/rename"));
                self.messages.push(Message::system("  cp <src> <dst>     Copy file/dir"));
                self.messages.push(Message::system("  rm [-f] <path>     Delete file/dir"));
                self.messages.push(Message::system("  pwd                Show working directory"));
                self.messages.push(Message::system(""));
                self.messages.push(Message::system("═══ Keyboard ═══"));
                self.messages.push(Message::system("  Up/Down     Command history"));
                self.messages.push(Message::system("  PgUp/PgDn   Scroll output"));
                self.messages.push(Message::system("  F2          Inspect mode (live tmux)"));
                self.messages.push(Message::system("  F3          Session picker"));
                self.messages.push(Message::system("  Ctrl+L      Clear output"));
                self.messages.push(Message::system("  Ctrl+C      Quit"));
                self.messages.push(Message::system(""));
                self.messages.push(Message::system("═══ CLI ═══"));
                self.messages.push(Message::system("  commander                          Launch TUI (default)"));
                self.messages.push(Message::system("  commander -v                       Verbose mode (-vv, -vvv)"));
                self.messages.push(Message::system("  commander tui -p <name>            TUI with auto-connect"));
                self.messages.push(Message::system("  commander repl                     Launch REPL"));
                self.messages.push(Message::system("  commander list                     List projects"));
                self.messages.push(Message::system("  commander adapters                 Show adapters"));
            }
            "connect" | "c" => {
                if let Some(arg_str) = arg {
                    // Parse connect arguments
                    match self.parse_connect_args(arg_str) {
                        Ok(ConnectArgs::Existing(name)) => {
                            if let Err(e) = self.connect(&name) {
                                self.messages.push(Message::system(format!("Error: {}", e)));
                            }
                        }
                        Ok(ConnectArgs::New { path, adapter, name }) => {
                            if let Err(e) = self.connect_new(&path, &adapter, &name) {
                                self.messages.push(Message::system(format!("Error: {}", e)));
                            }
                        }
                        Err(e) => {
                            self.messages.push(Message::system(format!("Error: {}", e)));
                        }
                    }
                } else {
                    self.messages.push(Message::system("Usage: /connect <name> or /connect <path> -a <adapter> -n <name>"));
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
            "status" | "s" => {
                self.show_status(arg);
            }
            "telegram" => {
                self.generate_telegram_pairing();
            }
            "send" => {
                if let Some(message) = arg {
                    if let Err(e) = self.send_message(message) {
                        self.messages.push(Message::system(format!("Error: {}", e)));
                    }
                } else {
                    self.messages.push(Message::system("Usage: /send <message>"));
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

    /// Show status for a project.
    fn show_status(&mut self, project_name: Option<&str>) {
        let name = project_name
            .map(String::from)
            .or_else(|| self.project.clone());

        match name {
            Some(name) => {
                // Check if session exists
                let session_name = format!("commander-{}", name);
                let session_exists = self.tmux.as_ref()
                    .map(|t| t.session_exists(&session_name))
                    .unwrap_or(false);

                // Get project info from store
                let project_info = self.store.load_all_projects().ok()
                    .and_then(|projects| {
                        projects.values()
                            .find(|p| p.name == name)
                            .cloned()
                    });

                self.messages.push(Message::system(format!("Status: {}", name)));

                if let Some(info) = project_info {
                    self.messages.push(Message::system(format!("  Path: {}", info.path)));
                    let adapter = info.config.get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    self.messages.push(Message::system(format!("  Adapter: {}", adapter)));
                }

                let status = if session_exists { "Running" } else { "Stopped" };
                self.messages.push(Message::system(format!("  Session: {}", status)));

                if self.project.as_ref() == Some(&name) {
                    self.messages.push(Message::system("  Connected: Yes"));
                }
            }
            None => {
                self.messages.push(Message::system("No project specified. Use /status <project> or connect first."));
            }
        }
    }

    /// Generate a Telegram pairing code.
    fn generate_telegram_pairing(&mut self) {
        let (project_name, session_name) = match &self.project {
            Some(p) => (p.clone(), format!("commander-{}", p)),
            None => (String::new(), String::new()),
        };

        match commander_telegram::create_pairing(&project_name, &session_name) {
            Ok(code) => {
                self.messages.push(Message::system("Telegram Pairing Code"));
                self.messages.push(Message::system(format!("  Code: {}", code)));
                self.messages.push(Message::system(format!("  In Telegram: /pair {}", code)));
                self.messages.push(Message::system("  Expires in 5 minutes"));
                if !project_name.is_empty() {
                    self.messages.push(Message::system(format!("  Auto-connects to: {}", project_name)));
                }
            }
            Err(e) => {
                self.messages.push(Message::system(format!("Error generating pairing code: {}", e)));
            }
        }
    }

    // ==================== Tab Completion ====================

    /// Available slash commands for completion.
    const COMMANDS: &'static [&'static str] = &[
        "/clear", "/connect", "/disconnect", "/help", "/inspect",
        "/list", "/quit", "/send", "/sessions", "/status", "/stop",
        "/telegram",
    ];

    /// Perform tab completion on the current input.
    pub fn complete_command(&mut self) {
        // Only complete if input starts with /
        if !self.input.starts_with('/') {
            self.completions.clear();
            self.completion_index = None;
            return;
        }

        // Build completions if not already built for this prefix
        if self.completions.is_empty() || self.completion_index.is_none() {
            self.completions = Self::COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(self.input.as_str()))
                .map(|s| s.to_string())
                .collect();
            self.completion_index = if self.completions.is_empty() {
                None
            } else {
                Some(0)
            };
        } else {
            // Cycle through completions
            if let Some(idx) = self.completion_index {
                self.completion_index = Some((idx + 1) % self.completions.len());
            }
        }

        // Apply completion
        if let Some(idx) = self.completion_index {
            if let Some(completion) = self.completions.get(idx) {
                self.input = completion.clone();
                self.cursor_pos = self.input.len();
            }
        }
    }

    /// Reset completion state (called when input changes).
    pub fn reset_completions(&mut self) {
        self.completions.clear();
        self.completion_index = None;
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
            // Filter out Claude Code UI noise
            if !is_ui_noise(trimmed) {
                new_lines.push(line.to_string());
            }
        }
    }

    new_lines
}

/// Check if Claude Code is ready for input (idle at prompt).
fn is_claude_code_ready(output: &str) -> bool {
    // Get the last few non-empty lines
    let lines: Vec<&str> = output.lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(10)
        .collect();

    if lines.is_empty() {
        return false;
    }

    // Pattern 1: Line contains just the prompt character ❯
    // Claude Code shows "❯ " when ready for input
    for line in &lines[..lines.len().min(3)] {
        let trimmed = line.trim();
        if trimmed == "❯" || trimmed == "❯ " {
            return true;
        }
        // Also check for prompt at end of line (after path)
        if trimmed.ends_with(" ❯") || trimmed.ends_with(" ❯ ") {
            return true;
        }
    }

    // Pattern 2: The input box separator lines
    // Claude Code shows ──────────── above and below input
    let has_separator = lines.iter().take(5).any(|l| {
        let trimmed = l.trim();
        trimmed.starts_with("───") || trimmed.starts_with("╭─") || trimmed.starts_with("╰─")
    });

    // Pattern 3: "bypass permissions" hint shown at prompt
    let has_bypass_hint = lines.iter().take(5).any(|l| {
        l.contains("bypass permissions")
    });

    // Pattern 4: Empty prompt box (two separators with nothing between)
    if has_separator {
        // Check if we see the prompt structure
        for (i, line) in lines.iter().enumerate() {
            if line.contains("❯") && i < 5 {
                return true;
            }
        }
    }

    // Pattern 5: Check for common ready indicators
    let has_ready_indicator = lines.iter().take(3).any(|l| {
        let trimmed = l.trim();
        // Empty input prompt
        trimmed == "│ ❯" ||
        trimmed.starts_with("│ ❯") ||
        // Just the chevron
        trimmed == ">" ||
        trimmed.ends_with("> ") ||
        // Explicit ready state
        trimmed.contains("[ready]")
    });

    has_ready_indicator || has_bypass_hint
}

/// Check if a line is Claude Code UI noise that should be filtered out.
fn is_ui_noise(line: &str) -> bool {
    // Prompt lines - echoed user input from Claude Code
    // Matches: [project] ❯ text, [project] > text, project> text
    if line.contains("] ❯ ") || line.contains("] > ") {
        return true;
    }
    // Also matches bare prompt at start: project>
    if line.chars().take(30).collect::<String>().contains("> ")
        && !line.contains(':')
        && !line.contains("http") {
        // Looks like a prompt echo, not content
        if let Some(pos) = line.find("> ") {
            let before = &line[..pos];
            // If it's just a word before >, it's likely a prompt
            if !before.contains(' ') || before.starts_with('[') {
                return true;
            }
        }
    }

    // Spinner characters and thinking indicators
    let spinners = ['✳', '✶', '✻', '✽', '✢', '⏺', '·', '●', '○', '◐', '◑', '◒', '◓'];
    if line.chars().next().map(|c| spinners.contains(&c)).unwrap_or(false) {
        return true;
    }

    // Status bar box drawing characters
    if line.starts_with('╰') || line.starts_with('╭') || line.starts_with('│')
        || line.starts_with('├') || line.starts_with('└') || line.starts_with('┌')
        || line.starts_with('┐') || line.starts_with('┘') || line.starts_with('┤')
        || line.starts_with('┬') || line.starts_with('┴') || line.starts_with('┼') {
        return true;
    }

    // Claude Code branding and UI
    if line.contains("▐▛") || line.contains("▜▌") || line.contains("▝▜") || line.contains("▛▘") {
        return true;
    }

    // Thinking/processing indicators
    let lower = line.to_lowercase();
    if lower.contains("spelunking") || lower.contains("(thinking)")
        || lower.contains("thinking…") || lower.contains("thinking...") {
        return true;
    }

    // Status messages that are UI noise
    if lower.contains("ctrl+b") || lower.contains("to run in background") {
        return true;
    }

    // Claude Code version/branding line
    if lower.contains("claude code v") || lower.contains("claude max")
        || lower.contains("opus 4") || lower.contains("sonnet") {
        return true;
    }

    // MCP tool invocation noise (keep the result, not the invocation)
    if line.contains("(MCP)(") && (line.contains("owner:") || line.contains("repo:")) {
        return true;
    }

    // Agent/task headers that are noise
    if line.ends_with("(MCP)") && !line.contains(':') {
        return true;
    }

    false
}

/// Blocking HTTP call to summarize a response via OpenRouter.
fn summarize_response_blocking(query: &str, raw_response: &str) -> String {
    use std::env;

    let api_key = match env::var("OPENROUTER_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            // No API key - return cleaned raw response
            return clean_raw_response(raw_response);
        }
    };

    let model = env::var("OPENROUTER_MODEL")
        .unwrap_or_else(|_| "anthropic/claude-sonnet-4".to_string());

    let system_prompt = r#"You are a response summarizer for Commander, an AI orchestration tool.
Your job is to take raw output from Claude Code and summarize it conversationally.

Rules:
- Be concise but informative (2-4 sentences for simple responses, more for complex ones)
- Focus on what was DONE or LEARNED, not the process
- Skip UI noise, file listings, and verbose tool output
- If code was written, summarize what it does
- If a question was answered, give the key answer
- Use natural language, not bullet points unless listing multiple items
- Never say "Claude Code" or mention the underlying tool"#;

    let user_prompt = format!(
        "User asked: {}\n\nRaw response:\n{}\n\nProvide a conversational summary:",
        query, raw_response
    );

    // Build request
    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt}
        ],
        "max_tokens": 500
    });

    // Make blocking HTTP request
    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send();

    match response {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>() {
                if let Some(content) = json["choices"][0]["message"]["content"].as_str() {
                    return content.to_string();
                }
            }
            // Fallback to cleaned raw response
            clean_raw_response(raw_response)
        }
        Err(_) => {
            // Fallback to cleaned raw response
            clean_raw_response(raw_response)
        }
    }
}

/// Clean raw response when summarization isn't available.
fn clean_raw_response(raw: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        // Skip obvious noise
        if trimmed.is_empty()
            || trimmed.starts_with("⎿")
            || trimmed.starts_with("⏺")
            || trimmed.contains("hook")
            || trimmed.contains("ctrl+o")
            || trimmed.contains("(MCP)")
            || trimmed.starts_with("Reading")
            || trimmed.starts_with("Searched")
        {
            continue;
        }
        lines.push(trimmed);
    }
    lines.join("\n")
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
    fn test_find_new_lines_filters_prompt_echo() {
        let prev = "";
        let current = "[duetto] ❯ describe this project\nActual response here\n";
        let new = find_new_lines(prev, current);
        assert_eq!(new, vec!["Actual response here"]);
    }

    #[test]
    fn test_is_ui_noise_prompt_lines() {
        assert!(is_ui_noise("[duetto] ❯ some command"));
        assert!(is_ui_noise("[project] > test input"));
        assert!(is_ui_noise("duetto> hello"));
        assert!(!is_ui_noise("This is actual content"));
        assert!(!is_ui_noise("Response: here is the answer"));
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
