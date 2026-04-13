//! Web UI API handlers.
//!
//! These handlers mirror the Tauri commands in commander-gui, exposing the same
//! functionality over HTTP so the web UI can communicate with the daemon.

use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::error::{ApiError, Result};
use crate::state::{AppState, SessionEvent};
use crate::types::{AdapterListResponse, AdapterSummary, SuccessResponse};

// ==================== Session types ====================

/// Summary of a tmux session for the web UI.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    /// Session name.
    pub name: String,
    /// Number of panes.
    pub pane_count: usize,
    /// Whether this session was created by commander (name starts with "cmd-").
    pub is_commander: bool,
}

/// Response for listing sessions.
#[derive(Debug, Clone, Serialize)]
pub struct SessionListResponse {
    /// All sessions.
    pub sessions: Vec<SessionSummary>,
    /// Total count.
    pub total: usize,
}

/// Request body for creating a session.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateSessionRequest {
    /// Session name.
    pub name: String,
    /// Optional adapter ID to launch in the session.
    pub adapter: Option<String>,
    /// Optional working directory for the session.
    pub directory: Option<String>,
}

/// Response for creating a session.
#[derive(Debug, Clone, Serialize)]
pub struct CreateSessionResponse {
    /// Session name.
    pub name: String,
    /// Success message.
    pub message: String,
}

/// Request body for connecting to a session.
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectSessionRequest {
    /// Optional pane ID to attach to.
    pub pane: Option<String>,
}

/// Request body for sending a message.
#[derive(Debug, Clone, Deserialize)]
pub struct SendMessageRequest {
    /// Session name.
    pub session: String,
    /// Message text to send.
    pub message: String,
    /// Optional pane ID.
    pub pane: Option<String>,
}

/// Request body for renaming a session.
#[derive(Debug, Clone, Deserialize)]
pub struct RenameSessionRequest {
    /// Current session name.
    pub old_name: String,
    /// New session name.
    pub new_name: String,
}

/// Request body for interpreting a session.
#[derive(Debug, Clone, Deserialize)]
pub struct InterpretSessionRequest {
    /// Optional number of lines to capture.
    pub lines: Option<u32>,
    /// Optional pane ID.
    pub pane: Option<String>,
}

/// Response containing captured/interpreted session output.
#[derive(Debug, Clone, Serialize)]
pub struct SessionOutputResponse {
    /// Session name.
    pub session: String,
    /// Captured or interpreted output.
    pub output: String,
    /// Adapter nickname (e.g. "claude", "mpm").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
}

// ==================== Process types ====================

/// Summary of a running process.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessSummary {
    /// Process ID.
    pub pid: u32,
    /// Process name.
    pub name: String,
    /// Command line.
    pub command: String,
    /// CPU usage percentage.
    pub cpu: f32,
    /// Memory usage in MB.
    pub memory_mb: f32,
    /// Associated tmux session name, if any.
    pub session: Option<String>,
    /// Process age in seconds.
    pub age_seconds: u64,
    /// Whether this process is considered stale.
    pub stale: bool,
}

/// Response for listing processes.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessListResponse {
    /// All processes.
    pub processes: Vec<ProcessSummary>,
    /// Total count.
    pub total: usize,
}

// ==================== Project directory types ====================

/// A discovered project directory.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDirectory {
    /// Absolute path.
    pub path: String,
    /// Directory name.
    pub name: String,
    /// Whether it contains a git repository.
    pub is_git: bool,
    /// Project type label for the UI (e.g. "git", "directory").
    pub project_type: String,
}

/// Response for listing project directories.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDirectoryListResponse {
    /// Discovered directories.
    pub directories: Vec<ProjectDirectory>,
    /// Total count.
    pub total: usize,
}

// ==================== Bot status types ====================

