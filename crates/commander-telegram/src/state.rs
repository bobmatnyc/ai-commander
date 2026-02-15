//! Shared state for the Telegram bot.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use commander_adapters::AdapterRegistry;
use commander_core::{
    clean_response, clean_screen_preview, config::authorized_chats_file, find_new_lines,
    is_claude_ready, is_summarization_available, summarize_incremental, summarize_with_fallback,
    config::runtime_state_dir,
};
use commander_persistence::StateStore;
use commander_tmux::TmuxOrchestrator;
use teloxide::types::{ChatId, MessageId, ThreadId};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[cfg(feature = "agents")]
use commander_orchestrator::AgentOrchestrator;

use crate::error::{Result, TelegramError};
use crate::pairing;
use crate::session::{PersistedSession, UserSession};

/// Result from polling output - represents different stages of output collection.
#[derive(Debug)]
pub enum PollResult {
    /// Progress update during output collection (line count increased).
    Progress(String),
    /// Incremental summary of content collected so far (every 50 lines).
    IncrementalSummary(String),
    /// Output collection complete, starting summarization.
    Summarizing,
    /// Complete response ready to send (summarization done or no summarization needed).
    /// Includes optional thread_id for forum topic routing.
    Complete(String, Option<MessageId>, Option<ThreadId>),
    /// No new output or not ready yet.
    NoOutput,
}

/// Topic-to-session mapping for forum groups.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TopicConfig {
    /// Forum topic thread ID.
    pub thread_id: i32,
    /// Display name for this topic/session.
    pub session_name: String,
    /// Associated tmux session name.
    pub tmux_session: String,
    /// Project path (if registered project).
    pub project_path: Option<String>,
}

/// Group chat configuration for forum supergroups.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GroupChatConfig {
    /// Whether group mode is enabled.
    pub is_enabled: bool,
    /// Topic-to-session mappings (thread_id -> config).
    pub topics: HashMap<i32, TopicConfig>,
}

/// Load group chat configs from disk.
fn load_group_configs() -> HashMap<i64, GroupChatConfig> {
    let path = runtime_state_dir().join("group_configs.json");
    if !path.exists() {
        return HashMap::new();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<HashMap<i64, GroupChatConfig>>(&content) {
            Ok(configs) => {
                info!(count = configs.len(), "Loaded group configs from disk");
                configs
            }
            Err(e) => {
                error!(error = %e, path = %path.display(), "Failed to parse group configs file");
                HashMap::new()
            }
        },
        Err(e) => {
            error!(error = %e, path = %path.display(), "Failed to read group configs file");
            HashMap::new()
        }
    }
}

/// Save group chat configs to disk.
fn save_group_configs(configs: &HashMap<i64, GroupChatConfig>) {
    let path = runtime_state_dir().join("group_configs.json");

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            error!(error = %e, "Failed to create state directory");
            return;
        }
    }

    match serde_json::to_string_pretty(configs) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                error!(error = %e, path = %path.display(), "Failed to write group configs file");
            } else {
                debug!(count = configs.len(), path = %path.display(), "Saved group configs to disk");
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to serialize group configs");
        }
    }
}

/// Load persisted sessions from disk.
fn load_persisted_sessions() -> HashMap<i64, PersistedSession> {
    let path = runtime_state_dir().join("telegram_sessions.json");
    if !path.exists() {
        return HashMap::new();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<HashMap<i64, PersistedSession>>(&content) {
            Ok(sessions) => {
                info!(count = sessions.len(), "Loaded persisted sessions from disk");
                sessions
            }
            Err(e) => {
                error!(error = %e, path = %path.display(), "Failed to parse persisted sessions file");
                HashMap::new()
            }
        },
        Err(e) => {
            error!(error = %e, path = %path.display(), "Failed to read persisted sessions file");
            HashMap::new()
        }
    }
}

