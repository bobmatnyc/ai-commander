//! Project types for Commander.
//!
//! Projects represent managed codebases with their associated state,
//! work queues, and communication threads.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::event::Event;
use crate::ids::{EventId, MessageId, ProjectId, SessionId};
use crate::work::WorkItem;

/// Adapter type for a project's runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterType {
    /// Claude Code adapter.
    ClaudeCode,
    /// Claude MPM adapter.
    ClaudeMpm,
    /// Auggie adapter.
    Auggie,
    /// Codex adapter.
    Codex,
    /// Shell adapter.
    Shell,
}

impl Default for AdapterType {
    fn default() -> Self {
        Self::ClaudeCode
    }
}

impl std::str::FromStr for AdapterType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude-code" | "cc" => Ok(Self::ClaudeCode),
            "claude-mpm" | "mpm" => Ok(Self::ClaudeMpm),
            "auggie" | "augment" => Ok(Self::Auggie),
            "codex" => Ok(Self::Codex),
            "shell" | "sh" => Ok(Self::Shell),
            _ => Err(format!(
                "Unknown adapter type: '{}'. Valid: claude-code, claude-mpm, auggie, codex, shell",
                s
            )),
        }
    }
}

impl fmt::Display for AdapterType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClaudeCode => write!(f, "claude-code"),
            Self::ClaudeMpm => write!(f, "claude-mpm"),
            Self::Auggie => write!(f, "auggie"),
            Self::Codex => write!(f, "codex"),
            Self::Shell => write!(f, "shell"),
        }
    }
}

/// State of a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectState {
    /// Project is idle and waiting for work.
    #[default]
    Idle,
    /// Project is actively being worked on.
    Working,
    /// Project is blocked and cannot proceed.
    Blocked,
    /// Project has been paused.
    Paused,
    /// Project is in an error state.
    Error,
}

/// A tool session within a project.
///
/// Represents an active connection to an external tool or service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSession {
    /// Unique identifier for the session.
    pub id: SessionId,

    /// ID of the project this session belongs to.
    pub project_id: ProjectId,

    /// Runtime environment for the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,

    /// tmux target for the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmux_target: Option<String>,

    /// Current status of the session.
    pub status: String,

    /// Buffer of recent output from the session.
    #[serde(default)]
    pub output_buffer: Vec<String>,

    /// When the session was created.
    pub created_at: DateTime<Utc>,

    /// When the session last produced output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_output_at: Option<DateTime<Utc>>,
}

impl ToolSession {
    /// Creates a new tool session.
    pub fn new(project_id: impl Into<ProjectId>) -> Self {
        Self {
            id: SessionId::new(),
            project_id: project_id.into(),
            runtime: None,
            tmux_target: None,
            status: "created".to_string(),
            output_buffer: Vec::new(),
            created_at: Utc::now(),
            last_output_at: None,
        }
    }

    /// Appends output to the buffer.
    pub fn append_output(&mut self, output: String) {
        self.output_buffer.push(output);
        self.last_output_at = Some(Utc::now());
    }
}

/// A message in a project's thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMessage {
    /// Unique identifier for the message.
    pub id: MessageId,

    /// Role of the message sender (e.g., "user", "assistant").
    pub role: String,

    /// Content of the message.
    pub content: String,

    /// Session ID associated with this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,

    /// Event ID associated with this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<EventId>,

    /// When the message was created.
    pub timestamp: DateTime<Utc>,
}

impl ThreadMessage {
    /// Creates a new thread message.
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            role: role.into(),
            content: content.into(),
            session_id: None,
            event_id: None,
            timestamp: Utc::now(),
        }
    }

    /// Creates a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    /// Creates an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }
}