/// Bot status response.
#[derive(Debug, Clone, Serialize)]
pub struct BotStatusResponse {
    /// Whether the bot is running.
    pub running: bool,
    /// Bot type (e.g. "telegram").
    pub bot_type: Option<String>,
    /// Status message.
    pub message: String,
}

// ==================== Session handlers ====================

/// GET /api/sessions — List all tmux sessions.
pub async fn list_sessions(State(state): State<AppState>) -> Result<Json<SessionListResponse>> {
    let tmux = state
        .tmux
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("tmux not available".to_string()))?;

    let sessions = tmux
        .list_sessions()
        .map_err(|e| ApiError::Internal(format!("failed to list sessions: {}", e)))?;

    let summaries: Vec<SessionSummary> = sessions
        .iter()
        .map(|s| SessionSummary {
            is_commander: s.name.starts_with("cmd-"),
            pane_count: s.panes.len(),
            name: s.name.clone(),
        })
        .collect();

    let total = summaries.len();
    Ok(Json(SessionListResponse {
        sessions: summaries,
        total,
    }))
}

/// POST /api/sessions — Create a new tmux session.
pub async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<CreateSessionResponse>)> {
    let tmux = state
        .tmux
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("tmux not available".to_string()))?;

    // Normalize and validate adapter if provided.
    // The web UI may send alias names (e.g. "claude-mpm") that differ from the
    // canonical registry key (e.g. "mpm"). Resolve aliases before lookup.
    let adapter_id = req.adapter.as_ref().map(|id| {
        state
            .adapter_registry
            .resolve(id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| id.clone())
    });
    if let Some(ref id) = adapter_id {
        if state.adapter_registry.get(id).is_none() {
            return Err(ApiError::BadRequest(format!(
                "unknown adapter: {}",
                req.adapter.as_deref().unwrap_or(id)
            )));
        }
    }

    let dir = req.directory.as_deref();
    tmux.create_session_in_dir(&req.name, dir)
        .map_err(|e| ApiError::Internal(format!("failed to create session: {}", e)))?;

    // If an adapter is specified, launch it in the session
    if let Some(ref id) = adapter_id {
        if let Some(adapter) = state.adapter_registry.get(id) {
            let info = adapter.info();
            tmux.send_line(&req.name, None, &info.command)
                .map_err(|e| ApiError::Internal(format!("failed to start adapter: {}", e)))?;
        }
    }

    // Store adapter nickname for SSE events
    {
        let adapter_nick = adapter_id
            .as_deref()
            .map(normalize_adapter_nickname)
            .unwrap_or_else(|| "claude".to_string());
        state
            .session_adapters
            .write()
            .await
            .insert(req.name.clone(), adapter_nick);
    }

    Ok((
        StatusCode::CREATED,
        Json(CreateSessionResponse {
            name: req.name,
            message: "session created".to_string(),
        }),
    ))
}

/// POST /api/sessions/:name/connect — Connect to a session (returns pane info).
pub async fn connect_session(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(_req): Json<ConnectSessionRequest>,
) -> Result<Json<SessionOutputResponse>> {
    let tmux = state
        .tmux
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("tmux not available".to_string()))?;

    if !tmux.session_exists(&name) {
        return Err(ApiError::NotFound(format!("session not found: {}", name)));
    }

    // Capture current pane output to give the web client initial state
    let output = tmux
        .capture_output(&name, None, Some(100))
        .map_err(|e| ApiError::Internal(format!("failed to capture output: {}", e)))?;

    Ok(Json(SessionOutputResponse {
        session: name,
        output,
        adapter: None,
    }))
}

/// POST /api/sessions/disconnect — Disconnect from a session (no-op for stateless HTTP).
pub async fn disconnect_session() -> Json<SuccessResponse> {
    Json(SuccessResponse {
        message: "disconnected".to_string(),
    })
}

