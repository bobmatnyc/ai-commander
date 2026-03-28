//! Core types for the MPM SDK.

/// Information about a registered MPM agent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// A spawned agent task in flight.
#[derive(Debug, Clone)]
pub struct AgentTask {
    pub id: String,
    pub agent_id: String,
    pub prompt: String,
    pub started_at: std::time::Instant,
}

/// The completed result of an agent task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentResult {
    pub text: String,
    pub session_id: Option<String>,
    pub cost_usd: Option<f64>,
    pub duration_ms: u64,
    pub is_error: bool,
    pub num_turns: Option<u32>,
    pub runtime: Option<String>,
}

/// Session created/managed by the ui_service daemon (port 7777).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServeSession {
    pub id: String,
    pub claude_session_id: Option<String>,
    pub status: String,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub project_root: Option<String>,
    pub created_at: Option<String>,
}

/// Request body for creating a session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct CreateSessionRequest {
    pub resume_id: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub project_root: Option<String>,
    pub bare: Option<bool>,
    pub permission_mode: Option<String>,
}

/// A streaming event from POST /api/v1/sessions/{id}/messages with stream=true.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ServeStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub content: Option<String>,
    pub name: Option<String>,
}

/// Context usage for a session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionContext {
    pub tokens_used: u64,
    pub tokens_total: u64,
    pub percent_used: f64,
    pub compaction_recommended: bool,
}

/// Streaming event from a running agent.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A chunk of partial output text.
    Text(String),
    /// A tool is being invoked (tool name).
    ToolUse(String),
    /// Agent completed successfully.
    Complete(AgentResult),
    /// Agent encountered an error.
    Error(String),
}

/// MPM system status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MpmStatus {
    pub version: String,
    pub binary_path: String,
    pub agent_count: usize,
    pub healthy: bool,
}

/// Errors from the MPM SDK.
#[derive(Debug, thiserror::Error)]
pub enum MpmError {
    #[error("MPM binary not found: {0}")]
    BinaryNotFound(String),
    #[error("Spawn failed: {0}")]
    SpawnFailed(#[from] std::io::Error),
    #[error("Timeout after {0}s")]
    Timeout(u64),
    #[error("Agent error: {0}")]
    AgentError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("HTTP error: {0}")]
    HttpError(String),
}