/// A project managed by Commander.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Unique identifier for the project.
    pub id: ProjectId,

    /// Path to the project directory.
    pub path: String,

    /// Name of the project.
    pub name: String,

    /// Current state of the project.
    pub state: ProjectState,

    /// Reason for the current state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_reason: Option<String>,

    /// Whether the project configuration has been loaded.
    #[serde(default)]
    pub config_loaded: bool,

    /// Project configuration.
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,

    /// Active tool sessions for this project.
    #[serde(default)]
    pub sessions: HashMap<SessionId, ToolSession>,

    /// Queue of pending work items.
    #[serde(default)]
    pub work_queue: Vec<WorkItem>,

    /// Currently active work item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_work: Option<WorkItem>,

    /// Completed work items.
    #[serde(default)]
    pub completed_work: Vec<WorkItem>,

    /// Pending events requiring attention.
    #[serde(default)]
    pub pending_events: Vec<Event>,

    /// History of all events.
    #[serde(default)]
    pub event_history: Vec<Event>,

    /// Thread of messages for this project.
    #[serde(default)]
    pub thread: Vec<ThreadMessage>,

    /// Aliases for this project (e.g., ["prod", "staging"]).
    /// Maximum 10 aliases per project.
    /// Aliases must be alphanumeric with optional dash/underscore.
    #[serde(default)]
    pub aliases: Vec<String>,

    /// Adapter type for this project's runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_type: Option<AdapterType>,

    /// When the project was created.
    pub created_at: DateTime<Utc>,

    /// When the project was last active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity: Option<DateTime<Utc>>,
}