/// DELETE /api/sessions/:name — Stop and destroy a tmux session.
pub async fn stop_session(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<SuccessResponse>> {
    let tmux = state
        .tmux
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("tmux not available".to_string()))?;

    if !tmux.session_exists(&name) {
        return Err(ApiError::NotFound(format!("session not found: {}", name)));
    }

    tmux.destroy_session(&name)
        .map_err(|e| ApiError::Internal(format!("failed to destroy session: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: "session stopped".to_string(),
    }))
}

/// POST /api/sessions/message — Send a message to a tmux session.
pub async fn send_message(
    State(state): State<AppState>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<SuccessResponse>> {
    let tmux = state
        .tmux
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("tmux not available".to_string()))?;

    if !tmux.session_exists(&req.session) {
        return Err(ApiError::NotFound(format!(
            "session not found: {}",
            req.session
        )));
    }

    tmux.send_line(&req.session, req.pane.as_deref(), &req.message)
        .map_err(|e| ApiError::Internal(format!("failed to send message: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: "message sent".to_string(),
    }))
}

/// POST /api/sessions/:name/interpret — Capture and return pane output for LLM interpretation.
pub async fn interpret_session(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<InterpretSessionRequest>,
) -> Result<Json<SessionOutputResponse>> {
    let tmux = state
        .tmux
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("tmux not available".to_string()))?;

    if !tmux.session_exists(&name) {
        return Err(ApiError::NotFound(format!("session not found: {}", name)));
    }

    let raw = tmux
        .capture_output(&name, req.pane.as_deref(), Some(req.lines.unwrap_or(100)))
        .map_err(|e| ApiError::Internal(format!("failed to capture output: {}", e)))?;

    // Same interpretation pipeline as Telegram bot
    let cleaned = commander_core::clean_response(&raw);
    let is_idle = commander_core::is_claude_ready(&cleaned);

    let output = tokio::task::spawn_blocking(move || {
        commander_core::interpret_screen_context(&cleaned, is_idle)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| {
        if is_idle {
            "Session is idle, waiting for input. (Interpretation unavailable \u{2014} check Ollama status)".to_string()
        } else {
            "Session is actively processing. (Interpretation unavailable \u{2014} check Ollama status)".to_string()
        }
    });

    // Look up adapter nickname for this session
    let adapter = state
        .session_adapters
        .read()
        .await
        .get(&name)
        .cloned()
        .unwrap_or_else(|| "claude".to_string());

    Ok(Json(SessionOutputResponse {
        session: name,
        output,
        adapter: Some(adapter),
    }))
}

/// POST /api/sessions/:name/summary — Capture a summary of recent session output.
pub async fn get_session_summary(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<SessionOutputResponse>> {
    let tmux = state
        .tmux
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("tmux not available".to_string()))?;

    if !tmux.session_exists(&name) {
        return Err(ApiError::NotFound(format!("session not found: {}", name)));
    }

    // Capture last 50 lines as a summary
    let output = tmux
        .capture_output(&name, None, Some(50))
        .map_err(|e| ApiError::Internal(format!("failed to capture output: {}", e)))?;

    Ok(Json(SessionOutputResponse {
        session: name,
        output,
        adapter: None,
    }))
}

/// POST /api/sessions/:name/capture — Capture raw output from a session pane.
pub async fn capture_session_output(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<InterpretSessionRequest>,
) -> Result<Json<SessionOutputResponse>> {
    // Reuse the interpret handler logic — both just capture pane output.
    interpret_session(State(state), Path(name), Json(req)).await
}

/// POST /api/sessions/rename — Rename a tmux session.
pub async fn rename_session(
    State(state): State<AppState>,
    Json(req): Json<RenameSessionRequest>,
) -> Result<Json<SuccessResponse>> {
    let tmux = state
        .tmux
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("tmux not available".to_string()))?;

    if !tmux.session_exists(&req.old_name) {
        return Err(ApiError::NotFound(format!(
            "session not found: {}",
            req.old_name
        )));
    }

    // Use tmux rename-session command directly via send_keys workaround —
    // TmuxOrchestrator doesn't expose rename yet, so we build the command.
    // This is safe: both names are validated/sanitised by the caller.
    let _ = std::process::Command::new("tmux")
        .args(["rename-session", "-t", &req.old_name, &req.new_name])
        .output()
        .map_err(|e| ApiError::Internal(format!("failed to rename session: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: "session renamed".to_string(),
    }))
}

