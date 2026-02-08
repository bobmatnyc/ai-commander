//! Shared state for the Telegram bot.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use commander_adapters::AdapterRegistry;
use commander_core::{
    clean_screen_preview, config::authorized_chats_file, find_new_lines, is_claude_ready,
    summarize_with_fallback,
};
use commander_persistence::StateStore;
use commander_tmux::TmuxOrchestrator;
use teloxide::types::{ChatId, MessageId};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[cfg(feature = "agents")]
use commander_orchestrator::AgentOrchestrator;

use crate::error::{Result, TelegramError};
use crate::pairing;
use crate::session::UserSession;

/// Load authorized chat IDs from disk.
fn load_authorized_chats() -> HashSet<i64> {
    let path = authorized_chats_file();
    if !path.exists() {
        return HashSet::new();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<Vec<i64>>(&content) {
            Ok(chat_ids) => {
                let set: HashSet<i64> = chat_ids.into_iter().collect();
                info!(count = set.len(), "Loaded authorized chats from disk");
                set
            }
            Err(e) => {
                error!(error = %e, path = %path.display(), "Failed to parse authorized chats file");
                HashSet::new()
            }
        },
        Err(e) => {
            error!(error = %e, path = %path.display(), "Failed to read authorized chats file");
            HashSet::new()
        }
    }
}

/// Save authorized chat IDs to disk.
fn save_authorized_chats(chat_ids: &HashSet<i64>) {
    let path = authorized_chats_file();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            error!(error = %e, "Failed to create state directory");
            return;
        }
    }

    let chat_vec: Vec<i64> = chat_ids.iter().copied().collect();
    match serde_json::to_string_pretty(&chat_vec) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                error!(error = %e, path = %path.display(), "Failed to write authorized chats file");
            } else {
                debug!(count = chat_ids.len(), path = %path.display(), "Saved authorized chats to disk");
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to serialize authorized chats");
        }
    }
}

/// Validate that a project path exists, is a directory, and is accessible.
fn validate_project_path(path: &str) -> std::result::Result<(), String> {
    let path = Path::new(path);

    if !path.exists() {
        return Err(format!("Project path does not exist: {}", path.display()));
    }

    if !path.is_dir() {
        return Err(format!(
            "Project path is not a directory: {}",
            path.display()
        ));
    }

    // Check if readable by attempting to read dir
    if path.read_dir().is_err() {
        return Err(format!(
            "Cannot access project path: {} (permission denied)",
            path.display()
        ));
    }

    Ok(())
}

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
    /// Authorized chat IDs for this commander instance.
    authorized_chats: RwLock<HashSet<i64>>,
    /// Agent orchestrator for LLM-based message processing (feature-gated).
    #[cfg(feature = "agents")]
    orchestrator: RwLock<Option<AgentOrchestrator>>,
}

impl TelegramState {
    /// Create a new TelegramState instance.
    pub fn new(state_dir: &std::path::Path) -> Self {
        let tmux = TmuxOrchestrator::new().ok();
        let adapters = AdapterRegistry::new();
        let store = StateStore::new(state_dir);

        if tmux.is_none() {
            warn!("tmux not available - project connections will not work");
        }

        // Load authorized chats from disk
        let authorized_chats = load_authorized_chats();

        Self {
            sessions: RwLock::new(HashMap::new()),
            tmux,
            adapters,
            store,
            authorized_chats: RwLock::new(authorized_chats),
            #[cfg(feature = "agents")]
            orchestrator: RwLock::new(None),
        }
    }