impl Project {
    /// Creates a new project with the given path and name.
    pub fn new(path: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: ProjectId::new(),
            path: path.into(),
            name: name.into(),
            state: ProjectState::Idle,
            state_reason: None,
            config_loaded: false,
            config: HashMap::new(),
            sessions: HashMap::new(),
            work_queue: Vec::new(),
            active_work: None,
            completed_work: Vec::new(),
            pending_events: Vec::new(),
            event_history: Vec::new(),
            thread: Vec::new(),
            aliases: Vec::new(),
            adapter_type: None,
            created_at: Utc::now(),
            last_activity: None,
        }
    }

    /// Validates an alias name.
    ///
    /// Aliases must be:
    /// - Alphanumeric with optional dash/underscore
    /// - Not empty
    /// - Between 1 and 64 characters
    fn validate_alias(alias: &str) -> Result<(), String> {
        if alias.is_empty() {
            return Err("Alias cannot be empty".to_string());
        }

        if alias.len() > 64 {
            return Err("Alias cannot exceed 64 characters".to_string());
        }

        // Must be alphanumeric with optional dash/underscore (same rules as project names)
        if !alias
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(
                "Alias must be alphanumeric with optional dash or underscore".to_string(),
            );
        }

        Ok(())
    }

    /// Adds an alias to this project.
    ///
    /// Returns an error if:
    /// - Alias is invalid (validation failed)
    /// - Alias already exists for this project
    /// - Maximum alias count (10) exceeded
    pub fn add_alias(&mut self, alias: String) -> Result<(), String> {
        // Validate alias format
        Self::validate_alias(&alias)?;

        // Check if alias already exists
        if self.aliases.contains(&alias) {
            return Err(format!("Alias '{}' already exists for this project", alias));
        }

        // Check maximum alias count
        if self.aliases.len() >= 10 {
            return Err("Maximum 10 aliases per project".to_string());
        }

        self.aliases.push(alias);
        self.aliases.sort();
        self.touch();

        Ok(())
    }

    /// Removes an alias from this project.
    ///
    /// Returns true if the alias was found and removed, false otherwise.
    pub fn remove_alias(&mut self, alias: &str) -> bool {
        if let Some(pos) = self.aliases.iter().position(|a| a == alias) {
            self.aliases.remove(pos);
            self.touch();
            true
        } else {
            false
        }
    }

    /// Checks if this project matches a name or alias.
    ///
    /// Matches against:
    /// - Project name (exact match)
    /// - Project ID (exact match)
    /// - Any alias (exact match)
    pub fn matches(&self, name_or_alias: &str) -> bool {
        self.name == name_or_alias
            || self.id.as_str() == name_or_alias
            || self.aliases.iter().any(|a| a == name_or_alias)
    }

    /// Updates the project's last activity timestamp.
    pub fn touch(&mut self) {
        self.last_activity = Some(Utc::now());
    }

    /// Adds a work item to the queue.
    pub fn enqueue_work(&mut self, work: WorkItem) {
        self.work_queue.push(work);
        self.touch();
    }

    /// Adds an event to the pending events.
    pub fn add_event(&mut self, event: Event) {
        self.pending_events.push(event);
        self.touch();
    }

    /// Adds a message to the thread.
    pub fn add_message(&mut self, message: ThreadMessage) {
        self.thread.push(message);
        self.touch();
    }

    /// Returns true if the project has any blocking events.
    pub fn has_blocking_events(&self) -> bool {
        self.pending_events.iter().any(|e| e.is_blocking())
    }

    /// Sets the project state with an optional reason.
    pub fn set_state(&mut self, state: ProjectState, reason: Option<String>) {
        self.state = state;
        self.state_reason = reason;
        self.touch();
    }

    /// Returns the adapter type, defaulting to ClaudeCode if not set.
    pub fn effective_adapter_type(&self) -> AdapterType {
        self.adapter_type.unwrap_or_default()
    }

    /// Returns the tmux session name for this project.
    /// Uses the project name directly (no prefix).
    /// Sanitizes characters that tmux doesn't allow in session names.
    pub fn session_name(&self) -> String {
        self.name.replace([' ', '.', '/', ':'], "-")
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new(".".to_string(), "default".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventStatus, EventType};

    #[test]
    fn test_project_state_default() {
        assert_eq!(ProjectState::default(), ProjectState::Idle);
    }

    #[test]
    fn test_project_creation() {
        let project = Project::new("/path/to/project", "my-project");

        assert!(project.id.as_str().starts_with("proj-"));
        assert_eq!(project.path, "/path/to/project");
        assert_eq!(project.name, "my-project");
        assert_eq!(project.state, ProjectState::Idle);
        assert!(project.work_queue.is_empty());
        assert!(project.pending_events.is_empty());
        assert!(project.thread.is_empty());
    }

    #[test]
    fn test_project_default() {
        let project = Project::default();

        assert_eq!(project.path, ".");
        assert_eq!(project.name, "default");
        assert_eq!(project.state, ProjectState::Idle);
    }

    #[test]
    fn test_project_touch() {
        let mut project = Project::new("/path", "test");
        assert!(project.last_activity.is_none());

        project.touch();
        assert!(project.last_activity.is_some());
    }

    #[test]
    fn test_project_set_state() {
        let mut project = Project::new("/path", "test");

        project.set_state(ProjectState::Working, Some("Processing tasks".to_string()));

        assert_eq!(project.state, ProjectState::Working);
        assert_eq!(project.state_reason, Some("Processing tasks".to_string()));
    }

    #[test]
    fn test_project_has_blocking_events_none() {
        let project = Project::new("/path", "test");
        assert!(!project.has_blocking_events());
    }

    #[test]
    fn test_project_has_blocking_events_with_error() {
        let mut project = Project::new("/path", "test");
        let event = Event::new(
            project.id.clone(),
            EventType::Error,
            "Error occurred",
        );
        project.add_event(event);

        assert!(project.has_blocking_events());
    }

    #[test]
    fn test_project_has_blocking_events_with_resolved_error() {
        let mut project = Project::new("/path", "test");
        let mut event = Event::new(
            project.id.clone(),
            EventType::Error,
            "Error occurred",
        );
        event.status = EventStatus::Resolved;
        project.add_event(event);

        assert!(!project.has_blocking_events());
    }

    #[test]
    fn test_project_has_blocking_events_with_status() {
        let mut project = Project::new("/path", "test");
        let event = Event::new(
            project.id.clone(),
            EventType::Status,
            "Status update",
        );
        project.add_event(event);

        assert!(!project.has_blocking_events());
    }

    #[test]
    fn test_tool_session_creation() {
        let session = ToolSession::new("project-1");

        assert!(session.id.as_str().starts_with("sess-"));
        assert_eq!(session.project_id.as_str(), "project-1");
        assert_eq!(session.status, "created");
        assert!(session.output_buffer.is_empty());
    }

    #[test]
    fn test_tool_session_append_output() {
        let mut session = ToolSession::new("project-1");
        assert!(session.last_output_at.is_none());

        session.append_output("Hello, world!".to_string());

        assert_eq!(session.output_buffer.len(), 1);
        assert_eq!(session.output_buffer[0], "Hello, world!");
        assert!(session.last_output_at.is_some());
    }

    #[test]
    fn test_thread_message_creation() {
        let msg = ThreadMessage::new("user", "Hello");

        assert!(msg.id.as_str().starts_with("msg-"));
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_thread_message_user() {
        let msg = ThreadMessage::user("User message");

        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "User message");
    }

    #[test]
    fn test_thread_message_assistant() {
        let msg = ThreadMessage::assistant("Assistant message");

        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "Assistant message");
    }

    #[test]
    fn test_project_add_message() {
        let mut project = Project::new("/path", "test");
        let msg = ThreadMessage::user("Hello");

        project.add_message(msg);

        assert_eq!(project.thread.len(), 1);
        assert_eq!(project.thread[0].content, "Hello");
    }

    #[test]
    fn test_project_state_serialization() {
        let json = serde_json::to_string(&ProjectState::Working).unwrap();
        assert_eq!(json, "\"working\"");

        let deserialized: ProjectState = serde_json::from_str("\"working\"").unwrap();
        assert_eq!(deserialized, ProjectState::Working);
    }

    #[test]
    fn test_tool_session_serialization_roundtrip() {
        let mut session = ToolSession::new("project-1");
        session.runtime = Some("python".to_string());
        session.tmux_target = Some("project:0".to_string());
        session.append_output("output line".to_string());

        let json = serde_json::to_string(&session).unwrap();
        let deserialized: ToolSession = serde_json::from_str(&json).unwrap();

        assert_eq!(session.id, deserialized.id);
        assert_eq!(session.project_id, deserialized.project_id);
        assert_eq!(session.runtime, deserialized.runtime);
        assert_eq!(session.tmux_target, deserialized.tmux_target);
        assert_eq!(session.status, deserialized.status);
        assert_eq!(session.output_buffer, deserialized.output_buffer);
    }

    #[test]
    fn test_thread_message_serialization_roundtrip() {
        use crate::ids::{EventId, SessionId};

        let mut msg = ThreadMessage::user("Hello");
        msg.session_id = Some(SessionId::from("session-1"));
        msg.event_id = Some(EventId::from("event-1"));

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ThreadMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(msg.id, deserialized.id);
        assert_eq!(msg.role, deserialized.role);
        assert_eq!(msg.content, deserialized.content);
        assert_eq!(msg.session_id, deserialized.session_id);
        assert_eq!(msg.event_id, deserialized.event_id);
    }

    #[test]
    fn test_project_serialization_roundtrip() {
        let mut project = Project::new("/path/to/project", "my-project");
        project.set_state(ProjectState::Working, Some("Processing".to_string()));
        project
            .config
            .insert("key".to_string(), serde_json::json!("value"));

        let json = serde_json::to_string(&project).unwrap();
        let deserialized: Project = serde_json::from_str(&json).unwrap();

        assert_eq!(project.id, deserialized.id);
        assert_eq!(project.path, deserialized.path);
        assert_eq!(project.name, deserialized.name);
        assert_eq!(project.state, deserialized.state);
        assert_eq!(project.state_reason, deserialized.state_reason);
    }

    // === Alias Tests ===

    #[test]
    fn test_validate_alias_valid() {
        assert!(Project::validate_alias("prod").is_ok());
        assert!(Project::validate_alias("staging").is_ok());
        assert!(Project::validate_alias("dev-1").is_ok());
        assert!(Project::validate_alias("my_alias").is_ok());
        assert!(Project::validate_alias("test123").is_ok());
    }

    #[test]
    fn test_validate_alias_empty() {
        let result = Project::validate_alias("");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Alias cannot be empty");
    }

    #[test]
    fn test_validate_alias_too_long() {
        let long_alias = "a".repeat(65);
        let result = Project::validate_alias(&long_alias);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Alias cannot exceed 64 characters");
    }

    #[test]
    fn test_validate_alias_invalid_characters() {
        let result = Project::validate_alias("prod@staging");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("alphanumeric with optional dash or underscore"));
    }

    #[test]
    fn test_add_alias_success() {
        let mut project = Project::new("/path", "test");

        let result = project.add_alias("prod".to_string());
        assert!(result.is_ok());
        assert_eq!(project.aliases.len(), 1);
        assert!(project.aliases.contains(&"prod".to_string()));
    }

    #[test]
    fn test_add_alias_sorted() {
        let mut project = Project::new("/path", "test");

        project.add_alias("staging".to_string()).unwrap();
        project.add_alias("prod".to_string()).unwrap();
        project.add_alias("dev".to_string()).unwrap();

        assert_eq!(project.aliases, vec!["dev", "prod", "staging"]);
    }

    #[test]
    fn test_add_alias_duplicate() {
        let mut project = Project::new("/path", "test");
        project.add_alias("prod".to_string()).unwrap();

        let result = project.add_alias("prod".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn test_add_alias_max_limit() {
        let mut project = Project::new("/path", "test");

        // Add 10 aliases (maximum)
        for i in 0..10 {
            project.add_alias(format!("alias{}", i)).unwrap();
        }

        // Try to add 11th alias
        let result = project.add_alias("alias11".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Maximum 10 aliases"));
    }

    #[test]
    fn test_add_alias_invalid_format() {
        let mut project = Project::new("/path", "test");

        let result = project.add_alias("prod@staging".to_string());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("alphanumeric with optional dash or underscore"));
    }

    #[test]
    fn test_remove_alias_success() {
        let mut project = Project::new("/path", "test");
        project.add_alias("prod".to_string()).unwrap();

        let removed = project.remove_alias("prod");
        assert!(removed);
        assert!(project.aliases.is_empty());
    }

    #[test]
    fn test_remove_alias_not_found() {
        let mut project = Project::new("/path", "test");

        let removed = project.remove_alias("nonexistent");
        assert!(!removed);
    }

    #[test]
    fn test_matches_by_name() {
        let project = Project::new("/path", "test-project");
        assert!(project.matches("test-project"));
        assert!(!project.matches("other-project"));
    }

    #[test]
    fn test_matches_by_id() {
        let project = Project::new("/path", "test");
        let id = project.id.as_str();

        assert!(project.matches(id));
    }

    #[test]
    fn test_matches_by_alias() {
        let mut project = Project::new("/path", "test");
        project.add_alias("prod".to_string()).unwrap();
        project.add_alias("staging".to_string()).unwrap();

        assert!(project.matches("prod"));
        assert!(project.matches("staging"));
        assert!(!project.matches("dev"));
    }

    #[test]
    fn test_aliases_serialization_roundtrip() {
        let mut project = Project::new("/path", "test");
        project.add_alias("prod".to_string()).unwrap();
        project.add_alias("staging".to_string()).unwrap();

        let json = serde_json::to_string(&project).unwrap();
        let deserialized: Project = serde_json::from_str(&json).unwrap();

        assert_eq!(project.aliases, deserialized.aliases);
    }

    #[test]
    fn test_aliases_backward_compatible() {
        // Old JSON without aliases or adapter_type field should deserialize with empty aliases
        // and adapter_type: None.
        let json = r#"{
            "id": "proj-123",
            "path": "/path",
            "name": "test",
            "state": "idle",
            "config_loaded": false,
            "config": {},
            "sessions": {},
            "work_queue": [],
            "completed_work": [],
            "pending_events": [],
            "event_history": [],
            "thread": [],
            "created_at": "2024-01-01T00:00:00Z"
        }"#;

        let project: Project = serde_json::from_str(json).unwrap();
        assert!(project.aliases.is_empty());
        assert_eq!(project.adapter_type, None);
    }

    // === AdapterType Tests ===

    #[test]
    fn test_adapter_type_serialization_roundtrip() {
        let cases = [
            (AdapterType::ClaudeCode, "\"claude-code\""),
            (AdapterType::ClaudeMpm, "\"claude-mpm\""),
            (AdapterType::Auggie, "\"auggie\""),
            (AdapterType::Codex, "\"codex\""),
            (AdapterType::Shell, "\"shell\""),
        ];

        for (variant, expected_json) in cases {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_json, "serialization mismatch for {:?}", variant);

            let deserialized: AdapterType = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant, "deserialization mismatch for {:?}", variant);
        }
    }

    #[test]
    fn test_adapter_type_display() {
        assert_eq!(AdapterType::ClaudeCode.to_string(), "claude-code");
        assert_eq!(AdapterType::ClaudeMpm.to_string(), "claude-mpm");
        assert_eq!(AdapterType::Auggie.to_string(), "auggie");
        assert_eq!(AdapterType::Codex.to_string(), "codex");
        assert_eq!(AdapterType::Shell.to_string(), "shell");
    }

    #[test]
    fn test_effective_adapter_type_default() {
        let project = Project::new("/path", "test");
        // adapter_type is None, so effective_adapter_type should default to ClaudeCode.
        assert_eq!(project.adapter_type, None);
        assert_eq!(project.effective_adapter_type(), AdapterType::ClaudeCode);
    }

    #[test]
    fn test_effective_adapter_type_explicit() {
        let mut project = Project::new("/path", "test");
        project.adapter_type = Some(AdapterType::Shell);
        assert_eq!(project.effective_adapter_type(), AdapterType::Shell);
    }

    #[test]
    fn test_session_name_simple() {
        let project = Project::new("/path", "my-project");
        assert_eq!(project.session_name(), "my-project");
    }

    #[test]
    fn test_session_name_sanitizes_spaces() {
        let project = Project::new("/path", "my project");
        assert_eq!(project.session_name(), "my-project");
    }

    #[test]
    fn test_session_name_sanitizes_dots() {
        let project = Project::new("/path", "my.project");
        assert_eq!(project.session_name(), "my-project");
    }

    #[test]
    fn test_session_name_sanitizes_slashes() {
        let project = Project::new("/path", "org/repo");
        assert_eq!(project.session_name(), "org-repo");
    }

    #[test]
    fn test_session_name_sanitizes_colons() {
        let project = Project::new("/path", "host:port");
        assert_eq!(project.session_name(), "host-port");
    }

    #[test]
    fn test_session_name_sanitizes_multiple_chars() {
        let project = Project::new("/path", "my project/v1.0:beta");
        assert_eq!(project.session_name(), "my-project-v1-0-beta");
    }

    #[test]
    fn test_adapter_type_backward_compatible() {
        // JSON with adapter_type field missing should deserialize with adapter_type: None.
        let json = r#"{
            "id": "proj-456",
            "path": "/some/path",
            "name": "my-project",
            "state": "idle",
            "config_loaded": false,
            "config": {},
            "sessions": {},
            "work_queue": [],
            "completed_work": [],
            "pending_events": [],
            "event_history": [],
            "thread": [],
            "aliases": [],
            "created_at": "2024-06-01T00:00:00Z"
        }"#;

        let project: Project = serde_json::from_str(json).unwrap();
        assert_eq!(project.adapter_type, None);
        assert_eq!(project.effective_adapter_type(), AdapterType::ClaudeCode);
    }

    #[test]
    fn test_adapter_type_from_str_valid() {
        use std::str::FromStr;

        assert_eq!(AdapterType::from_str("claude-code").unwrap(), AdapterType::ClaudeCode);
        assert_eq!(AdapterType::from_str("cc").unwrap(), AdapterType::ClaudeCode);
        assert_eq!(AdapterType::from_str("claude-mpm").unwrap(), AdapterType::ClaudeMpm);
        assert_eq!(AdapterType::from_str("mpm").unwrap(), AdapterType::ClaudeMpm);
        assert_eq!(AdapterType::from_str("auggie").unwrap(), AdapterType::Auggie);
        assert_eq!(AdapterType::from_str("augment").unwrap(), AdapterType::Auggie);
        assert_eq!(AdapterType::from_str("codex").unwrap(), AdapterType::Codex);
        assert_eq!(AdapterType::from_str("shell").unwrap(), AdapterType::Shell);
        assert_eq!(AdapterType::from_str("sh").unwrap(), AdapterType::Shell);
    }

    #[test]
    fn test_adapter_type_from_str_invalid() {
        use std::str::FromStr;

        let err = AdapterType::from_str("unknown").unwrap_err();
        assert!(err.contains("Unknown adapter type"));
        assert!(err.contains("unknown"));
    }

    #[test]
    fn test_adapter_type_not_serialized_when_none() {
        let project = Project::new("/path", "test");
        let json = serde_json::to_string(&project).unwrap();
        // adapter_type: None should be skipped in serialized output.
        assert!(!json.contains("adapter_type"));
    }

    #[test]
    fn test_adapter_type_serialized_when_set() {
        let mut project = Project::new("/path", "test");
        project.adapter_type = Some(AdapterType::ClaudeMpm);
        let json = serde_json::to_string(&project).unwrap();
        assert!(json.contains("\"adapter_type\":\"claude-mpm\""));
    }
}