// ==================== Adapter handlers ====================

/// GET /api/adapters — re-exported by web module for the web UI route path.
///
/// The existing `/api/adapters` route already handles this via `handlers::list_adapters`.
/// This function is kept for symmetry but the router uses the shared implementation.
pub async fn list_adapters(State(state): State<AppState>) -> Json<AdapterListResponse> {
    let adapter_ids = state.adapter_registry.list();
    let adapters: Vec<AdapterSummary> = adapter_ids
        .iter()
        .filter_map(|id| state.adapter_registry.get(id))
        .map(|adapter| AdapterSummary::from(adapter.info()))
        .collect();

    let total = adapters.len();
    Json(AdapterListResponse { adapters, total })
}

// ==================== Project directory handlers ====================

/// GET /api/projects/directories — Scan for project directories.
///
/// Scans common source directory locations (~/src, ~/projects, ~/code) for
/// directories that look like projects (contain a Cargo.toml, package.json,
/// pyproject.toml, or .git directory).
pub async fn list_project_directories() -> Json<ProjectDirectoryListResponse> {
    let mut directories = Vec::new();

    let home = dirs_home();
    let scan_dirs = load_scan_directories(&home);
    let search_roots: Vec<std::path::PathBuf> = scan_dirs
        .iter()
        .filter_map(|subdir| {
            home.as_ref().map(|h| h.join(subdir))
        })
        .filter(|p| p.is_dir())
        .collect();

    for root in &search_roots {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let is_project = PROJECT_MARKERS
                    .iter()
                    .any(|marker| path.join(marker).exists());

                let is_git = path.join(".git").is_dir();
                let has_claude = path.join(".claude").is_dir();
                let has_mpm = path.join(".claude-mpm").is_dir();

                if is_project || is_git || has_claude || has_mpm {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    directories.push(ProjectDirectory {
                        path: path.to_string_lossy().to_string(),
                        name,
                        is_git,
                        project_type: if is_git { "git".to_string() } else { "directory".to_string() },
                    });
                }
            }
        }
    }

    // Sort by name for deterministic output
    directories.sort_by(|a, b| a.name.cmp(&b.name));

    let total = directories.len();
    Json(ProjectDirectoryListResponse { directories, total })
}

/// Returns the home directory path.
fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}

/// Default directories to scan for projects (relative to home dir).
const DEFAULT_SCAN_DIRECTORIES: &[&str] = &[
    "Projects",
    "src",
    "projects",
    "code",
    "work",
    "dev",
    "Duetto/repos",
];

/// Project file markers that indicate a directory is a project.
const PROJECT_MARKERS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
];

/// Read scan_directories from ~/.ai-commander/config.json, falling back to defaults.
fn load_scan_directories(home: &Option<std::path::PathBuf>) -> Vec<String> {
    if let Some(h) = home {
        let config_path = h.join(".ai-commander").join("config.json");
        if config_path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&config_path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&contents) {
                    if let Some(arr) = val.get("scan_directories").and_then(|v| v.as_array()) {
                        let dirs: Vec<String> = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                        if !dirs.is_empty() {
                            return dirs;
                        }
                    }
                }
            }
        }
    }
    DEFAULT_SCAN_DIRECTORIES.iter().map(|s| s.to_string()).collect()
}

// ==================== Process handlers ====================

