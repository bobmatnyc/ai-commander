//! Session management for the TUI.
//!
//! Contains methods for monitoring session status, scanning sessions,
//! and managing the sessions list view.

use std::collections::HashMap;
use std::time::Instant;

use commander_core::is_claude_ready;

use super::app::{App, Message, SessionInfo, ViewMode};
use super::helpers::extract_ready_preview;

impl App {
    /// Check session status and notify when sessions become ready for input.
    pub fn check_session_status(&mut self) {
        // Rate limit checks to every 5 seconds (was 2, increased to reduce noise)
        let now = Instant::now();
        if let Some(last_check) = self.last_status_check {
            if now.duration_since(last_check).as_secs() < 5 {
                return;
            }
        }
        self.last_status_check = Some(now);

        // Skip if no tmux or currently working
        if self.tmux.is_none() || self.is_working {
            return;
        }

        // Collect session info and status to avoid borrow issues
        let sessions_to_check: Vec<_> = self.sessions.iter()
            .map(|(name, session)| (name.clone(), session.clone()))
            .collect();

        let connected_project = self.project.clone();

        // Check each session and collect notifications
        let mut notifications: Vec<(String, bool, String)> = Vec::new();
        let mut state_updates: Vec<(String, bool)> = Vec::new();

        if let Some(tmux) = &self.tmux {
            for (name, session) in sessions_to_check {
                if let Ok(output) = tmux.capture_output(&session, None, Some(50)) {
                    let is_ready = is_claude_ready(&output);

                    // Check if we have prior state - if not, just record current state
                    // without notifying (avoids false positives on startup)
                    let has_prior_state = self.session_ready_state.contains_key(&name);
                    let was_ready = self.session_ready_state.get(&name).copied().unwrap_or(true);

                    // Only notify on actual transitions (not-ready -> ready)
                    // AND only if we had prior state (not first observation)
                    if has_prior_state && is_ready && !was_ready {
                        let preview = extract_ready_preview(&output);
                        let is_connected = connected_project.as_ref() == Some(&name);
                        notifications.push((name.clone(), is_connected, preview));
                    }

                    state_updates.push((name, is_ready));
                }
            }
        }

        // Apply notifications
        let mut should_scroll = false;
        for (name, is_connected, preview) in notifications {
            let msg = if is_connected {
                format!("[inbox] {} is ready", name)
            } else {
                format!("[inbox] @{} is ready", name)
            };
            // Only add preview if it's meaningful
            let full_msg = if preview.is_empty() {
                msg
            } else {
                format!("{}: {}", msg, preview)
            };
            self.messages.push(Message::system(full_msg.clone()));
            should_scroll = true;

            // Broadcast to all channels (Telegram, etc.)
            // Use the actual session name (might be commander-prefixed or not)
            let session_name = self.sessions.get(&name)
                .cloned()
                .unwrap_or_else(|| format!("commander-{}", name));
            if let Err(e) = commander_telegram::notify_session_ready(
                &session_name,
                if preview.is_empty() { None } else { Some(&preview) }
            ) {
                tracing::warn!(error = %e, "Failed to broadcast notification");
            }
        }

        // Apply state updates
        for (name, is_ready) in state_updates {
            self.session_ready_state.insert(name, is_ready);
        }

        if should_scroll {
            self.scroll_to_bottom();
        }
    }

    /// Full scan of ALL tmux sessions every 5 minutes.
    /// Reports only CHANGED state - new sessions waiting or sessions no longer waiting.
    pub fn scan_all_sessions(&mut self) {
        // Rate limit to every 5 minutes
        let now = Instant::now();
        if let Some(last_scan) = self.last_full_scan {
            if now.duration_since(last_scan).as_secs() < 300 {
                return;
            }
        } else {
            // First call - don't run immediately, wait for the interval
            // This prevents spamming on startup
            self.last_full_scan = Some(now);
            return;
        }
        self.last_full_scan = Some(now);

        // Skip if no tmux
        let Some(tmux) = &self.tmux else { return };

        // Get all sessions
        let Ok(all_sessions) = tmux.list_sessions() else { return };

        // Check each session for ready state
        let mut current_waiting: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut waiting_previews: HashMap<String, String> = HashMap::new();

        for session_info in &all_sessions {
            if let Ok(output) = tmux.capture_output(&session_info.name, None, Some(50)) {
                if is_claude_ready(&output) {
                    let preview = extract_ready_preview(&output);
                    current_waiting.insert(session_info.name.clone());
                    waiting_previews.insert(session_info.name.clone(), preview);
                }
            }
        }

        // Exclude currently connected session
        let connected = self.project.as_ref().map(|p| format!("commander-{}", p));
        if let Some(ref conn) = connected {
            current_waiting.remove(conn);
        }

        // Find newly waiting sessions (in current but not in last scan)
        // Collect as owned strings to avoid borrow issues
        let newly_waiting: Vec<String> = current_waiting.iter()
            .filter(|name| !self.last_scan_waiting.contains(*name))
            .cloned()
            .collect();

        // Find sessions no longer waiting (in last scan but not in current)
        let no_longer_waiting: Vec<String> = self.last_scan_waiting.iter()
            .filter(|name| !current_waiting.contains(*name))
            .cloned()
            .collect();

        // Report newly waiting sessions
        if !newly_waiting.is_empty() {
            self.messages.push(Message::system(format!(
                "[clock] {} new session(s) waiting for input:",
                newly_waiting.len()
            )));
            for name in &newly_waiting {
                let display_name = name.strip_prefix("commander-").unwrap_or(name);
                let preview = waiting_previews.get(name).map(|s| s.as_str()).unwrap_or("");
                self.messages.push(Message::system(format!(
                    "   @{}{}",
                    display_name,
                    if preview.is_empty() { String::new() } else { format!(" - {}", preview) }
                )));
            }
            self.scroll_to_bottom();

            // Broadcast to all channels (Telegram, etc.)
            let sessions_for_broadcast: Vec<_> = newly_waiting.iter()
                .map(|name| {
                    let preview = waiting_previews.get(name).cloned().unwrap_or_default();
                    (name.clone(), preview)
                })
                .collect();
            if let Err(e) = commander_telegram::notify_sessions_waiting(&sessions_for_broadcast) {
                tracing::warn!(error = %e, "Failed to broadcast notification");
            }
        }

        // Report sessions that resumed work (optional, can be noisy)
        if !no_longer_waiting.is_empty() && no_longer_waiting.len() <= 3 {
            for name in &no_longer_waiting {
                let display_name = name.strip_prefix("commander-").unwrap_or(name);
                self.messages.push(Message::system(format!(
                    "[play] @{} resumed work",
                    display_name
                )));

                // Broadcast to all channels
                if let Err(e) = commander_telegram::notify_session_resumed(name) {
                    tracing::warn!(error = %e, "Failed to broadcast notification");
                }
            }
            self.scroll_to_bottom();
        }

        // Update tracking state
        self.last_scan_waiting = current_waiting;
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
                self.messages.push(Message::system(format!("[C] Connected to '{}'", project_name)));
                self.view_mode = ViewMode::Normal;
            } else {
                // Connect to regular tmux session (use session name as project name)
                let project_name = session.name.clone();
                self.sessions.insert(project_name.clone(), session.name.clone());
                self.project = Some(project_name.clone());
                self.project_path = None;  // No project path for regular sessions
                self.messages.push(Message::system(format!("[R] Connected to regular session '{}'", project_name)));
                self.messages.push(Message::system("    Note: This session is not managed by Commander. Some features may be limited."));
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
}
