//! Command handling for the TUI.
//!
//! Contains methods for processing slash commands and @ routing.

use super::app::{App, Message};
use super::connection::ConnectArgs;

impl App {
    /// Handle a slash command.
    pub(super) fn handle_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts[0].to_lowercase();
        let arg = parts.get(1).map(|s| s.trim());

        match command.as_str() {
            "help" | "h" | "?" => {
                self.messages.push(Message::system("=== TUI Commands ==="));
                self.messages.push(Message::system("  /connect <name>                    Connect to project or tmux session"));
                self.messages.push(Message::system("  /connect <path> -a <adapter> -n <name>  Start new project"));
                self.messages.push(Message::system("  /disconnect                        Disconnect from project"));
                self.messages.push(Message::system("  /list                              List sessions with activity"));
                self.messages.push(Message::system("    Indicators: [Claude] AI session, [Shell] plain shell, [?] unknown"));
                self.messages.push(Message::system("  /status [name]                     Show project status"));
                self.messages.push(Message::system("  /sessions                          Session picker (F3)"));
                self.messages.push(Message::system("  /inspect                           Toggle inspect mode (F2)"));
                self.messages.push(Message::system("  /stop [session]                    Stop session (commits git, ends tmux)"));
                self.messages.push(Message::system("  /rename <new-name>                 Rename current tmux session"));
                self.messages.push(Message::system("  /send <msg>                        Send message to connected session"));
                self.messages.push(Message::system("  /telegram                          Generate Telegram pairing code"));
                self.messages.push(Message::system("  /clear                             Clear output"));
                self.messages.push(Message::system(""));
                self.messages.push(Message::system("=== Message Routing ==="));
                self.messages.push(Message::system("  @alias message                     Send to specific session"));
                self.messages.push(Message::system("  @alias1 @alias2 message            Send to multiple sessions"));
                self.messages.push(Message::system("  /quit                              Exit TUI"));
                self.messages.push(Message::system(""));
                self.messages.push(Message::system("=== Adapters ==="));
                self.messages.push(Message::system("  cc, claude-code    Claude Code CLI"));
                self.messages.push(Message::system("  mpm                Claude MPM (multi-project manager)"));
                self.messages.push(Message::system(""));
                self.messages.push(Message::system("=== Filesystem (when connected) ==="));
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
                self.messages.push(Message::system("=== Keyboard ==="));
                self.messages.push(Message::system("  Up/Down     Command history"));
                self.messages.push(Message::system("  PgUp/PgDn   Scroll output"));
                self.messages.push(Message::system("  F2          Inspect mode (live tmux)"));
                self.messages.push(Message::system("  F3          Session picker"));
                self.messages.push(Message::system("  Ctrl+L      Clear output"));
                self.messages.push(Message::system("  Ctrl+C      Quit"));
                self.messages.push(Message::system(""));
                self.messages.push(Message::system("=== CLI ==="));
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
                let Some(tmux) = &self.tmux else {
                    self.messages.push(Message::system("Tmux not available."));
                    return;
                };

                let tmux_sessions = tmux.list_sessions().ok();

                if tmux_sessions.as_ref().map_or(true, |s| s.is_empty()) {
                    self.messages.push(Message::system("No sessions found."));
                } else if let Some(sessions) = tmux_sessions {
                    self.messages.push(Message::system("Sessions:"));
                    for session in &sessions {
                        let is_connected = self.sessions.values().any(|n| n == &session.name);
                        let connected_marker = if is_connected { " (connected)" } else { "" };

                        // Detect adapter type from screen content
                        let adapter = tmux.capture_output(&session.name, None, Some(50))
                            .map(|output| commander_core::detect_adapter(&output))
                            .unwrap_or(commander_core::Adapter::Unknown);
                        let indicator = adapter.indicator();

                        // Get activity summary for this session (now works for all types)
                        let activity = self.get_session_activity(&session.name, &adapter);

                        // Display name: strip commander- prefix for cleaner output
                        let display_name = session.name.strip_prefix("commander-")
                            .unwrap_or(&session.name);

                        self.messages.push(Message::system(format!(
                            "  {} {}{} - {}",
                            indicator, display_name, connected_marker, activity
                        )));
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
                // Priority: arg > connected project > current tmux session
                let target = arg.map(|s| s.to_string())
                    .or_else(|| self.project.clone())
                    .or_else(|| self.get_current_tmux_session());

                if let Some(name) = target {
                    // Check if we're stopping the session we're running in
                    let current_session = self.get_current_tmux_session();
                    let stopping_self = current_session.as_ref() == Some(&name)
                        || current_session.as_ref().map(|s| s == &format!("commander-{}", name)).unwrap_or(false);

                    if stopping_self {
                        self.messages.push(Message::system(format!("Stopping current session '{}'...", name)));
                        self.stop_session(&name);
                        // Note: If we're running inside this tmux session, the process will be killed
                    } else {
                        self.stop_session(&name);
                    }
                } else {
                    self.messages.push(Message::system("Usage: /stop [session] or connect to a session first"));
                }
            }
            "rename" => {
                // Rename the current tmux session
                if let Some(new_name) = arg {
                    self.rename_current_session(new_name);
                } else {
                    self.messages.push(Message::system("Usage: /rename <new-name>"));
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
    pub(super) fn show_status(&mut self, project_name: Option<&str>) {
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

                // Show session activity if running
                if session_exists {
                    if let Some(tmux) = &self.tmux {
                        if let Ok(output) = tmux.capture_output(&session_name, None, Some(100)) {
                            let summary = crate::repl::extract_session_summary(&output);
                            if !summary.is_empty() {
                                // Display activity as narrative (no "Activity:" label)
                                for line in summary {
                                    self.messages.push(Message::system(format!("  {}", line)));
                                }
                            } else {
                                let ready = commander_core::is_claude_ready(&output);
                                if ready {
                                    // Use LLM to interpret what Claude is waiting for
                                    self.show_interpreted_status(&output, true);
                                } else {
                                    // Use LLM to interpret what Claude is doing
                                    self.show_interpreted_status(&output, false);
                                }
                            }
                        }
                    }
                }
            }
            None => {
                self.messages.push(Message::system("No project specified. Use /status <project> or connect first."));
            }
        }
    }

    /// Show interpreted status using LLM analysis of screen content.
    fn show_interpreted_status(&mut self, screen_content: &str, is_ready: bool) {
        // Try LLM interpretation first
        if let Some(interpretation) = commander_core::interpret_screen_context(screen_content, is_ready) {
            self.messages.push(Message::system(format!("  {}", interpretation)));
        } else {
            // Fallback to simple status if LLM unavailable
            let fallback = if is_ready {
                "Waiting for input"
            } else {
                "Processing..."
            };
            self.messages.push(Message::system(format!("  {}", fallback)));
        }
    }

    /// Generate a Telegram pairing code.
    pub(super) fn generate_telegram_pairing(&mut self) {
        // Ensure telegram bot is running
        match crate::ensure_telegram_running() {
            Ok(crate::TelegramStartResult::AlreadyRunning) => {
                self.messages.push(Message::system("[ok] Telegram bot is running"));
            }
            Ok(crate::TelegramStartResult::Started) => {
                self.messages.push(Message::system("[ok] Telegram bot started"));
            }
            Ok(crate::TelegramStartResult::BuiltAndStarted) => {
                self.messages.push(Message::system("[ok] Built and started Telegram bot"));
            }
            Err(e) => {
                self.messages.push(Message::system(format!("[warn] Could not start Telegram bot: {}", e)));
                self.messages.push(Message::system("  Start manually: cargo run -p commander-telegram"));
            }
        }

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

    /// Handle @ routing syntax - send message to specific session(s).
    pub(super) fn handle_route(&mut self, input: &str) {
        // Parse targets and message using the REPL parser
        let cmd = crate::repl::ReplCommand::parse(input);

        match cmd {
            crate::repl::ReplCommand::Route { targets, message } => {
                if let Some(tmux) = &self.tmux {
                    let mut sent_count = 0;
                    let mut failed_targets = Vec::new();

                    // Get all session mappings
                    let sessions = self.sessions.clone();

                    for target in &targets {
                        // Look up session for this target
                        let session_name = if let Some(session) = sessions.get(target) {
                            Some(session.clone())
                        } else {
                            // Try commander- prefix
                            let prefixed = format!("commander-{}", target);
                            if tmux.session_exists(&prefixed) {
                                Some(prefixed)
                            } else if tmux.session_exists(target) {
                                Some(target.clone())
                            } else {
                                None
                            }
                        };

                        match session_name {
                            Some(session) => {
                                match tmux.send_line(&session, None, &message) {
                                    Ok(_) => {
                                        self.messages.push(Message::sent(
                                            format!("@{}", target),
                                            message.clone(),
                                        ));
                                        sent_count += 1;
                                    }
                                    Err(e) => {
                                        self.messages.push(Message::system(
                                            format!("[@{}] Failed: {}", target, e),
                                        ));
                                        failed_targets.push(target.clone());
                                    }
                                }
                            }
                            None => {
                                self.messages.push(Message::system(
                                    format!("[@{}] Session not found", target),
                                ));
                                failed_targets.push(target.clone());
                            }
                        }
                    }

                    if sent_count > 0 && targets.len() > 1 {
                        self.messages.push(Message::system(
                            format!("Sent to {} session(s)", sent_count),
                        ));
                    }
                    if !failed_targets.is_empty() {
                        self.messages.push(Message::system("Use /sessions to see available sessions"));
                    }
                } else {
                    self.messages.push(Message::system("Tmux not available"));
                }
            }
            crate::repl::ReplCommand::Status(Some(target)) => {
                // @alias with no message - show status
                self.show_status(Some(&target));
            }
            _ => {
                self.messages.push(Message::system("Invalid @ routing syntax. Use: @alias message"));
            }
        }
        self.scroll_to_bottom();
    }

    /// Get activity summary for a session.
    ///
    /// Works for all session types:
    /// - Claude sessions: extracts task status, waiting state, etc.
    /// - Shell sessions: shows current directory, last command, or running process
    fn get_session_activity(&self, session_name: &str, adapter: &commander_core::Adapter) -> String {
        let Some(tmux) = &self.tmux else {
            return "Idle".to_string();
        };

        // Capture recent output from the session
        let output = match tmux.capture_output(session_name, None, Some(100)) {
            Ok(out) => out,
            Err(_) => return "Unable to read session".to_string(),
        };

        match adapter {
            commander_core::Adapter::Claude => {
                // Claude Code session - use existing logic
                let is_ready = commander_core::is_claude_ready(&output);

                // Try to extract session summary from deterministic patterns first
                let summary = crate::repl::extract_session_summary(&output);
                if !summary.is_empty() {
                    return truncate_preview(&summary[0], 50);
                }

                if is_ready {
                    let preview = super::helpers::extract_ready_preview(&output);
                    if !preview.is_empty() && preview.len() < 60 {
                        return format!("Waiting ({})", truncate_preview(&preview, 40));
                    }
                    return "Waiting for input".to_string();
                }

                "Processing...".to_string()
            }
            commander_core::Adapter::Shell => {
                // Shell session - extract useful info from output
                extract_shell_activity(&output)
            }
            commander_core::Adapter::Unknown => {
                "Active".to_string()
            }
        }
    }
}

/// Extract activity summary for a shell session.
///
/// Looks for:
/// - Current directory from prompt
/// - Last command if visible
/// - Running process indicators
fn extract_shell_activity(output: &str) -> String {
    let lines: Vec<&str> = output.lines().rev()
        .filter(|l| !l.trim().is_empty())
        .take(10)
        .collect();

    if lines.is_empty() {
        return "Idle".to_string();
    }

    // Look for prompt line to extract current directory
    for line in &lines {
        let trimmed = line.trim();

        // Pattern: user@host:~/path$
        if let Some(colon_pos) = trimmed.find(':') {
            if trimmed.ends_with('$') || trimmed.ends_with("$ ") {
                let path_part = &trimmed[colon_pos + 1..];
                let path = path_part.trim_end_matches('$').trim_end_matches("$ ").trim();
                if !path.is_empty() && path.len() < 50 {
                    return format!("In: {}", path);
                }
            }
        }

        // Pattern: ~/path $ or /path $
        if (trimmed.starts_with('~') || trimmed.starts_with('/'))
            && (trimmed.ends_with('$') || trimmed.ends_with("$ "))
        {
            let path = trimmed.trim_end_matches('$').trim_end_matches("$ ").trim();
            if !path.is_empty() && path.len() < 50 {
                return format!("In: {}", path);
            }
        }
    }

    // Look for running commands (common patterns)
    for line in &lines {
        let lower = line.to_lowercase();

        // Common running process indicators
        if lower.contains("running") || lower.contains("installing") {
            return truncate_preview(line.trim(), 50);
        }

        // npm/yarn/cargo/git operations
        if lower.starts_with("npm ") || lower.starts_with("yarn ") {
            return format!("Running: {}", truncate_preview(line.trim(), 40));
        }
        if lower.starts_with("cargo ") || lower.starts_with("git ") {
            return format!("Running: {}", truncate_preview(line.trim(), 40));
        }
    }

    "Active".to_string()
}

/// Truncate a preview string to fit in the list display.
fn truncate_preview(s: &str, max_len: usize) -> String {
    let s = s.trim();
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