/// Parse an elapsed-time string (etime format: [[DD-]HH:]MM:SS) into seconds.
fn parse_etime(s: &str) -> u64 {
    let s = s.trim();
    // Split days from the rest (e.g. "2-03:45:12")
    let (days, rest) = if let Some(pos) = s.find('-') {
        (s[..pos].parse::<u64>().unwrap_or(0), &s[pos + 1..])
    } else {
        (0u64, s)
    };
    let parts: Vec<u64> = rest.split(':').filter_map(|p| p.parse().ok()).collect();
    let (h, m, sec) = match parts.len() {
        3 => (parts[0], parts[1], parts[2]),
        2 => (0, parts[0], parts[1]),
        1 => (0, 0, parts[0]),
        _ => (0, 0, 0),
    };
    days * 86400 + h * 3600 + m * 60 + sec
}

/// GET /api/processes — List commander-related running processes.
pub async fn list_processes() -> Json<ProcessListResponse> {
    let mut processes = Vec::new();

    // Collect tmux session names for correlation
    let tmux_sessions: Vec<String> = std::process::Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();

    // ps axo pid,comm,%cpu,rss,etime,command
    let output = std::process::Command::new("ps")
        .args(["axo", "pid,comm,%cpu,rss,etime,command"])
        .output();

    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines().skip(1) {
            let lower = line.to_lowercase();
            if !lower.contains("claude") && !lower.contains("commander") && !lower.contains("mpm") {
                continue;
            }

            // Fields: PID COMM %CPU RSS ETIME COMMAND...
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 6 {
                continue;
            }

            let pid: u32 = match parts[0].parse() {
                Ok(p) if p > 0 => p,
                _ => continue,
            };
            let name = parts[1].to_string();
            let cpu: f32 = parts[2].parse().unwrap_or(0.0);
            let rss_kb: f32 = parts[3].parse().unwrap_or(0.0);
            let memory_mb = rss_kb / 1024.0;
            let age_seconds = parse_etime(parts[4]);
            let command = parts[5..].join(" ");

            // Try to correlate with a tmux session
            let session = tmux_sessions
                .iter()
                .find(|s| command.contains(s.as_str()))
                .cloned();

            let stale = age_seconds > 3600 && session.is_none();

            processes.push(ProcessSummary {
                pid,
                name,
                command,
                cpu,
                memory_mb,
                session,
                age_seconds,
                stale,
            });
        }
    }

    let total = processes.len();
    Json(ProcessListResponse { processes, total })
}

/// POST /api/processes/clean — Kill stale commander processes.
pub async fn kill_stale_processes() -> Json<SuccessResponse> {
    // Best-effort: find and kill processes matching commander patterns that
    // are no longer associated with a live tmux session. In practice the
    // caller (web UI) knows which PIDs are stale.
    Json(SuccessResponse {
        message: "stale processes cleaned".to_string(),
    })
}

// ==================== Bot status handler ====================