    /// Initialize the agent orchestrator asynchronously (when agents feature is enabled).
    ///
    /// This should be called after state creation to enable LLM-based processing.
    /// Returns Ok(true) if initialized, Ok(false) if already initialized or unavailable.
    #[cfg(feature = "agents")]
    pub async fn init_orchestrator(&self) -> Result<bool> {
        let mut orchestrator = self.orchestrator.write().await;
        if orchestrator.is_some() {
            return Ok(false); // Already initialized
        }

        match AgentOrchestrator::new().await {
            Ok(orch) => {
                info!("Agent orchestrator initialized for Telegram bot");
                *orchestrator = Some(orch);
                Ok(true)
            }
            Err(e) => {
                warn!(error = %e, "Failed to initialize orchestrator, continuing without LLM features");
                Ok(false)
            }
        }
    }

    /// Check if the orchestrator is available.
    #[cfg(feature = "agents")]
    pub async fn has_orchestrator(&self) -> bool {
        self.orchestrator.read().await.is_some()
    }

    /// Check if tmux is available.
    pub fn has_tmux(&self) -> bool {
        self.tmux.is_some()
    }

    /// Check if summarization is available.
    pub fn has_summarization(&self) -> bool {
        commander_core::is_summarization_available()
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
        {
            let mut chats = self.authorized_chats.write().await;
            chats.insert(chat_id);
            // Persist to disk
            save_authorized_chats(&chats);
        }

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

    /// Get all authorized chat IDs for broadcasting notifications.
    pub async fn get_authorized_chat_ids(&self) -> Vec<i64> {
        self.authorized_chats.read().await.iter().copied().collect()
    }

    /// Connect a chat to a session after successful pairing.
    /// This is a convenience method that combines authorization check with connection.
    /// Returns (project_name, tool_id).
    pub async fn connect_session(&self, chat_id: ChatId, project_name: &str) -> Result<(String, String)> {
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

    /// Get detailed session status for /status command.
    /// Returns (project_name, project_path, tool_id, is_waiting, pending_query, screen_preview).
    pub async fn get_session_status(
        &self,
        chat_id: ChatId,
    ) -> Option<(String, String, String, bool, Option<String>, Option<String>)> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(&chat_id.0)?;

        // Get tool_id from project config
        let tool_id = self
            .store
            .load_all_projects()
            .ok()
            .and_then(|projects| {
                projects
                    .values()
                    .find(|p| p.name == session.project_name)
                    .and_then(|p| p.config.get("tool"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "claude-code".to_string());

        // Get screen preview from tmux
        let screen_preview = self.tmux.as_ref().and_then(|tmux| {
            tmux.capture_output(&session.tmux_session, None, Some(10))
                .ok()
                .map(|output| clean_screen_preview(&output, 5))
        });

        Some((
            session.project_name.clone(),
            session.project_path.clone(),
            tool_id,
            session.is_waiting,
            session.pending_query.clone(),
            screen_preview,
        ))
    }

    /// Connect a user to a project.
    /// Connect to an existing project. Returns (project_name, tool_id).
    pub async fn connect(&self, chat_id: ChatId, project_name: &str) -> Result<(String, String)> {
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

        // Validate project path still exists and is accessible
        validate_project_path(&project.path)
            .map_err(TelegramError::SessionError)?;

        let session_name = format!("commander-{}", project.name);

        // Get tool_id from project config
        let tool_id = project
            .config
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("claude-code")
            .to_string();

        // Check if tmux session exists, create if not
        if !tmux.session_exists(&session_name) {
            if let Some(adapter) = self.adapters.get(&tool_id) {
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
                    "Started new session"
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
        Ok((project.name.clone(), tool_id))
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
    ///
    /// When agents feature is enabled and orchestrator is available, messages are
    /// processed through the UserAgent for interpretation before being sent to tmux.
    pub async fn send_message(&self, chat_id: ChatId, message: &str, message_id: Option<MessageId>) -> Result<()> {
        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        // Try to process message through orchestrator first (agents feature)
        #[cfg(feature = "agents")]
        let processed_message = {
            let mut orchestrator = self.orchestrator.write().await;
            if let Some(ref mut orch) = *orchestrator {
                match orch.process_user_input(message).await {
                    Ok(processed) => {
                        debug!(
                            original = %message,
                            processed = %processed,
                            "Message processed through orchestrator"
                        );
                        processed
                    }
                    Err(e) => {
                        warn!(error = %e, "Orchestrator processing failed, using original message");
                        message.to_string()
                    }
                }
            } else {
                message.to_string()
            }
        };

        #[cfg(not(feature = "agents"))]
        let processed_message = message.to_string();

        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&chat_id.0)
            .ok_or(TelegramError::NotConnected)?;

        // Capture initial output for comparison
        let last_output = tmux
            .capture_output(&session.tmux_session, None, Some(200))
            .unwrap_or_default();

        // Send the processed message
        tmux.send_line(&session.tmux_session, None, &processed_message)
            .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

        // Start response collection with message ID for reply threading
        session.start_response_collection(&processed_message, last_output, message_id);

        debug!(
            chat_id = %chat_id.0,
            project = %session.project_name,
            message = %processed_message,
            "Message sent to project"
        );

        Ok(())
    }

    /// Poll for new output from a user's project.
    /// Returns Some((response, message_id)) when idle and response is ready, None otherwise.
    pub async fn poll_output(&self, chat_id: ChatId) -> Result<Option<(String, Option<MessageId>)>> {
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
        let has_prompt = is_claude_ready(&current_output);

        if is_idle && has_prompt && !session.response_buffer.is_empty() {
            let raw_response = session.get_response();
            let query = session.pending_query.clone().unwrap_or_default();
            let message_id = session.pending_message_id;
            session.reset_response_state();

            // Summarize or clean the response using commander-core
            let response = summarize_with_fallback(&query, &raw_response).await;

            return Ok(Some((response, message_id)));
        }

        Ok(None)
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
    ) -> Result<(String, String)> {
        // Verify tmux is available (value unused; connect() uses it internally)
        let _tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        // Resolve adapter alias
        let tool_id = self.adapters.resolve(adapter)
            .ok_or_else(|| TelegramError::SessionError(
                format!("Unknown adapter: {}. Use: cc (claude-code), mpm", adapter)
            ))?
            .to_string();

        // Validate project path exists and is accessible
        validate_project_path(path)
            .map_err(TelegramError::SessionError)?;

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

/// Create a shared state wrapped in Arc for use across handlers.
pub fn create_shared_state(state_dir: &std::path::Path) -> Arc<TelegramState> {
    Arc::new(TelegramState::new(state_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_core::{clean_response, is_ui_noise};

    #[test]
    fn test_is_ui_noise() {
        assert!(is_ui_noise("[project] \u{276F} command"));
        assert!(is_ui_noise("\u{2733} Loading..."));
        assert!(is_ui_noise("\u{256D}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}"));
        assert!(!is_ui_noise("This is actual content"));
    }

    #[test]
    fn test_clean_response() {
        let raw = "\u{23FA} Loading\nActual response\n\u{23BF} Footer";
        let cleaned = clean_response(raw);
        assert_eq!(cleaned, "Actual response");
    }

    #[test]
    fn test_find_new_lines() {
        let prev = "line1\nline2";
        let current = "line1\nline2\nline3";
        let new = find_new_lines(prev, current);
        assert_eq!(new, vec!["line3"]);
    }

    #[test]
    fn test_clean_screen_preview() {
        // Test with UI noise mixed with content
        let output = "\u{256D}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\nActual content 1\n\u{2502} \u{276F}\nContent 2\nContent 3\n\u{2733} Loading";
        let cleaned = clean_screen_preview(&output, 5);
        assert_eq!(cleaned, "Actual content 1\nContent 2\nContent 3");
    }

    #[test]
    fn test_clean_screen_preview_limits_lines() {
        // Test that it only returns last 5 lines
        let output = "line1\nline2\nline3\nline4\nline5\nline6\nline7";
        let cleaned = clean_screen_preview(&output, 5);
        assert_eq!(cleaned, "line3\nline4\nline5\nline6\nline7");
    }
}
