//! Shared state for the Telegram bot.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use commander_adapters::AdapterRegistry;
use commander_persistence::StateStore;
use commander_tmux::TmuxOrchestrator;
use teloxide::types::ChatId;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::{Result, TelegramError};
use crate::pairing;
use crate::session::UserSession;

/// Shared state for the Telegram bot, accessible across all handlers.
pub struct TelegramState {
    /// Active user sessions (chat_id -> session).
    sessions: RwLock<HashMap<i64, UserSession>>,
    /// Tmux orchestrator for session management.
    tmux: Option<TmuxOrchestrator>,
    /// Adapter registry for tool adapters.
    adapters: AdapterRegistry,
    /// State store for project persistence.
    store: StateStore,
    /// OpenRouter API key for summarization.
    openrouter_key: Option<String>,
    /// Model to use for summarization.
    openrouter_model: String,
    /// Authorized chat IDs for this commander instance.
    authorized_chats: RwLock<HashSet<i64>>,
}

impl TelegramState {
    /// Create a new TelegramState instance.
    pub fn new(state_dir: &std::path::Path) -> Self {
        let tmux = TmuxOrchestrator::new().ok();
        let adapters = AdapterRegistry::new();
        let store = StateStore::new(state_dir);
        let openrouter_key = std::env::var("OPENROUTER_API_KEY").ok();
        let openrouter_model = std::env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| "anthropic/claude-sonnet-4".to_string());

        if tmux.is_none() {
            warn!("tmux not available - project connections will not work");
        }