/// Save persisted sessions to disk.
fn save_persisted_sessions(sessions: &HashMap<i64, PersistedSession>) {
    let path = runtime_state_dir().join("telegram_sessions.json");

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            error!(error = %e, "Failed to create state directory");
            return;
        }
    }

    match serde_json::to_string_pretty(sessions) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                error!(error = %e, path = %path.display(), "Failed to write persisted sessions file");
            } else {
                debug!(count = sessions.len(), path = %path.display(), "Saved persisted sessions to disk");
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to serialize persisted sessions");
        }
    }
}

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
    /// For 1:1 chats: chat_id -> session
    /// For group topics: we use (chat_id, thread_id) key encoded as i64
    sessions: RwLock<HashMap<i64, UserSession>>,
    /// Tmux orchestrator for session management.
    tmux: Option<TmuxOrchestrator>,
    /// Adapter registry for tool adapters.
    adapters: AdapterRegistry,
    /// State store for project persistence.
    store: StateStore,
    /// Authorized chat IDs for this commander instance.
    authorized_chats: RwLock<HashSet<i64>>,
    /// Group chat configurations (chat_id -> config).
    group_configs: RwLock<HashMap<i64, GroupChatConfig>>,
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

        // Load authorized chats and group configs from disk
        let authorized_chats = load_authorized_chats();
        let group_configs = load_group_configs();

        Self {
            sessions: RwLock::new(HashMap::new()),
            tmux,
            adapters,
            store,
            authorized_chats: RwLock::new(authorized_chats),
            group_configs: RwLock::new(group_configs),
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

    // --- Group mode methods ---

    /// Enable group mode for a supergroup chat.
    pub async fn enable_group_mode(&self, chat_id: i64) -> Result<()> {
        let mut configs = self.group_configs.write().await;
        let config = configs.entry(chat_id).or_insert_with(GroupChatConfig::default);
        config.is_enabled = true;
        save_group_configs(&configs);
        info!(chat_id = %chat_id, "Group mode enabled");
        Ok(())
    }

    /// Check if group mode is enabled for a chat.
    pub async fn is_group_mode(&self, chat_id: i64) -> bool {
        let configs = self.group_configs.read().await;
        configs.get(&chat_id).map(|c| c.is_enabled).unwrap_or(false)
    }

    /// Add a topic-to-session mapping.
    pub async fn add_topic(
        &self,
        chat_id: i64,
        thread_id: i32,
        session_name: String,
        tmux_session: String,
        project_path: Option<String>,
    ) -> Result<()> {
        let mut configs = self.group_configs.write().await;
        let config = configs.entry(chat_id).or_insert_with(GroupChatConfig::default);
        config.topics.insert(thread_id, TopicConfig {
            thread_id,
            session_name: session_name.clone(),
            tmux_session,
            project_path,
        });
        save_group_configs(&configs);
        info!(chat_id = %chat_id, thread_id = %thread_id, session = %session_name, "Topic added");
        Ok(())
    }

    /// Get the session for a topic.
    pub async fn get_topic_session(&self, chat_id: i64, thread_id: i32) -> Option<TopicConfig> {
        let configs = self.group_configs.read().await;
        configs.get(&chat_id)?.topics.get(&thread_id).cloned()
    }

    /// List all topics for a chat.
    pub async fn list_topics(&self, chat_id: i64) -> Vec<TopicConfig> {
        let configs = self.group_configs.read().await;
        configs.get(&chat_id)
            .map(|c| c.topics.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Remove a topic mapping.
    pub async fn remove_topic(&self, chat_id: i64, thread_id: i32) -> Option<TopicConfig> {
        let mut configs = self.group_configs.write().await;
        if let Some(config) = configs.get_mut(&chat_id) {
            let removed = config.topics.remove(&thread_id);
            if removed.is_some() {
                save_group_configs(&configs);
            }
            removed
        } else {
            None
        }
    }

    /// Generate a unique session key for group topics.
    /// Uses chat_id for 1:1 chats, or combines chat_id + thread_id for topics.
    pub fn session_key(chat_id: i64, thread_id: Option<ThreadId>) -> i64 {
        match thread_id {
            Some(tid) => {
                // Combine chat_id and thread_id into a unique key
                // Thread IDs are message IDs which are positive integers
                // We use a simple hashing: chat_id XOR (thread_id << 32)
                let tid_i64 = tid.0 .0 as i64; // ThreadId(MessageId(i32))
                chat_id ^ (tid_i64 << 32)
            }
            None => chat_id,
        }
    }

    // --- End group mode methods ---

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

    /// Get worktree info for a session if it exists.
    pub async fn get_worktree_info(&self, chat_id: ChatId) -> Option<crate::session::WorktreeInfo> {
        let sessions = self.sessions.read().await;
        sessions
            .get(&chat_id.0)
            .and_then(|s| s.worktree_info.clone())
    }

    /// Get the tmux session name for a user's current session.
    pub async fn get_current_tmux_session(&self, chat_id: i64) -> Option<String> {
        let sessions = self.sessions.read().await;
        sessions.get(&chat_id).map(|s| s.tmux_session.clone())
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

        // Strip commander- prefix if present for consistent lookup
        let base_name = project_name
            .strip_prefix("commander-")
            .unwrap_or(project_name);

        // Load all projects
        let projects = self
            .store
            .load_all_projects()
            .map_err(|e| TelegramError::SessionError(format!("Failed to load projects: {}", e)))?;

        // Try 1: Find registered project by name
        if let Some(project) = projects
            .values()
            .find(|p| p.name == base_name || p.id.as_str() == base_name)
        {
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
            drop(sessions); // Release lock before saving

            debug!(chat_id = %chat_id.0, project = %project.name, "User connected");

            // Auto-save sessions after connection
            self.save_sessions().await;

            return Ok((project.name.clone(), tool_id));
        }

        // Try 2: Fallback to direct tmux session lookup (unregistered sessions)
        let session_candidates = [
            format!("commander-{}", base_name),
            project_name.to_string(),
            base_name.to_string(),
        ];

        for session_name in &session_candidates {
            if tmux.session_exists(session_name) {
                let display_name = session_name
                    .strip_prefix("commander-")
                    .unwrap_or(session_name)
                    .to_string();

                let session = UserSession::new(
                    chat_id,
                    "unknown".to_string(), // No project path for unregistered sessions
                    display_name.clone(),
                    session_name.clone(),
                );

                let mut sessions = self.sessions.write().await;
                sessions.insert(chat_id.0, session);

                debug!(
                    chat_id = %chat_id.0,
                    session = %session_name,
                    "User connected to unregistered tmux session"
                );
                return Ok((display_name, "unknown".to_string()));
            }
        }

        // Neither registered project nor tmux session found
        Err(TelegramError::ProjectNotFound(project_name.to_string()))
    }

    /// Disconnect a user from their current project.
    pub async fn disconnect(&self, chat_id: ChatId) -> Result<Option<String>> {
        let mut sessions = self.sessions.write().await;
        let result = if let Some(session) = sessions.remove(&chat_id.0) {
            debug!(chat_id = %chat_id.0, project = %session.project_name, "User disconnected");
            Some(session.project_name)
        } else {
            None
        };
        drop(sessions); // Release lock before saving

        // Auto-save sessions after disconnection
        self.save_sessions().await;

        Ok(result)
    }

    /// Connect a topic to a project in group mode.
    /// Returns (project_name, tool_id).
    pub async fn connect_topic(
        &self,
        chat_id: ChatId,
        thread_id: ThreadId,
        project_name: &str,
    ) -> Result<(String, String)> {
        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        // Strip commander- prefix if present for consistent lookup
        let base_name = project_name
            .strip_prefix("commander-")
            .unwrap_or(project_name);

        // Load all projects
        let projects = self
            .store
            .load_all_projects()
            .map_err(|e| TelegramError::SessionError(format!("Failed to load projects: {}", e)))?;

        // Find registered project by name
        if let Some(project) = projects
            .values()
            .find(|p| p.name == base_name || p.id.as_str() == base_name)
        {
            // Validate project path still exists and is accessible
            validate_project_path(&project.path)
                .map_err(TelegramError::SessionError)?;

            let tmux_session_name = format!("commander-{}", project.name);

            // Get tool_id from project config
            let tool_id = project
                .config
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("claude-code")
                .to_string();

            // Check if tmux session exists, create if not
            if !tmux.session_exists(&tmux_session_name) {
                if let Some(adapter) = self.adapters.get(&tool_id) {
                    let (cmd, cmd_args) = adapter.launch_command(&project.path);
                    let full_cmd = if cmd_args.is_empty() {
                        cmd
                    } else {
                        format!("{} {}", cmd, cmd_args.join(" "))
                    };

                    // Create tmux session in project directory
                    tmux.create_session_in_dir(&tmux_session_name, Some(&project.path))
                        .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

                    // Send launch command
                    tmux.send_line(&tmux_session_name, None, &full_cmd)
                        .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

                    info!(
                        project = %project.name,
                        session = %tmux_session_name,
                        "Started new session for topic"
                    );
                } else {
                    return Err(TelegramError::SessionError(format!(
                        "Unknown adapter: {}",
                        tool_id
                    )));
                }
            }

            // Create user session with thread_id
            let session = UserSession::with_thread_id(
                chat_id,
                project.path.clone(),
                project.name.clone(),
                tmux_session_name.clone(),
                thread_id,
            );

            // Use combined key for topic sessions
            let session_key = Self::session_key(chat_id.0, Some(thread_id));
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_key, session);

            // Also store the topic mapping for persistence
            self.add_topic(
                chat_id.0,
                thread_id.0.0,
                project.name.clone(),
                tmux_session_name,
                Some(project.path.clone()),
            ).await?;

            debug!(
                chat_id = %chat_id.0,
                thread_id = ?thread_id,
                project = %project.name,
                "Topic connected to project"
            );
            return Ok((project.name.clone(), tool_id));
        }

        // Fallback to tmux session lookup
        let session_candidates = [
            format!("commander-{}", base_name),
            project_name.to_string(),
            base_name.to_string(),
        ];

        for tmux_session_name in &session_candidates {
            if tmux.session_exists(tmux_session_name) {
                let display_name = tmux_session_name
                    .strip_prefix("commander-")
                    .unwrap_or(tmux_session_name)
                    .to_string();

                let session = UserSession::with_thread_id(
                    chat_id,
                    "unknown".to_string(),
                    display_name.clone(),
                    tmux_session_name.clone(),
                    thread_id,
                );

                let session_key = Self::session_key(chat_id.0, Some(thread_id));
                let mut sessions = self.sessions.write().await;
                sessions.insert(session_key, session);

                // Store topic mapping
                self.add_topic(
                    chat_id.0,
                    thread_id.0.0,
                    display_name.clone(),
                    tmux_session_name.clone(),
                    None,
                ).await?;

                debug!(
                    chat_id = %chat_id.0,
                    thread_id = ?thread_id,
                    session = %tmux_session_name,
                    "Topic connected to unregistered tmux session"
                );
                return Ok((display_name, "unknown".to_string()));
            }
        }

        Err(TelegramError::ProjectNotFound(project_name.to_string()))
    }

    /// Check if a topic has an active session.
    pub async fn has_topic_session(&self, chat_id: ChatId, thread_id: ThreadId) -> bool {
        let session_key = Self::session_key(chat_id.0, Some(thread_id));
        let sessions = self.sessions.read().await;
        sessions.contains_key(&session_key)
    }

    /// Get session for a specific topic.
    pub async fn get_topic_session_info(
        &self,
        chat_id: ChatId,
        thread_id: ThreadId,
    ) -> Option<(String, String)> {
        let session_key = Self::session_key(chat_id.0, Some(thread_id));
        let sessions = self.sessions.read().await;
        sessions
            .get(&session_key)
            .map(|s| (s.project_name.clone(), s.project_path.clone()))
    }

    /// Send a message to a topic's session.
    pub async fn send_message_to_topic(
        &self,
        chat_id: ChatId,
        thread_id: ThreadId,
        message: &str,
        message_id: Option<MessageId>,
    ) -> Result<()> {
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
                            "Topic message processed through orchestrator"
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

        let session_key = Self::session_key(chat_id.0, Some(thread_id));
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&session_key)
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
            thread_id = ?thread_id,
            project = %session.project_name,
            message = %processed_message,
            "Message sent to topic session"
        );

        Ok(())
    }

    /// Poll output for a topic session.
    pub async fn poll_topic_output(
        &self,
        chat_id: ChatId,
        thread_id: ThreadId,
    ) -> Result<PollResult> {
        let session_key = Self::session_key(chat_id.0, Some(thread_id));

        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&session_key)
            .ok_or(TelegramError::NotConnected)?;

        if !session.is_waiting {
            return Ok(PollResult::NoOutput);
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

            // Check if we should emit an incremental summary (every 50 lines)
            if session.should_emit_incremental_summary() {
                let content_so_far = session.get_response();
                let line_count = session.response_buffer.len();

                // Generate incremental summary asynchronously
                match summarize_incremental(&content_so_far, line_count).await {
                    Ok(summary) => {
                        session.mark_incremental_summary_sent();
                        return Ok(PollResult::IncrementalSummary(summary));
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to generate incremental summary, continuing");
                    }
                }
            }

            // Check if we should emit a progress update
            if session.should_emit_progress() {
                let progress_msg = session.get_progress_message();
                return Ok(PollResult::Progress(progress_msg));
            }
        }

        // Check if Claude Code is idle (prompt visible and no activity for 1.5s)
        let is_idle = session.is_idle(1500);
        let has_prompt = is_claude_ready(&current_output);

        if is_idle && has_prompt && !session.response_buffer.is_empty() {
            let needs_summarization = is_summarization_available();

            if needs_summarization && !session.is_summarizing {
                session.is_summarizing = true;
                return Ok(PollResult::Summarizing);
            }

            let raw_response = session.get_response();
            let query = session.pending_query.clone().unwrap_or_default();
            let message_id = session.pending_message_id;
            let sess_thread_id = session.thread_id;
            session.reset_response_state();

            let response = if needs_summarization {
                summarize_with_fallback(&query, &raw_response).await
            } else {
                clean_response(&raw_response)
            };

            return Ok(PollResult::Complete(response, message_id, sess_thread_id));
        }

        Ok(PollResult::NoOutput)
    }

    /// Send a message to the user's connected project.
    ///
    /// Sends the user's message directly to the tmux session without LLM interpretation.
    /// The orchestrator processing was removed as it caused the LLM response to be sent
    /// to tmux instead of the user's actual message (output echo bug).
    pub async fn send_message(&self, chat_id: ChatId, message: &str, message_id: Option<MessageId>) -> Result<()> {
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

        // Send the user's message directly without LLM processing
        tmux.send_line(&session.tmux_session, None, message)
            .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

        // Start response collection with message ID for reply threading
        session.start_response_collection(message, last_output, message_id);

        debug!(
            chat_id = %chat_id.0,
            project = %session.project_name,
            message = %message,
            "Message sent to project"
        );

        Ok(())
    }

    /// Send a message directly to the session without LLM interpretation.
    ///
    /// This bypasses the orchestrator and sends the message exactly as provided,
    /// useful for sending commands that shouldn't be interpreted by the AI.
    pub async fn send_message_direct(&self, chat_id: ChatId, message: &str, message_id: Option<MessageId>) -> Result<()> {
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

        // Send message directly without orchestrator processing
        tmux.send_line(&session.tmux_session, None, message)
            .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

        // Start response collection with message ID for reply threading
        session.start_response_collection(message, last_output, message_id);

        debug!(
            chat_id = %chat_id.0,
            project = %session.project_name,
            message = %message,
            "Message sent directly to project (bypassing LLM)"
        );

        Ok(())
    }

    /// Poll for new output from a user's project.
    /// Returns PollResult indicating progress, summarizing, complete, or no output.
    pub async fn poll_output(&self, chat_id: ChatId) -> Result<PollResult> {
        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&chat_id.0)
            .ok_or(TelegramError::NotConnected)?;

        if !session.is_waiting {
            return Ok(PollResult::NoOutput);
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

            // Check if we should emit an incremental summary (every 50 lines)
            if session.should_emit_incremental_summary() {
                let content_so_far = session.get_response();
                let line_count = session.response_buffer.len();

                // Generate incremental summary asynchronously
                match summarize_incremental(&content_so_far, line_count).await {
                    Ok(summary) => {
                        session.mark_incremental_summary_sent();
                        return Ok(PollResult::IncrementalSummary(summary));
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to generate incremental summary, continuing");
                        // Don't block on failure, just log and continue
                    }
                }
            }

            // Check if we should emit a progress update
            if session.should_emit_progress() {
                let progress_msg = session.get_progress_message();
                return Ok(PollResult::Progress(progress_msg));
            }
        }

        // Check if Claude Code is idle (prompt visible and no activity for 1.5s)
        let is_idle = session.is_idle(1500);
        let has_prompt = is_claude_ready(&current_output);

        if is_idle && has_prompt && !session.response_buffer.is_empty() {
            // Check if we need to summarize (only if API key available)
            let needs_summarization = is_summarization_available();

            if needs_summarization && !session.is_summarizing {
                // First time we detect completion - signal "Summarizing" state
                session.is_summarizing = true;
                return Ok(PollResult::Summarizing);
            }

            // If we already signaled Summarizing or don't need it, do the actual work
            let raw_response = session.get_response();
            let query = session.pending_query.clone().unwrap_or_default();
            let message_id = session.pending_message_id;
            let thread_id = session.thread_id;
            session.reset_response_state();

            // Summarize or clean the response using commander-core
            let response = if needs_summarization {
                summarize_with_fallback(&query, &raw_response).await
            } else {
                clean_response(&raw_response)
            };

            return Ok(PollResult::Complete(response, message_id, thread_id));
        }

        Ok(PollResult::NoOutput)
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

    /// Get list of sessions that are waiting for responses.
    /// Returns (session_key, chat_id, thread_id) tuples.
    pub async fn get_waiting_sessions(&self) -> Vec<(i64, ChatId, Option<ThreadId>)> {
        let sessions = self.sessions.read().await;
        sessions
            .iter()
            .filter(|(_, s)| s.is_waiting)
            .map(|(key, s)| (*key, s.chat_id, s.thread_id))
            .collect()
    }

    /// Get list of chat IDs that are waiting for responses (legacy).
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

    /// List all tmux sessions with extended status info.
    /// Returns (session_name, is_commander_session, created_at, screen_preview) tuples.
    pub fn list_tmux_sessions_with_status(&self) -> Vec<(String, bool, chrono::DateTime<chrono::Utc>, Option<String>)> {
        let Some(tmux) = &self.tmux else {
            return Vec::new();
        };

        tmux.list_sessions()
            .map(|sessions| {
                sessions
                    .into_iter()
                    .map(|s| {
                        let is_commander = s.name.starts_with("commander-");
                        // Capture a small screen preview to determine idle/active state
                        let preview = tmux.capture_output(&s.name, None, Some(5))
                            .ok()
                            .map(|output| clean_screen_preview(&output, 3));
                        (s.name, is_commander, s.created_at, preview)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Connect with a git worktree in the current working directory.
    /// Creates a worktree at .worktrees/<session_name>/ with branch session/<session_name>.
    pub async fn connect_with_worktree(
        &self,
        chat_id: ChatId,
        session_name: &str,
    ) -> Result<(String, String)> {
        use std::process::Command;

        let tmux = self.tmux.as_ref().ok_or_else(|| {
            TelegramError::TmuxError("tmux not available".to_string())
        })?;

        // Get current working directory
        let cwd = std::env::current_dir()
            .map_err(|e| TelegramError::SessionError(format!("Failed to get current directory: {}", e)))?;

        let parent_repo = cwd.to_string_lossy().to_string();

        // Check if we're in a git repository
        let git_check = Command::new("git")
            .args(["-C", &parent_repo, "rev-parse", "--git-dir"])
            .output()
            .map_err(|e| TelegramError::SessionError(format!("Failed to check git: {}", e)))?;

        if !git_check.status.success() {
            return Err(TelegramError::SessionError(
                "Not in a git repository. Please run from a git repo.".to_string()
            ));
        }

        // Create worktree path
        let worktree_path = cwd.join(".worktrees").join(session_name);
        let worktree_path_str = worktree_path.to_string_lossy().to_string();
        let branch_name = format!("session/{}", session_name);

        // Check if worktree already exists
        if worktree_path.exists() {
            return Err(TelegramError::SessionError(format!(
                "Worktree already exists at {}",
                worktree_path.display()
            )));
        }

        // Create .worktrees directory if it doesn't exist
        let worktrees_dir = cwd.join(".worktrees");
        if !worktrees_dir.exists() {
            std::fs::create_dir(&worktrees_dir)
                .map_err(|e| TelegramError::SessionError(format!("Failed to create .worktrees directory: {}", e)))?;
        }

        // Create worktree with new branch
        let worktree_output = Command::new("git")
            .args([
                "-C",
                &parent_repo,
                "worktree",
                "add",
                &worktree_path_str,
                "-b",
                &branch_name,
            ])
            .output()
            .map_err(|e| TelegramError::SessionError(format!("Failed to create worktree: {}", e)))?;

        if !worktree_output.status.success() {
            let error_msg = String::from_utf8_lossy(&worktree_output.stderr);
            return Err(TelegramError::SessionError(format!(
                "git worktree add failed: {}",
                error_msg
            )));
        }

        // Detect adapter from existing config or default to claude-code
        let tool_id = self
            .store
            .load_all_projects()
            .ok()
            .and_then(|projects| {
                projects
                    .values()
                    .find(|p| p.path == parent_repo)
                    .and_then(|p| p.config.get("tool"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "claude-code".to_string());

        // Create tmux session in worktree directory
        let tmux_session_name = format!("commander-{}", session_name);
        if tmux.session_exists(&tmux_session_name) {
            return Err(TelegramError::SessionError(format!(
                "Session '{}' already exists",
                tmux_session_name
            )));
        }

        tmux.create_session_in_dir(&tmux_session_name, Some(&worktree_path_str))
            .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

        // Launch adapter in the worktree
        if let Some(adapter) = self.adapters.get(&tool_id) {
            let (cmd, cmd_args) = adapter.launch_command(&worktree_path_str);
            let full_cmd = if cmd_args.is_empty() {
                cmd
            } else {
                format!("{} {}", cmd, cmd_args.join(" "))
            };

            tmux.send_line(&tmux_session_name, None, &full_cmd)
                .map_err(|e| TelegramError::TmuxError(e.to_string()))?;
        }

        // Create user session with worktree info
        let mut session = UserSession::new(
            chat_id,
            worktree_path_str.clone(),
            session_name.to_string(),
            tmux_session_name,
        );

        session.worktree_info = Some(crate::session::WorktreeInfo {
            worktree_path: worktree_path_str,
            branch_name,
            parent_repo,
        });

        let mut sessions = self.sessions.write().await;
        sessions.insert(chat_id.0, session);

        info!(
            chat_id = %chat_id.0,
            session = %session_name,
            "Created worktree session"
        );

        Ok((session_name.to_string(), tool_id))
    }

    /// Attach to an existing tmux session.
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

    /// Save all active sessions to disk for persistence.
    pub async fn save_sessions(&self) {
        let sessions = self.sessions.read().await;
        let persisted: HashMap<i64, PersistedSession> = sessions
            .iter()
            .map(|(key, session)| (*key, PersistedSession::from_user_session(session)))
            .collect();
        save_persisted_sessions(&persisted);
    }

    /// Load sessions from disk and restore valid ones.
    /// Returns (restored_count, total_count).
    pub async fn load_sessions(&self) -> (usize, usize) {
        let persisted = load_persisted_sessions();
        let total_count = persisted.len();

        if persisted.is_empty() {
            return (0, 0);
        }

        let tmux = match &self.tmux {
            Some(t) => t,
            None => {
                warn!("Cannot restore sessions: tmux not available");
                return (0, total_count);
            }
        };

        let mut sessions = self.sessions.write().await;
        let mut restored_count = 0;

        for (key, persisted_session) in persisted {
            // Validate session: must be < 24h old and tmux session must exist
            if !persisted_session.is_valid() {
                debug!(
                    session = %persisted_session.tmux_session,
                    age_hours = persisted_session.age_seconds() / 3600,
                    "Skipping expired session"
                );
                continue;
            }

            if !tmux.session_exists(&persisted_session.tmux_session) {
                debug!(
                    session = %persisted_session.tmux_session,
                    "Skipping session: tmux session not found"
                );
                continue;
            }

            // Restore session
            let user_session = persisted_session.restore_to_user_session();
            sessions.insert(key, user_session);
            restored_count += 1;

            info!(
                session = %persisted_session.tmux_session,
                chat_id = %persisted_session.chat_id,
                "Restored session from disk"
            );
        }

        (restored_count, total_count)
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

    #[test]
    fn test_session_key_without_thread() {
        let key = TelegramState::session_key(12345, None);
        assert_eq!(key, 12345);
    }

    #[test]
    fn test_session_key_with_thread() {
        use teloxide::types::MessageId;

        let thread_id = ThreadId(MessageId(999));
        let key1 = TelegramState::session_key(12345, Some(thread_id));
        let key2 = TelegramState::session_key(12345, None);

        // Keys should be different
        assert_ne!(key1, key2);

        // Same inputs should produce same key
        let key1_again = TelegramState::session_key(12345, Some(thread_id));
        assert_eq!(key1, key1_again);
    }

    #[test]
    fn test_topic_config_serialization() {
        let config = TopicConfig {
            thread_id: 999,
            session_name: "my-session".to_string(),
            tmux_session: "commander-my-session".to_string(),
            project_path: Some("/path/to/project".to_string()),
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: TopicConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.thread_id, 999);
        assert_eq!(parsed.session_name, "my-session");
        assert_eq!(parsed.tmux_session, "commander-my-session");
        assert_eq!(parsed.project_path, Some("/path/to/project".to_string()));
    }

    #[test]
    fn test_group_chat_config_serialization() {
        let mut config = GroupChatConfig::default();
        config.is_enabled = true;
        config.topics.insert(123, TopicConfig {
            thread_id: 123,
            session_name: "test".to_string(),
            tmux_session: "commander-test".to_string(),
            project_path: None,
        });

        let json = serde_json::to_string(&config).unwrap();
        let parsed: GroupChatConfig = serde_json::from_str(&json).unwrap();

        assert!(parsed.is_enabled);
        assert_eq!(parsed.topics.len(), 1);
        assert!(parsed.topics.contains_key(&123));
    }
}
