//! Connection management for the TUI.
//!
//! Contains methods for connecting to projects, disconnecting,
//! and managing tmux sessions.

use super::app::{App, Message};
use crate::validate_project_path;

/// Parsed connect command arguments.
#[derive(Debug)]
pub(super) enum ConnectArgs {
    /// Connect to existing project by name
    Existing(String),
    /// Create and connect to new project
    New { path: String, adapter: String, name: String },
}

impl App {
    /// Connect to a project by name.
    ///
    /// Fallback chain:
    /// 1. Try registered project (has adapter, path, etc.)
    /// 2. Try tmux session directly (if no project found)
    pub fn connect(&mut self, name: &str) -> Result<(), String> {
        // Strip commander- prefix if present (user might use session name)
        let base_name = name.strip_prefix("commander-").unwrap_or(name);

        // Load all projects
        let projects = self.store.load_all_projects()
            .map_err(|e| format!("Failed to load projects: {}", e))?;

        // Try 1: Find registered project by name
        if let Some(project) = projects.values()
            .find(|p| p.name == base_name || p.id.as_str() == base_name)
        {
            // Validate project path still exists and is accessible
            validate_project_path(&project.path)?;

            let session_name = format!("commander-{}", project.name);

            // Check if tmux session exists
            if let Some(ref tmux) = self.tmux {
                if tmux.session_exists(&session_name) {
                    self.sessions.insert(project.name.clone(), session_name);
                    self.project = Some(project.name.clone());
                    self.project_path = Some(project.path.clone());
                    self.messages.push(Message::system(format!("[C] Connected to '{}'", project.name)));
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
                    self.messages.push(Message::system(format!("[C] Started and connected to '{}'", project.name)));
                    return Ok(());
                }
            }

            return Err("Tmux not available".to_string());
        }

        // Try 2: Check for tmux session directly (unregistered session)
        if let Some(ref tmux) = self.tmux {
            // Try multiple session name variants
            let session_candidates = [
                format!("commander-{}", base_name),
                name.to_string(),
                base_name.to_string(),
            ];

            for session_name in session_candidates {
                if tmux.session_exists(&session_name) {
                    // Connect to regular/unregistered tmux session
                    let is_commander = session_name.starts_with("commander-");
                    let display_name = if is_commander {
                        session_name.strip_prefix("commander-").unwrap_or(&session_name)
                    } else {
                        &session_name
                    };
                    self.sessions.insert(display_name.to_string(), session_name.clone());
                    self.project = Some(display_name.to_string());
                    self.project_path = None;
                    if is_commander {
                        self.messages.push(Message::system(
                            format!("[C] Connected to session '{}' (unregistered project)", display_name)
                        ));
                    } else {
                        self.messages.push(Message::system(
                            format!("[R] Connected to regular session '{}'", session_name)
                        ));
                        self.messages.push(Message::system(
                            "    Note: This session is not managed by Commander. Some features may be limited."
                        ));
                    }
                    return Ok(());
                }
            }
        }

        Err(format!("No project or session found: {}", name))
    }

    /// Parse connect command arguments.
    pub(super) fn parse_connect_args(&self, arg: &str) -> Result<ConnectArgs, String> {
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

        // Validate project path exists and is accessible
        validate_project_path(path)?;

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

    /// Get the current tmux session name from environment or tmux command.
    pub(super) fn get_current_tmux_session(&self) -> Option<String> {
        // First try environment variable
        if std::env::var("TMUX").is_ok() {
            // We're inside tmux, get the session name
            if let Ok(output) = std::process::Command::new("tmux")
                .args(["display-message", "-p", "#S"])
                .output()
            {
                if output.status.success() {
                    let session = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !session.is_empty() {
                        // Strip "commander-" prefix if present for consistency
                        return Some(session.strip_prefix("commander-")
                            .unwrap_or(&session)
                            .to_string());
                    }
                }
            }
        }
        None
    }

    /// Rename the current tmux session.
    pub(super) fn rename_current_session(&mut self, new_name: &str) {
        // Get current session name
        let current_session = if std::env::var("TMUX").is_ok() {
            std::process::Command::new("tmux")
                .args(["display-message", "-p", "#S"])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if !s.is_empty() { Some(s) } else { None }
                    } else {
                        None
                    }
                })
        } else {
            None
        };

        let Some(old_session) = current_session else {
            self.messages.push(Message::system("Not running inside a tmux session"));
            return;
        };

        // Determine new session name (add commander- prefix if old had it)
        let new_session = if old_session.starts_with("commander-") {
            format!("commander-{}", new_name)
        } else {
            new_name.to_string()
        };

        // Run tmux rename-session
        match std::process::Command::new("tmux")
            .args(["rename-session", "-t", &old_session, &new_session])
            .output()
        {
            Ok(output) if output.status.success() => {
                self.messages.push(Message::system(format!(
                    "Renamed session '{}' to '{}'",
                    old_session, new_session
                )));

                // Update internal tracking if this was a commander session
                if let Some(old_project) = old_session.strip_prefix("commander-") {
                    // Remove old mapping
                    self.sessions.remove(old_project);
                    // Add new mapping
                    self.sessions.insert(new_name.to_string(), new_session.clone());

                    // Update connected project if it was the old one
                    if self.project.as_deref() == Some(old_project) {
                        self.project = Some(new_name.to_string());
                        self.messages.push(Message::system(format!(
                            "Updated connection to '{}'", new_name
                        )));
                    }
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.messages.push(Message::system(format!(
                    "Failed to rename session: {}", stderr.trim()
                )));
            }
            Err(e) => {
                self.messages.push(Message::system(format!(
                    "Failed to run tmux rename: {}", e
                )));
            }
        }
    }

    /// Stop a session: commit git changes and destroy tmux session.
    pub fn stop_session(&mut self, name: &str) {
        // Strip commander- prefix if present (user might use session name)
        let name = name.strip_prefix("commander-").unwrap_or(name);
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
}