        Self {
            sessions: RwLock::new(HashMap::new()),
            tmux,
            adapters,
            store,
            openrouter_key,
            openrouter_model,
            authorized_chats: RwLock::new(HashSet::new()),
        }
    }

    /// Check if tmux is available.
    pub fn has_tmux(&self) -> bool {
        self.tmux.is_some()
    }

    /// Check if summarization is available.
    pub fn has_summarization(&self) -> bool {
        self.openrouter_key.is_some()
    }

    /// Get a reference to the tmux orchestrator.
    pub fn tmux(&self) -> Option<&TmuxOrchestrator> {
        self.tmux.as_ref()
    }

    /// Get a reference to the adapter registry.
    pub fn adapters(&self) -> &AdapterRegistry {
        &self.adapters
    }

    /// Get a reference to the state store.
    pub fn store(&self) -> &StateStore {
        &self.store
    }

    // --- Pairing methods ---

    /// Validate and consume a pairing code, returning (project_name, session_name) on success.
    /// Authorizes the chat for the entire commander instance.
    pub async fn validate_pairing(
        &self,
        code: &str,
        chat_id: i64,
    ) -> Result<(String, String)> {
        let code = code.to_uppercase();

        // Try to consume the pairing from the shared file
        let (project_name, session_name) = pairing::consume_pairing(&code)
            .ok_or(TelegramError::InvalidPairingCode)?;

        // Authorize this chat for the commander instance
        self.authorized_chats.write().await.insert(chat_id);

        info!(
            chat_id = %chat_id,
            "Chat authorized for commander instance"
        );

        Ok((project_name, session_name))
    }

    /// Check if a chat is authorized for this commander instance.
    pub async fn is_authorized(&self, chat_id: i64) -> bool {
        self.authorized_chats.read().await.contains(&chat_id)
    }

    /// Connect a chat to a session after successful pairing.
    /// This is a convenience method that combines authorization check with connection.
    pub async fn connect_session(&self, chat_id: ChatId, project_name: &str) -> Result<String> {
        // The chat was just authorized via pairing, proceed with connection
        self.connect(chat_id, project_name).await
    }

    /// Revoke authorization for a chat.
    #[allow(dead_code)]
    pub async fn revoke_authorization(&self, chat_id: i64) {
        self.authorized_chats.write().await.remove(&chat_id);
        debug!(
            chat_id = %chat_id,
            "Authorization revoked"
        );
    }

    // --- End pairing methods ---

    /// Check if a user has an active session.
    pub async fn has_session(&self, chat_id: ChatId) -> bool {
        let sessions = self.sessions.read().await;
        sessions.contains_key(&chat_id.0)
    }

    /// Get a user's session info (project name).
    pub async fn get_session_info(&self, chat_id: ChatId) -> Option<(String, String)> {
        let sessions = self.sessions.read().await;
        sessions
            .get(&chat_id.0)
            .map(|s| (s.project_name.clone(), s.project_path.clone()))
    }

    /// Connect a user to a project.
    pub async fn connect(&self, chat_id: ChatId, project_name: &str) -> Result<String> {
        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        // Load all projects
        let projects = self
            .store
            .load_all_projects()
            .map_err(|e| TelegramError::SessionError(format!("Failed to load projects: {}", e)))?;

        // Find project by name
        let project = projects
            .values()
            .find(|p| p.name == project_name || p.id.as_str() == project_name)
            .ok_or_else(|| TelegramError::ProjectNotFound(project_name.to_string()))?;

        let session_name = format!("commander-{}", project.name);

        // Check if tmux session exists, create if not
        if !tmux.session_exists(&session_name) {
            // Try to start the project
            let tool_id = project
                .config
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("claude-code");

            if let Some(adapter) = self.adapters.get(tool_id) {
                let (cmd, cmd_args) = adapter.launch_command(&project.path);
                let full_cmd = if cmd_args.is_empty() {
                    cmd
                } else {
                    format!("{} {}", cmd, cmd_args.join(" "))
                };

                // Create tmux session in project directory
                tmux.create_session_in_dir(&session_name, Some(&project.path))
                    .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

                // Send launch command
                tmux.send_line(&session_name, None, &full_cmd)
                    .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

                info!(
                    project = %project.name,
                    session = %session_name,
                    "Started new Claude Code session"
                );
            } else {
                return Err(TelegramError::SessionError(format!(
                    "Unknown adapter: {}",
                    tool_id
                )));
            }
        }

        // Create user session
        let session = UserSession::new(
            chat_id,
            project.path.clone(),
            project.name.clone(),
            session_name,
        );

        let mut sessions = self.sessions.write().await;
        sessions.insert(chat_id.0, session);

        debug!(chat_id = %chat_id.0, project = %project.name, "User connected");
        Ok(project.name.clone())
    }

    /// Disconnect a user from their current project.
    pub async fn disconnect(&self, chat_id: ChatId) -> Result<Option<String>> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.remove(&chat_id.0) {
            debug!(chat_id = %chat_id.0, project = %session.project_name, "User disconnected");
            Ok(Some(session.project_name))
        } else {
            Ok(None)
        }
    }

    /// Send a message to the user's connected project.
    pub async fn send_message(&self, chat_id: ChatId, message: &str) -> Result<()> {
        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&chat_id.0)
            .ok_or(TelegramError::NotConnected)?;

        // Capture initial output for comparison
        let last_output = tmux
            .capture_output(&session.tmux_session, None, Some(200))
            .unwrap_or_default();

        // Send the message
        tmux.send_line(&session.tmux_session, None, message)
            .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

        // Start response collection
        session.start_response_collection(message, last_output);

        debug!(
            chat_id = %chat_id.0,
            project = %session.project_name,
            message = %message,
            "Message sent to project"
        );

        Ok(())
    }

    /// Poll for new output from a user's project.
    /// Returns Some(response) when idle and response is ready, None otherwise.
    pub async fn poll_output(&self, chat_id: ChatId) -> Result<Option<String>> {
        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&chat_id.0)
            .ok_or(TelegramError::NotConnected)?;

        if !session.is_waiting {
            return Ok(None);
        }

        // Capture current output
        let current_output = tmux
            .capture_output(&session.tmux_session, None, Some(200))
            .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

        // Check for new content
        if current_output != session.last_output {
            let new_lines = find_new_lines(&session.last_output, &current_output);
            session.add_response_lines(new_lines);
            session.last_output = current_output.clone();
        }

        // Check if Claude Code is idle (prompt visible and no activity for 1.5s)
        let is_idle = session.is_idle(1500);
        let has_prompt = is_claude_code_ready(&current_output);

        if is_idle && has_prompt && !session.response_buffer.is_empty() {
            let raw_response = session.get_response();
            let query = session.pending_query.clone().unwrap_or_default();
            session.reset_response_state();

            // Summarize or clean the response
            let response = if self.openrouter_key.is_some() {
                self.summarize_response(&query, &raw_response).await
            } else {
                clean_raw_response(&raw_response)
            };

            return Ok(Some(response));
        }

        Ok(None)
    }

    /// Summarize a response using OpenRouter.
    async fn summarize_response(&self, query: &str, raw_response: &str) -> String {
        let Some(api_key) = &self.openrouter_key else {
            return clean_raw_response(raw_response);
        };

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

        let request_body = serde_json::json!({
            "model": self.openrouter_model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "max_tokens": 500
        });

        let client = reqwest::Client::new();
        let response = client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await;

        match response {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(content) = json["choices"][0]["message"]["content"].as_str() {
                        return content.to_string();
                    }
                }
                clean_raw_response(raw_response)
            }
            Err(e) => {
                warn!(error = %e, "Summarization failed, using raw response");
                clean_raw_response(raw_response)
            }
        }
    }

    /// List all available projects.
    pub fn list_projects(&self) -> Vec<(String, String)> {
        self.store
            .load_all_projects()
            .map(|projects| {
                projects
                    .values()
                    .map(|p| (p.name.clone(), p.path.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get list of chat IDs that are waiting for responses.
    pub async fn get_waiting_chat_ids(&self) -> Vec<i64> {
        let sessions = self.sessions.read().await;
        sessions
            .iter()
            .filter(|(_, s)| s.is_waiting)
            .map(|(id, _)| *id)
            .collect()
    }

    /// List all tmux sessions.
    /// Returns (session_name, is_commander_session) pairs.
    pub fn list_tmux_sessions(&self) -> Vec<(String, bool)> {
        let Some(tmux) = &self.tmux else {
            return Vec::new();
        };

        tmux.list_sessions()
            .map(|sessions| {
                sessions
                    .into_iter()
                    .map(|s| {
                        let is_commander = s.name.starts_with("commander-");
                        (s.name, is_commander)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Attach to an existing tmux session.
    pub async fn attach_session(
        &self,
        chat_id: ChatId,
        session_name: &str,
    ) -> Result<String> {
        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        // Check if session exists
        if !tmux.session_exists(session_name) {
            return Err(TelegramError::SessionError(
                format!("Session '{}' not found. Use /sessions to list available sessions.", session_name)
            ));
        }

        // Try to determine project info from session name
        let (project_name, project_path) = if session_name.starts_with("commander-") {
            let name = session_name.strip_prefix("commander-").unwrap_or(session_name);
            // Try to find project in store
            if let Ok(projects) = self.store.load_all_projects() {
                if let Some(project) = projects.values().find(|p| p.name == name) {
                    (project.name.clone(), project.path.clone())
                } else {
                    (name.to_string(), format!("~/{}", name))
                }
            } else {
                (name.to_string(), format!("~/{}", name))
            }
        } else {
            (session_name.to_string(), "unknown".to_string())
        };

        // Create user session
        let session = UserSession::new(
            chat_id,
            project_path,
            project_name.clone(),
            session_name.to_string(),
        );

        let mut sessions = self.sessions.write().await;
        sessions.insert(chat_id.0, session);

        debug!(chat_id = %chat_id.0, session = %session_name, "User attached to session");
        Ok(session_name.to_string())
    }

    /// Create and connect to a new project.
    pub async fn connect_new(
        &self,
        chat_id: ChatId,
        path: &str,
        adapter: &str,
        name: &str,
    ) -> Result<String> {
        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        // Resolve adapter alias
        let tool_id = self.adapters.resolve(adapter)
            .ok_or_else(|| TelegramError::SessionError(
                format!("Unknown adapter: {}. Use: cc (claude-code), mpm", adapter)
            ))?
            .to_string();

        // Check if project already exists
        let projects = self.store.load_all_projects()
            .map_err(|e| TelegramError::SessionError(format!("Failed to load projects: {}", e)))?;

        if projects.values().any(|p| p.name == name) {
            return Err(TelegramError::SessionError(
                format!("Project '{}' already exists. Use /connect {}", name, name)
            ));
        }

        // Create project
        let mut project = commander_models::Project::new(path, name);
        project.config.insert("tool".to_string(), serde_json::json!(tool_id));

        // Save project
        self.store.save_project(&project)
            .map_err(|e| TelegramError::SessionError(format!("Failed to save project: {}", e)))?;

        info!(name = %name, path = %path, adapter = %tool_id, "Created new project");

        // Connect to the new project
        self.connect(chat_id, name).await
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
    let lines: Vec<&str> = output
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(10)
        .collect();

    if lines.is_empty() {
        return false;
    }

    // Pattern 1: Line contains just the prompt character
    for line in &lines[..lines.len().min(3)] {
        let trimmed = line.trim();
        if trimmed == "❯" || trimmed == "❯ " {
            return true;
        }
        if trimmed.ends_with(" ❯") || trimmed.ends_with(" ❯ ") {
            return true;
        }
    }

    // Pattern 2: The input box separator lines
    let has_separator = lines.iter().take(5).any(|l| {
        let trimmed = l.trim();
        trimmed.starts_with("───") || trimmed.starts_with("╭─") || trimmed.starts_with("╰─")
    });

    // Pattern 3: "bypass permissions" hint shown at prompt
    let has_bypass_hint = lines.iter().take(5).any(|l| l.contains("bypass permissions"));

    // Pattern 4: Empty prompt box
    if has_separator {
        for (i, line) in lines.iter().enumerate() {
            if line.contains("❯") && i < 5 {
                return true;
            }
        }
    }

    // Pattern 5: Common ready indicators
    let has_ready_indicator = lines.iter().take(3).any(|l| {
        let trimmed = l.trim();
        trimmed == "│ ❯"
            || trimmed.starts_with("│ ❯")
            || trimmed == ">"
            || trimmed.ends_with("> ")
            || trimmed.contains("[ready]")
    });

    has_ready_indicator || has_bypass_hint
}

/// Check if a line is Claude Code UI noise that should be filtered out.
fn is_ui_noise(line: &str) -> bool {
    // Prompt lines
    if line.contains("] ❯ ") || line.contains("] > ") {
        return true;
    }

    // Spinner characters
    let spinners = ['✳', '✶', '✻', '✽', '✢', '⏺', '·', '●', '○', '◐', '◑', '◒', '◓'];
    if line
        .chars()
        .next()
        .map(|c| spinners.contains(&c))
        .unwrap_or(false)
    {
        return true;
    }

    // Status bar box drawing characters
    if line.starts_with('╰')
        || line.starts_with('╭')
        || line.starts_with('│')
        || line.starts_with('├')
        || line.starts_with('└')
        || line.starts_with('┌')
    {
        return true;
    }

    // Claude Code branding
    if line.contains("▐▛") || line.contains("▜▌") || line.contains("▝▜") {
        return true;
    }

    // Thinking indicators
    let lower = line.to_lowercase();
    if lower.contains("spelunking")
        || lower.contains("(thinking)")
        || lower.contains("thinking…")
        || lower.contains("thinking...")
    {
        return true;
    }

    // Status messages
    if lower.contains("ctrl+b") || lower.contains("to run in background") {
        return true;
    }

    // Version/branding
    if lower.contains("claude code v")
        || lower.contains("claude max")
        || lower.contains("opus 4")
        || lower.contains("sonnet")
    {
        return true;
    }

    false
}

/// Clean raw response when summarization isn't available.
fn clean_raw_response(raw: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
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

/// Create a shared state wrapped in Arc for use across handlers.
pub fn create_shared_state(state_dir: &std::path::Path) -> Arc<TelegramState> {
    Arc::new(TelegramState::new(state_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ui_noise() {
        assert!(is_ui_noise("[project] ❯ command"));
        assert!(is_ui_noise("✳ Loading..."));
        assert!(is_ui_noise("╭─────────"));
        assert!(!is_ui_noise("This is actual content"));
    }

    #[test]
    fn test_clean_raw_response() {
        let raw = "⏺ Loading\nActual response\n⎿ Footer";
        let cleaned = clean_raw_response(raw);
        assert_eq!(cleaned, "Actual response");
    }

    #[test]
    fn test_find_new_lines() {
        let prev = "line1\nline2";
        let current = "line1\nline2\nline3";
        let new = find_new_lines(prev, current);
        assert_eq!(new, vec!["line3"]);
    }
}