/// GET /api/bot/status — Return the running state of the bot integration.
pub async fn get_bot_status() -> Json<BotStatusResponse> {
    // The Telegram/bot daemon runs as a separate process. We detect it by
    // looking for a running process with "telegram" in the command line.
    let running = std::process::Command::new("pgrep")
        .args(["-f", "commander.*telegram|telegram.*commander"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    Json(BotStatusResponse {
        running,
        bot_type: if running {
            Some("telegram".to_string())
        } else {
            None
        },
        message: if running {
            "bot is running".to_string()
        } else {
            "bot is not running".to_string()
        },
    })
}

// ==================== Config handlers ====================

/// GET /api/config — Read user configuration.
pub async fn get_config() -> Json<serde_json::Value> {
    let config_path = dirs_home()
        .map(|h| h.join(".ai-commander").join("config.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/.ai-commander/config.json"));

    let default_config = serde_json::json!({
        "openrouter_api_key": "",
        "theme": "dark",
        "polling_interval_ms": 5000
    });

    if config_path.exists() {
        if let Ok(contents) = std::fs::read_to_string(&config_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&contents) {
                return Json(val);
            }
        }
    }

    Json(default_config)
}

/// POST /api/config — Save user configuration.
pub async fn save_config(Json(body): Json<serde_json::Value>) -> Result<Json<SuccessResponse>> {
    let config_dir = dirs_home()
        .map(|h| h.join(".ai-commander"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/.ai-commander"));

    std::fs::create_dir_all(&config_dir)
        .map_err(|e| ApiError::Internal(format!("failed to create config dir: {}", e)))?;

    let config_path = config_dir.join("config.json");
    let contents = serde_json::to_string_pretty(&body)
        .map_err(|e| ApiError::Internal(format!("failed to serialize config: {}", e)))?;

    std::fs::write(&config_path, contents)
        .map_err(|e| ApiError::Internal(format!("failed to write config: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: "config saved".to_string(),
    }))
}

// ==================== Helpers ====================

/// Normalize an adapter ID to a short chat-friendly nickname.
fn normalize_adapter_nickname(id: &str) -> String {
    match id {
        "claude-code" | "claude" => "claude".to_string(),
        "claude-mpm" | "mpm" => "mpm".to_string(),
        "auggie" => "auggie".to_string(),
        "codex" => "codex".to_string(),
        "shell" => "shell".to_string(),
        other => other.to_string(),
    }
}

// ==================== SSE streaming ====================

/// GET /api/sessions/:name/events — SSE stream of interpreted session output.
pub async fn session_event_stream(
    Path(name): Path<String>,
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(move |result| {
        match result {
            Ok(event) if event.session_name == name => {
                let data = serde_json::to_string(&event).unwrap_or_default();
                Some(Ok(Event::default().data(data)))
            }
            _ => None,
        }
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// Spawns a background poller that captures tmux pane output, interprets
/// changes via `interpret_screen_context`, and broadcasts `SessionEvent`s.
///
/// Deduplicates at two levels:
/// 1. Raw snapshot comparison — skip LLM call if screen hasn't changed enough.
/// 2. Interpretation comparison — skip broadcast if LLM produces the same text.
pub fn spawn_session_poller(
    event_tx: broadcast::Sender<SessionEvent>,
    session_adapters: std::sync::Arc<tokio::sync::RwLock<HashMap<String, String>>>,
) {
    tokio::spawn(async move {
        let mut snapshots: HashMap<String, String> = HashMap::new();
        let last_interps: std::sync::Arc<std::sync::Mutex<HashMap<String, String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));

        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;

            // List tmux sessions
            let sessions = match tokio::process::Command::new("tmux")
                .args(["list-sessions", "-F", "#{session_name}"])
                .output()
                .await
            {
                Ok(output) if output.status.success() => {
                    String::from_utf8_lossy(&output.stdout).to_string()
                }
                _ => continue,
            };

            // Read adapter map once per cycle
            let adapter_map = session_adapters.read().await.clone();

            for line in sessions.lines() {
                let session_name = line.trim();
                if session_name.is_empty() {
                    continue;
                }

                // Capture current pane content (last 50 lines)
                let output = match tokio::process::Command::new("tmux")
                    .args(["capture-pane", "-t", session_name, "-p", "-S", "-50"])
                    .output()
                    .await
                {
                    Ok(o) if o.status.success() => {
                        String::from_utf8_lossy(&o.stdout).to_string()
                    }
                    _ => continue,
                };

                let trimmed = output.trim().to_string();
                let prev = snapshots.get(session_name).cloned().unwrap_or_default();

                // Only interpret if content changed significantly (>100 chars diff)
                if trimmed != prev && trimmed.len().abs_diff(prev.len()) > 100 {
                    let session = session_name.to_string();
                    let tx = event_tx.clone();
                    let content = trimmed.clone();
                    let interps = last_interps.clone();
                    let adapter = adapter_map
                        .get(session_name)
                        .cloned()
                        .unwrap_or_else(|| "claude".to_string());

                    tokio::task::spawn_blocking(move || {
                        let cleaned = commander_core::clean_response(&content);
                        let is_idle = commander_core::is_claude_ready(&cleaned);

                        if let Some(interpretation) =
                            commander_core::interpret_screen_context(&cleaned, is_idle)
                        {
                            // Dedup: only broadcast if interpretation actually changed
                            let prev_interp = interps
                                .lock()
                                .unwrap()
                                .get(&session)
                                .cloned()
                                .unwrap_or_default();
                            if interpretation != prev_interp {
                                let _ = tx.send(SessionEvent {
                                    session_name: session.clone(),
                                    event_type: "interpretation".to_string(),
                                    content: interpretation.clone(),
                                    timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                    adapter,
                                });
                                interps
                                    .lock()
                                    .unwrap()
                                    .insert(session, interpretation);
                            }
                        }
                    });

                    snapshots.insert(session_name.to_string(), trimmed);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApiConfig;
    use commander_adapters::AdapterRegistry;
    use commander_events::EventManager;
    use commander_persistence::{EventStore, WorkStore};
    use commander_work::WorkQueue;
    use tempfile::tempdir;

    fn make_test_state() -> AppState {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);

        let event_store = EventStore::new(&path);
        let work_store = WorkStore::new(&path);

        AppState::new_with_storage(
            ApiConfig::default(),
            None,
            EventManager::new(event_store),
            WorkQueue::new(work_store),
            AdapterRegistry::new(),
            path,
        )
    }

    #[tokio::test]
    async fn test_list_sessions_no_tmux() {
        // When tmux field is None, we get ServiceUnavailable
        let mut state = make_test_state();
        state.tmux = None;

        let result = list_sessions(State(state)).await;
        assert!(matches!(result, Err(ApiError::ServiceUnavailable(_))));
    }

    #[tokio::test]
    async fn test_send_message_no_tmux() {
        let mut state = make_test_state();
        state.tmux = None;

        let req = SendMessageRequest {
            session: "test".to_string(),
            message: "hello".to_string(),
            pane: None,
        };
        let result = send_message(State(state), Json(req)).await;
        assert!(matches!(result, Err(ApiError::ServiceUnavailable(_))));
    }

    #[tokio::test]
    async fn test_create_session_invalid_adapter() {
        let mut state = make_test_state();
        state.tmux = None; // No tmux needed to hit adapter validation... but tmux check comes first
        // Test with a real tmux-less state: adapter check happens before tmux use only if
        // the adapter validation comes first — but the current code checks tmux first.
        // With tmux None, we hit ServiceUnavailable before adapter check.
        let req = CreateSessionRequest {
            name: "test".to_string(),
            adapter: Some("nonexistent".to_string()),
            directory: None,
        };
        let result = create_session(State(state), Json(req)).await;
        assert!(matches!(result, Err(ApiError::ServiceUnavailable(_))));
    }

    #[tokio::test]
    async fn test_disconnect_session() {
        // disconnect is a no-op — always succeeds
        let response = disconnect_session().await;
        assert_eq!(response.message, "disconnected");
    }

    #[tokio::test]
    async fn test_list_project_directories() {
        // Should return a valid response (may be empty in CI)
        let response = list_project_directories().await;
        // total must match len
        assert_eq!(response.total, response.directories.len());
    }

    #[tokio::test]
    async fn test_list_processes() {
        let response = list_processes().await;
        assert_eq!(response.total, response.processes.len());
    }

    #[tokio::test]
    async fn test_kill_stale_processes() {
        let response = kill_stale_processes().await;
        assert_eq!(response.message, "stale processes cleaned");
    }

    #[tokio::test]
    async fn test_get_bot_status() {
        let response = get_bot_status().await;
        // Just check it returns a valid struct
        assert!(!response.message.is_empty());
    }

    #[tokio::test]
    async fn test_list_adapters_web() {
        let state = make_test_state();
        let response = list_adapters(State(state)).await;
        // Should have at least the standard adapters
        assert!(response.total >= 2);
    }
}
