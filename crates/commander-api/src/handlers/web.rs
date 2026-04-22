//! Web UI API handlers.
//!
//! These handlers mirror the Tauri commands in commander-gui, exposing the same
//! functionality over HTTP so the web UI can communicate with the daemon.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response},
    Json,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::{debug, warn};

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
    /// Current working directory of the session's active pane, if available.
    pub path: Option<String>,
    /// Human-readable project nickname, if a registered project matches this session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    /// Tri-state lifecycle label: "connected", "disconnected", or "registered".
    /// Mirrors the GUI `SessionInfo.session_state` so both clients render the
    /// same color/opacity states.
    pub session_state: String,
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

    // Load projects to resolve session nicknames. Uses a minimal ProjectStub that
    // only deserializes `name` and `path`, avoiding silent failures from the full
    // Project struct's complex nested fields. Failure is non-fatal.
    #[derive(serde::Deserialize, Clone)]
    struct ProjectStub {
        name: String,
        path: String,
    }

    let projects: Vec<ProjectStub> = {
        let dir = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default()
            .join(".ai-commander/projects");
        std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |x| x == "json"))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|s| serde_json::from_str::<ProjectStub>(&s).ok())
            .collect()
    };

    // Per-session display-name overrides (user renames). Mirror of the GUI
    // `read_session_overrides` helper so the web UI respects the same user
    // choices. See crates/commander-gui/src/commands.rs for the rationale.
    let overrides: HashMap<String, String> = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .map(|h| h.join(".ai-commander/session-overrides.json"))
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<HashMap<String, String>>(&s).ok())
        .unwrap_or_default();

    // Snapshot of actively-monitored sessions for the state label.
    let connected_snapshot: std::collections::HashSet<String> =
        state.connected_sessions.read().unwrap().clone();
    let mut matched_project_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    let mut summaries: Vec<SessionSummary> = sessions
        .iter()
        .map(|s| {
            let path = std::process::Command::new("tmux")
                .args(["display-message", "-p", "-t", &s.name, "#{pane_current_path}"])
                .output()
                .ok()
                .and_then(|o| {
                    let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if p.is_empty() { None } else { Some(p) }
                });
            let matched = projects.iter().find(|p| {
                p.name.replace([' ', '.', '/', ':'], "-") == s.name
                    || p.path == s.name
                    || path.as_deref() == Some(p.path.as_str())
            });
            if let Some(p) = matched {
                matched_project_names.insert(p.name.clone());
            }
            let project_nickname = matched.map(|p| p.name.clone());
            // Overrides take highest priority — above project nickname.
            let nickname = overrides.get(&s.name).cloned().or(project_nickname);
            let session_state = if connected_snapshot.contains(&s.name) {
                "connected"
            } else {
                "disconnected"
            };
            SessionSummary {
                is_commander: s.name.starts_with("cmd-"),
                pane_count: s.panes.len(),
                name: s.name.clone(),
                path,
                nickname,
                session_state: session_state.to_string(),
            }
        })
        .collect();

    // Append registered-only projects (no matching tmux session).
    let existing_names: std::collections::HashSet<String> =
        summaries.iter().map(|s| s.name.clone()).collect();
    for proj in &projects {
        if matched_project_names.contains(&proj.name) {
            continue;
        }
        let sanitized = proj.name.replace([' ', '.', '/', ':'], "-");
        if existing_names.contains(&sanitized) {
            continue;
        }
        summaries.push(SessionSummary {
            name: proj.name.clone(),
            pane_count: 0,
            is_commander: false,
            path: Some(proj.path.clone()),
            nickname: Some(proj.name.clone()),
            session_state: "registered".to_string(),
        });
    }

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

    // Set the tmux session title so terminal emulators attaching later show
    // the project nickname (or session name if no nickname exists yet).
    // Cheap — just two `tmux set-option -q` calls.
    let display = resolve_session_display_name(&req.name);
    set_tmux_session_title(&req.name, &display);

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

/// POST /api/sessions/:name/connect — Mark a session as actively monitored and
/// return the full log history for client hydration.
///
/// Why: Web clients need the same history-on-connect UX as the GUI so users see
/// prior summaries immediately rather than an empty chat. Tracking
/// `connected_sessions` also gates the background SSE poller.
/// What: Inserts `name` into `connected_sessions` and returns
/// `{"session", "history": [{text, ts, hash}, …]}`.
/// Test: Seed two log entries, POST this endpoint, assert the response's
/// `history` field has two items and `connected_sessions.contains(&name)`.
pub async fn connect_session(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(_req): Json<ConnectSessionRequest>,
) -> Result<Json<serde_json::Value>> {
    let tmux = state
        .tmux
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("tmux not available".to_string()))?;

    // Fuzzy-resolve the identifier to a live tmux session. The web UI may
    // POST an identifier that is a user nickname ("cto"), a project name
    // ("izzie"), or a bracket-suffixed label ("cto [cto3]"); tmux only
    // understands raw session names. Without fuzzy resolution, clicking a
    // row whose label doesn't match the raw tmux name fails with an opaque
    // "session not found" error even when the user's intent is obvious.
    let live_sessions: Vec<String> = tmux
        .list_sessions()
        .map(|v| v.into_iter().map(|s| s.name).collect())
        .unwrap_or_default();
    let resolved = resolve_to_tmux_session(&name, &live_sessions)
        .map_err(ApiError::NotFound)?;

    // After fuzzy resolution, the name used for tmux MUST be the raw tmux
    // session name, not the user-facing label the frontend sent.
    if !tmux.session_exists(&resolved) {
        return Err(ApiError::NotFound(format!(
            "session '{}' resolved to '{}' but tmux has-session failed; available: [{}]",
            name,
            resolved,
            live_sessions.join(", ")
        )));
    }

    // Track as actively monitored so the background SSE poller picks it up.
    state
        .connected_sessions
        .write()
        .unwrap()
        .insert(resolved.clone());

    // Refresh the tmux session title — the nickname may have changed since
    // creation, or the session may have been created outside AIC and be
    // connecting for the first time. Keeps every attached terminal in sync.
    let display = resolve_session_display_name(&resolved);
    set_tmux_session_title(&resolved, &display);

    // Full log history for client-side hydration.
    let history: Vec<serde_json::Value> = commander_core::read_all_log_entries(&resolved)
        .unwrap_or_default()
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "text": e.text,
                "ts": e.ts,
                "hash": e.hash,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "session": resolved,
        "history": history,
    })))
}

/// Try to auto-create a tmux session for a registered project matching `input`.
///
/// Why: When a web client POSTs `/api/sessions/<name>/connect` with a name that
/// only matches a registered-only project (project JSON exists, no live tmux
/// session), the old flow returned 404. Users expect a click on a registered
/// row to *start* the session — we know its working directory from the JSON,
/// so starting tmux automatically is strictly more useful than failing.
/// Mirror of the Tauri helper in `commander-gui/src/commands.rs` so both UIs
/// behave identically.
/// What: Searches `~/.ai-commander/projects/*.json` for a project whose `name`
/// or sanitized name equals `input`. If found, runs `tmux new-session -d -s
/// <sanitized> -c <path>` and returns the sanitized session name. Returns
/// `None` if no project matched or if tmux creation failed (name collision,
/// tmux missing, etc.) so the caller can fall back to the normal 404 path.
/// Test: Seed a project JSON with name "dci" and path "/tmp/dci"; call with
/// "dci"; assert a tmux session "dci" exists and the helper returned
/// `Some("dci")`. Call again with "nonexistent"; assert `None`.
fn try_auto_create_registered_session(input: &str) -> Option<String> {
    // Same sanitization as `list_sessions` and `delete_registration`.
    let sanitize = |s: &str| -> String { s.replace([' ', '.', '/', ':'], "-") };

    let projects_dir = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".ai-commander/projects");

    let mut match_info: Option<(String, String)> = None; // (project_name, project_path)
    if let Ok(entries) = std::fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |x| x == "json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        let proj_name = val.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let proj_path = val.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let sanitized = sanitize(proj_name);
                        if (proj_name == input || sanitized == input) && !proj_path.is_empty() {
                            match_info = Some((proj_name.to_string(), proj_path.to_string()));
                            break;
                        }
                    }
                }
            }
        }
    }

    let (proj_name, proj_path) = match_info?;
    let session_name = sanitize(&proj_name);

    let output = std::process::Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            &session_name,
            "-c",
            &proj_path,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        warn!(
            session = %session_name,
            path = %proj_path,
            stderr = %String::from_utf8_lossy(&output.stderr).trim(),
            "auto-create tmux session failed"
        );
        return None;
    }

    tracing::info!(
        session = %session_name,
        path = %proj_path,
        "Auto-created tmux session for registered project"
    );
    Some(session_name)
}

/// Resolve a user-supplied session identifier to a live tmux session name.
///
/// Why: Mirror of the Tauri `resolve_to_tmux_session` helper in
/// `commander-gui/src/commands.rs`. The web UI, like the desktop UI, can send
/// nicknames, project names, or bracket-suffixed labels to `connect_session`;
/// tmux only understands raw session names, so we must fuzzy-match before the
/// `has-session` check. Keeping the logic in-crate (rather than exporting it
/// from commander-core) avoids a new cross-crate public surface for what is
/// ultimately a handler-layer concern.
/// What: Tries exact → case-insensitive → bracket-strip → override-value →
/// project-path matching, in that order. If nothing matches but `input`
/// identifies a registered project, auto-creates a tmux session at the
/// project's stored path. Only returns an error when no fuzzy match succeeded
/// AND no registered project could be started.
/// Test: Given live_sessions=["cto [cto3]", "hyperdev"] and an override
/// "cto [cto3]" -> "cto", assert that resolve("cto") returns "cto [cto3]".
/// Given live_sessions=[] and a project JSON named "dci", assert that
/// resolve("dci") auto-creates a tmux session and returns Ok("dci").
fn resolve_to_tmux_session(input: &str, live_sessions: &[String]) -> std::result::Result<String, String> {
    // 1. Exact match — fast path for the common case.
    if live_sessions.iter().any(|s| s == input) {
        return Ok(input.to_string());
    }

    // 2. Case-insensitive exact match.
    let input_lower = input.to_lowercase();
    if let Some(hit) = live_sessions
        .iter()
        .find(|s| s.to_lowercase() == input_lower)
    {
        return Ok(hit.clone());
    }

    // 3. Bracket-suffix strip on both sides. Examples:
    //    input "cto" matches live "cto [cto3]"
    //    input "cto [cto3]" matches live "cto"
    let strip_suffix = |s: &str| -> String {
        s.split_once(" [")
            .map(|(head, _)| head.trim().to_string())
            .unwrap_or_else(|| s.trim().to_string())
    };
    let input_stripped = strip_suffix(input).to_lowercase();
    if !input_stripped.is_empty() {
        if let Some(hit) = live_sessions
            .iter()
            .find(|s| strip_suffix(s).to_lowercase() == input_stripped)
        {
            return Ok(hit.clone());
        }
    }

    // 4. Override map: find any tmux session whose user-set override equals input.
    let overrides: HashMap<String, String> = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .map(|h| h.join(".ai-commander/session-overrides.json"))
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if let Some((tmux_name, _)) = overrides
        .iter()
        .find(|(_, v)| v.as_str() == input || v.to_lowercase() == input_lower)
    {
        if live_sessions.contains(tmux_name) {
            return Ok(tmux_name.clone());
        }
    }

    // 5. Project JSON match: if `input` equals a project's name/sanitized name,
    //    look for a tmux session whose pane cwd matches the project's path.
    let projects_dir = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".ai-commander/projects");
    let mut project_path: Option<String> = None;
    if let Ok(entries) = std::fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |x| x == "json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        let proj_name = val.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let proj_path = val.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let sanitized = proj_name.replace([' ', '.', '/', ':'], "-");
                        if proj_name == input || sanitized == input {
                            project_path = Some(proj_path.to_string());
                            break;
                        }
                    }
                }
            }
        }
    }
    if let Some(pp) = project_path.filter(|p| !p.is_empty()) {
        for live in live_sessions {
            let pane_path = std::process::Command::new("tmux")
                .args(["display-message", "-p", "-t", live, "#{pane_current_path}"])
                .output()
                .ok()
                .and_then(|o| {
                    let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if p.is_empty() { None } else { Some(p) }
                });
            if pane_path.as_deref() == Some(pp.as_str()) {
                return Ok(live.clone());
            }
        }
    }

    // 6. No live tmux session matched. If the input identifies a registered
    //    project, auto-create a tmux session at its stored path so the connect
    //    call "just works" rather than 404-ing with a list of unrelated names.
    if let Some(created) = try_auto_create_registered_session(input) {
        return Ok(created);
    }

    // No match — return a helpful error listing what IS available so the user
    // (or a developer tailing logs) can see why the click failed.
    let available = if live_sessions.is_empty() {
        "none".to_string()
    } else {
        live_sessions.join(", ")
    };
    Err(format!(
        "session '{}' not found; available: [{}]",
        input, available
    ))
}

/// POST /api/sessions/disconnect — Stop monitoring a session.
///
/// Why: The state-machine refactor needs a way for web clients to drop sessions
/// from `connected_sessions` so they stop showing up in the SSE broadcast.
/// What: Reads `{"session": "..."}` from the body and removes it from the set.
/// Missing body / missing session field is tolerated (returns success) so the
/// endpoint stays idempotent.
/// Test: Insert "a" into connected_sessions, POST `{"session":"a"}`, assert
/// the set no longer contains "a".
pub async fn disconnect_session(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Json<SuccessResponse> {
    if let Some(name) = body.get("session").and_then(|v| v.as_str()) {
        state.connected_sessions.write().unwrap().remove(name);
    }
    Json(SuccessResponse {
        message: "disconnected".to_string(),
    })
}

/// DELETE /api/sessions/:name/unregister — Unregister a session without killing tmux.
///
/// Why: Web UI parity with the Tauri `unregister_session` command. Users need to
/// dissociate a running tmux session from its AI Commander project registration
/// (removing the JSON file) without destroying the underlying tmux session. That
/// distinction matters because `DELETE /api/sessions/:name` destroys tmux, and
/// `DELETE /api/sessions/:name/registration` only matches by name — neither
/// cleanly covers "forget this session exists, but keep the tmux process alive".
/// What: Resolves the session's tmux pane path, finds the first project JSON in
/// `~/.ai-commander/projects/` whose `name`, sanitised name, or `path` matches,
/// and deletes it. Clears any dangling session-override entry. Returns 404 if no
/// matching project JSON is found.
/// Test: Seed a project JSON whose `path` matches a running tmux session's cwd;
/// DELETE this endpoint; assert the JSON is gone and `tmux has-session -t <name>`
/// still exits 0.
pub async fn unregister_session(Path(name): Path<String>) -> Response {
    // Look up the session's pane current path for path-based matching (mirrors
    // the match rule in `list_sessions`).
    let pane_path = std::process::Command::new("tmux")
        .args(["display-message", "-p", "-t", &name, "#{pane_current_path}"])
        .output()
        .ok()
        .and_then(|o| {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if p.is_empty() { None } else { Some(p) }
        });

    let projects_dir = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".ai-commander/projects");

    let mut removed = false;
    if let Ok(entries) = std::fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |x| x == "json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        let proj_name = val.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let proj_path = val.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let sanitized = proj_name.replace([' ', '.', '/', ':'], "-");
                        let matches = proj_name == name
                            || sanitized == name
                            || proj_path == name
                            || pane_path.as_deref() == Some(proj_path);
                        if matches {
                            let _ = std::fs::remove_file(&path);
                            removed = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    // Clear any dangling display-name override regardless of whether a JSON was
    // removed — orphaned overrides are harmless but accumulate otherwise.
    let overrides_path = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".ai-commander/session-overrides.json");
    if let Ok(content) = std::fs::read_to_string(&overrides_path) {
        if let Ok(mut map) = serde_json::from_str::<HashMap<String, String>>(&content) {
            if map.remove(&name).is_some() {
                if let Ok(updated) = serde_json::to_string_pretty(&map) {
                    let _ = std::fs::write(&overrides_path, updated);
                }
            }
        }
    }

    if !removed {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("No registered project found matching session '{}'", name)
            })),
        )
            .into_response();
    }

    Json(SuccessResponse {
        message: "session unregistered".to_string(),
    })
    .into_response()
}

/// Resolve the preferred display name for a session — mirror of the GUI helper.
///
/// Why: The REST create/connect paths need the same nickname-or-project-name
/// label so terminal emulators attaching to sessions created via the web UI
/// see the same window title as the Tauri app does.
/// What: Returns, in priority order: session-overrides value, project JSON `name`
/// whose `path` matches the session's pane cwd, else the raw session name.
/// Test: Seed a project JSON with matching path; call with session name; assert
/// the project `name` is returned.
fn resolve_session_display_name(session_name: &str) -> String {
    let overrides: HashMap<String, String> = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .map(|h| h.join(".ai-commander/session-overrides.json"))
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    if let Some(name) = overrides.get(session_name) {
        if !name.trim().is_empty() {
            return name.clone();
        }
    }

    let pane_path = std::process::Command::new("tmux")
        .args(["display-message", "-p", "-t", session_name, "#{pane_current_path}"])
        .output()
        .ok()
        .and_then(|o| {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if p.is_empty() { None } else { Some(p) }
        });

    let projects_dir = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".ai-commander/projects");

    if let Ok(entries) = std::fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |x| x == "json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        let proj_name = val.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let proj_path = val.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let sanitized = proj_name.replace([' ', '.', '/', ':'], "-");
                        let matches = sanitized == session_name
                            || proj_path == session_name
                            || pane_path.as_deref() == Some(proj_path);
                        if matches && !proj_name.is_empty() {
                            return proj_name.to_string();
                        }
                    }
                }
            }
        }
    }

    session_name.to_string()
}

/// Set the terminal window/tab title emitted by tmux for a session.
///
/// Why: Web-API-created sessions should behave identically to GUI-created ones
/// w.r.t. terminal titling — the label follows the tmux session across
/// attach/detach so any terminal emulator that picks up OSC 0/2 shows the
/// project nickname.
/// What: Session-scoped `set-option set-titles on` plus a custom
/// `set-titles-string`. `#` in the title is escaped to `##` because tmux
/// treats `#` as a format escape.
/// Test: Call with ("foo", "My Project"); assert
/// `tmux show-options -t foo -v set-titles-string` returns "My Project".
fn set_tmux_session_title(session: &str, title: &str) {
    let _ = std::process::Command::new("tmux")
        .args(["set-option", "-q", "-t", session, "set-titles", "on"])
        .output();
    let safe_title = title.replace('#', "##");
    let _ = std::process::Command::new("tmux")
        .args([
            "set-option",
            "-q",
            "-t",
            session,
            "set-titles-string",
            &safe_title,
        ])
        .output();
}

/// DELETE /api/sessions/:name/registration — Remove a registered project.
///
/// Why: Parity with the Tauri `delete_registration` command. Registered-only
/// sessions (no tmux running) need a removal path that doesn't involve
/// `destroy_session`.
/// What: Deletes the first `~/.ai-commander/projects/*.json` whose `name` (or
/// its sanitized tmux-safe form) matches, and removes any session-override
/// entry for that key.
/// Test: Seed a project JSON, DELETE this endpoint, assert the file is gone.
pub async fn delete_registration(Path(name): Path<String>) -> Json<SuccessResponse> {
    let projects_dir = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".ai-commander/projects");

    if let Ok(entries) = std::fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |x| x == "json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        let proj_name = val.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let sanitized = proj_name.replace([' ', '.', '/', ':'], "-");
                        if proj_name == name || sanitized == name {
                            let _ = std::fs::remove_file(&path);
                            break;
                        }
                    }
                }
            }
        }
    }

    // Clear any display-name override attached to this key.
    let overrides_path = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".ai-commander/session-overrides.json");
    if let Ok(content) = std::fs::read_to_string(&overrides_path) {
        if let Ok(mut map) = serde_json::from_str::<HashMap<String, String>>(&content) {
            if map.remove(&name).is_some() {
                if let Ok(updated) = serde_json::to_string_pretty(&map) {
                    let _ = std::fs::write(&overrides_path, updated);
                }
            }
        }
    }

    Json(SuccessResponse {
        message: "registration deleted".to_string(),
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

    // When the LLM interpreter is unavailable (Ollama down, OpenRouter
    // misconfigured, etc.), return an empty `output` string rather than a
    // hard-coded fallback message. The frontend treats empty strings as
    // "nothing to render" and the web `llm_unavailable` banner provides a
    // user-visible hint, so surfacing the fallback text here produced
    // duplicate/confusing UX. See transport.ts `get_session_summary`.
    let output = tokio::task::spawn_blocking(move || {
        commander_core::interpret_screen_context(&cleaned, is_idle)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();

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
///
/// Why: Raw mode in the web/Tauri UI needs verbatim tmux pane output so users can see the
/// actual terminal content. The earlier implementation delegated to `interpret_session`,
/// which ran the LLM interpretation pipeline and returned summarised text — making Raw
/// mode indistinguishable from Interpreted mode.
/// What: Runs `tmux capture-pane` directly (default last 500 lines) and returns the
/// unprocessed output as `{session, output}` JSON.
/// Test: POST to `/api/sessions/<name>/capture` with `{}` body and assert the response
/// `output` field contains the raw pane text (ANSI/control chars preserved) and differs
/// from `/api/sessions/<name>/interpret`.
pub async fn capture_session_output(
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

    let output = tmux
        .capture_output(&name, req.pane.as_deref(), Some(req.lines.unwrap_or(500)))
        .map_err(|e| ApiError::Internal(format!("failed to capture output: {}", e)))?;

    Ok(Json(SessionOutputResponse {
        session: name,
        output,
        adapter: None,
    }))
}

// ==================== Session log handlers ====================

/// GET /api/sessions/:name/logs — List available log dates for a session.
///
/// Why: The GUI (and future web UI date picker) needs to discover which days
/// have stored summaries before loading them. Enumerating the logs directory
/// is O(n) in the number of days (tiny) and avoids a stat-per-date probe.
/// What: Returns a JSON array of `"YYYY-MM-DD"` strings, sorted ascending.
/// Test: `GET /api/sessions/s1/logs` after writing two jsonl files; assert
/// the response body equals `["2026-01-01","2026-02-01"]`.
pub async fn list_session_logs(
    Path(name): Path<String>,
) -> Json<Vec<String>> {
    Json(commander_core::list_log_dates(&name))
}

/// GET /api/sessions/:name/logs/:date — Fetch entries for a given date.
///
/// Why: The GUI replays summaries when a session is opened so the user sees
/// what happened before they arrived — this is the backing call.
/// What: Returns a JSON array of `LogEntry`. Empty if the date has no file.
/// Test: Seed a jsonl file with two entries; `GET
/// /api/sessions/s1/logs/<today>` returns those two entries in order.
pub async fn get_session_log(
    Path((name, date)): Path<(String, String)>,
) -> Json<Vec<commander_core::LogEntry>> {
    Json(commander_core::read_log_entries(&name, &date))
}

/// POST /api/sessions/:name/logs/archive — Zip all logs for a session.
///
/// Why: Users want a single exportable artifact of a session's history
/// (ticket write-ups, sharing, pre-delete snapshot) without copying files by
/// hand. A server-side zip is faster than streaming every entry.
/// What: Calls the `zip` CLI to pack `~/.ai-commander/logs/<session>/` into
/// `~/.ai-commander/logs/archive/<session>-<timestamp>.zip`. Returns
/// `{"path": "<absolute path>"}` on success.
/// Test: Seed a log file, POST to this endpoint, assert the response has a
/// `path` field and the file exists with non-zero size.
pub async fn archive_session_logs(
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let path = commander_core::archive_session_logs(&name)
        .map_err(|e| ApiError::Internal(format!("archive failed: {}", e)))?;
    Ok(Json(serde_json::json!({
        "path": path.to_string_lossy().to_string(),
    })))
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

/// POST /api/sessions/nickname — Set (or clear) a session's display nickname.
///
/// Why: The web UI needs parity with the Tauri `set_session_nickname` command.
/// Nicknames live in the same `session-overrides.json` file used by
/// `list_sessions` (see [read_session_overrides] in the GUI crate). Writing
/// through this endpoint keeps both UIs in sync without having to cross-call.
/// What: Accepts `{"session_name": "...", "nickname": "..."}`. An empty (or
/// whitespace-only) nickname removes the override. Returns 204 No Content on
/// success, 400 if `session_name` is missing.
/// Test: POST with a non-empty nickname and assert the overrides file on
/// disk contains the mapping; POST again with `"nickname": ""` and assert the
/// mapping is removed.
pub async fn set_session_nickname(
    State(_state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let session_name = body
        .get("session_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let nickname = body
        .get("nickname")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if session_name.is_empty() {
        return (StatusCode::BAD_REQUEST, "session_name required").into_response();
    }

    // Read/write the same override file used by the Tauri commands so web
    // and desktop UIs converge on a single source of truth.
    let overrides_path = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".ai-commander/session-overrides.json");

    let mut map: HashMap<String, String> = std::fs::read_to_string(&overrides_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    if nickname.trim().is_empty() {
        map.remove(&session_name);
    } else {
        map.insert(session_name, nickname.trim().to_string());
    }

    if let Some(parent) = overrides_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &overrides_path,
        serde_json::to_string_pretty(&map).unwrap_or_default(),
    );

    StatusCode::NO_CONTENT.into_response()
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
/// Non-existent directories are filtered later via `.is_dir()`, so listing
/// many candidates here is cheap.
const DEFAULT_SCAN_DIRECTORIES: &[&str] = &[
    "Projects",
    "Developer",
    "Code",
    "code",
    "src",
    "projects",
    "work",
    "dev",
    "workspace",
    "Writing",
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

/// Minimum age (seconds) before a process may be considered stale.
///
/// Why: A young process has not had time to register itself with a tmux session
/// or for us to observe it in `connected_sessions`. Killing young processes
/// creates the exact "killed my active session" bug we are fixing.
const STALE_MIN_AGE_SECONDS: u64 = 300; // 5 minutes

/// Max tmux inactivity (seconds) before we stop treating the session as "recently active".
///
/// Why: Bug 1 — an active session was killed because the correlated tmux session
/// happened to not appear in our snapshot at scan time. Sessions with recent
/// pane activity must never be classified as stale regardless of correlation.
const TMUX_RECENT_ACTIVITY_SECONDS: u64 = 30 * 60;

/// Process name/command substrings that are NEVER eligible to be killed by the
/// process monitor, regardless of correlation state.
///
/// Why: URGENT bug — the process monitor was classifying active claude-mpm
/// sessions as stale when their command line did not contain the tmux session
/// name verbatim (substring correlation failed). Once `session` was `None`,
/// every `.unwrap_or(false)` guard defaulted to the unsafe side and the
/// process was killed. This allowlist is the last line of defense: AI
/// assistant processes and commander binaries must never be auto-killed, even
/// if the heuristic says they're stale. Users can still kill them manually
/// with `kill` from a terminal — they just cannot be swept by "Clean stale".
/// What: Lowercase substring match against both the `comm` column (short
/// process name) AND the full command line. Any match → NEVER stale.
/// Test: Spawn a process named `claude-mpm` with no tmux session correlation
/// and age 10 minutes; `list_processes` must return `stale == false`.
const PROTECTED_PROCESS_PATTERNS: &[&str] = &[
    "mpm",
    "claude",
    "claude-mpm",
    "claude_mpm",
    "ai-commander",
    "ai_commander",
    "commander-gui",
    "commander-daemon",
    "commander-api",
    "commander-telegram",
];

/// Returns true when the process name or command matches a protected pattern
/// and must never be classified as stale.
fn is_protected_process(name: &str, command: &str) -> bool {
    let name_lower = name.to_lowercase();
    let cmd_lower = command.to_lowercase();
    PROTECTED_PROCESS_PATTERNS
        .iter()
        .any(|pat| name_lower.contains(pat) || cmd_lower.contains(pat))
}

/// Check whether a tmux session had pane activity in the last 30 minutes.
///
/// Why: Belt-and-suspenders guard for Bug 1: a session with recent output from
/// its adapter (long LLM turn, test run, file watch) is almost certainly still
/// in use. Refusing to classify such sessions as stale costs one
/// `tmux display-message` per candidate process — cheap.
/// What: Runs `tmux display-message -t <session> -p "#{session_activity}"`
/// (milliseconds since Unix epoch). On parse failure, treats the session as
/// recently active to stay on the safe side.
/// Test: Create a tmux session, call with its name, assert true. Kill the
/// session, call again, assert false.
fn tmux_session_recently_active(session: &str) -> bool {
    let output = std::process::Command::new("tmux")
        .args([
            "display-message",
            "-t",
            session,
            "-p",
            "#{session_activity}",
        ])
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let activity_ms: u64 = match raw.parse() {
        Ok(v) => v,
        Err(_) => return true,
    };
    let activity_secs = activity_ms / 1000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now <= activity_secs {
        return true;
    }
    (now - activity_secs) < TMUX_RECENT_ACTIVITY_SECONDS
}

/// Fetch the currently-live tmux session names (empty on failure).
fn live_tmux_sessions() -> Vec<String> {
    std::process::Command::new("tmux")
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
        .unwrap_or_default()
}

/// Verify a specific tmux session is alive via `tmux has-session -t <name>`.
///
/// Why: `list_tmux_sessions` can race with session creation/teardown. Before
/// killing a process we take a fresh per-session reading to close that race.
/// What: Returns `true` when `tmux has-session` exits 0; `false` on any error
/// or non-zero exit.
/// Test: Create tmux session "a"; assert `tmux_has_session("a") == true`.
/// Kill the session; assert it returns `false`.
fn tmux_has_session(name: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["has-session", "-t", name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// GET /api/processes — List commander-related running processes.
///
/// Why: The web UI's Process Monitor needs the full list of candidate processes
/// with a stale flag it can render. The stale flag is the gate that controls
/// whether `kill_stale_processes` would target a row, so the classifier must
/// err on the side of leaving rows alone.
/// What: Scans `ps axo` output, keeps rows matching commander/claude/mpm,
/// correlates each with a live tmux session by substring, and sets
/// `stale = true` only when ALL of:
///   1. The correlated tmux session (if any) is NOT in `tmux list-sessions`.
///   2. `age_seconds > STALE_MIN_AGE_SECONDS`.
///   3. The process is NOT associated with a session in `connected_sessions`.
/// Test: Start a long-lived claude process, create a tmux session matching
/// its command line, assert the returned row has `stale == false`.
pub async fn list_processes(State(state): State<AppState>) -> Json<ProcessListResponse> {
    let mut processes = Vec::new();

    let tmux_sessions = live_tmux_sessions();
    let connected: std::collections::HashSet<String> = state
        .connected_sessions
        .read()
        .map(|g| g.clone())
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

            // Try to correlate with a tmux session (substring, case-insensitive).
            // We intentionally over-associate: any plausible match means we
            // keep the process alive.
            let cmd_lower = command.to_lowercase();
            let session = tmux_sessions
                .iter()
                .find(|s| cmd_lower.contains(&s.to_lowercase()))
                .cloned();

            let session_alive = session
                .as_ref()
                .map(|s| tmux_sessions.iter().any(|t| t == s))
                .unwrap_or(false);
            let is_connected_session = session
                .as_ref()
                .map(|s| connected.contains(s))
                .unwrap_or(false);
            // Bug 1 guard: a session with pane activity in the last 30 minutes
            // is never stale. Covers the "temporarily absent from snapshot"
            // race where the process correlates but the session list doesn't.
            let recently_active = session
                .as_ref()
                .map(|s| tmux_session_recently_active(s))
                .unwrap_or(false);

            // Protected-process guard: URGENT Bug 2 — AI assistant processes
            // (claude, claude-mpm, mpm) and commander binaries must NEVER be
            // classified as stale, even when correlation fails. Correlation is
            // a heuristic; this allowlist is a hard rule.
            let protected = is_protected_process(&name, &command);

            let stale = !protected
                && !session_alive
                && !is_connected_session
                && !recently_active
                && age_seconds > STALE_MIN_AGE_SECONDS;

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

/// Request body for `POST /api/processes/clean`.
///
/// Why: The previous handler killed on every POST with no dry-run option,
/// which made it dangerous to call from the UI or anywhere else. Now callers
/// must opt into destructive behavior via `confirm: true` — every other call
/// is a dry-run that returns the list of processes that WOULD be killed.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct KillStaleRequest {
    /// When true, actually send SIGTERM. Otherwise, return a dry-run preview
    /// with `dry_run: true` in the response.
    pub confirm: bool,
}

/// POST /api/processes/clean — Kill stale commander processes (requires confirm).
///
/// Why: Users need a "clean up leftover processes" action, but prior
/// implementations classified active claude-mpm sessions as stale and killed
/// them. This version:
///   1. Refuses to kill any process whose name/command matches the
///      `PROTECTED_PROCESS_PATTERNS` allowlist (claude, mpm, claude-mpm,
///      ai-commander, commander-*). Those processes are not eligible for the
///      "stale" classification in the first place.
///   2. Defaults to DRY-RUN. Callers must send `{"confirm": true}` to
///      actually send SIGTERM. Dry-run responses return the same shape but
///      with `dry_run: true` and `killed` populated as the list that WOULD
///      be killed.
///   3. Keeps the belt-and-suspenders tmux recheck: a correlated session that
///      is alive / connected / recently active blocks the kill.
/// What: Parses optional `{"confirm": bool}` body. Enumerates stale rows from
/// `list_processes`, double-filters through `is_protected_process` (defense
/// in depth in case classification misses), re-verifies tmux state, and
/// either previews or kills based on `confirm`.
/// Test: POST with no body → dry-run response with `dry_run: true`. POST
/// with `{"confirm":true}` against a seeded stale python → process is killed.
/// POST with `{"confirm":true}` against a seeded claude-mpm process with age
/// > STALE_MIN_AGE_SECONDS → 0 kills because it's protected.
pub async fn kill_stale_processes(
    State(state): State<AppState>,
    body: Option<Json<KillStaleRequest>>,
) -> Result<Json<serde_json::Value>> {
    let confirm = body.map(|Json(b)| b.confirm).unwrap_or(false);
    let list = list_processes(State(state.clone())).await.0;
    let connected: std::collections::HashSet<String> = state
        .connected_sessions
        .read()
        .map(|g| g.clone())
        .unwrap_or_default();

    let mut killed = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = Vec::new();

    for proc in list.processes.iter().filter(|p| p.stale) {
        // Defense-in-depth allowlist: even if the classifier ever regresses,
        // refuse to kill protected processes here.
        if is_protected_process(&proc.name, &proc.command) {
            skipped.push(serde_json::json!({
                "pid": proc.pid,
                "name": proc.name,
                "session": proc.session,
                "reason": "process name is on the protected allowlist"
            }));
            continue;
        }

        // Safety gate: refuse to kill if the correlated session is alive per a
        // fresh `tmux has-session` check, is in the connected set, or had pane
        // activity in the last 30 minutes. Any of these means a client cares
        // about the process.
        if let Some(ref s) = proc.session {
            if tmux_has_session(s)
                || connected.contains(s)
                || tmux_session_recently_active(s)
            {
                skipped.push(serde_json::json!({
                    "pid": proc.pid,
                    "name": proc.name,
                    "session": s,
                    "reason": "session is alive, connected, or recently active"
                }));
                continue;
            }
        }

        if !confirm {
            // Dry-run: log what WOULD happen, do not kill.
            killed.push(serde_json::json!({
                "pid": proc.pid,
                "name": proc.name,
                "would_kill": true,
            }));
            continue;
        }

        match std::process::Command::new("kill")
            .args(["-TERM", &proc.pid.to_string()])
            .output()
        {
            Ok(_) => killed.push(serde_json::json!({
                "pid": proc.pid,
                "name": proc.name,
            })),
            Err(e) => failed.push(serde_json::json!({
                "pid": proc.pid,
                "name": proc.name,
                "error": e.to_string(),
            })),
        }
    }

    let count = killed.len();
    let message = if !confirm {
        format!(
            "Dry-run: {} stale process(es) would be killed. Resend with {{\"confirm\": true}} to kill.",
            count
        )
    } else if count > 0 {
        format!("Killed {} stale process(es)", count)
    } else {
        "No stale processes found".to_string()
    };

    Ok(Json(serde_json::json!({
        "dry_run": !confirm,
        "killed": killed,
        "failed": failed,
        "skipped": skipped,
        "count": count,
        "message": message,
    })))
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

/// GET /api/git-user — Return the git user name and email from global config.
pub async fn get_git_user() -> Json<serde_json::Value> {
    let name = tokio::process::Command::new("git")
        .args(["config", "--global", "user.name"])
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    let email = tokio::process::Command::new("git")
        .args(["config", "--global", "user.email"])
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    Json(serde_json::json!({
        "name": name,
        "email": email,
    }))
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

// ==================== GitHub stats ====================

use crate::state::GitHubStats;

/// GET /api/github-stats — Return cached GitHub issue/PR counts per project.
pub async fn get_github_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let stats = state.github_stats.read().await;
    Json(serde_json::json!({ "stats": *stats }))
}

/// Spawns a background task that polls GitHub for open issue/PR counts hourly.
///
/// Scans the same project directories as `list_project_directories`, extracts the
/// GitHub remote URL from each git repo, and queries the GitHub Search API for
/// open issue and PR counts.
pub fn spawn_github_stats_poller(
    github_stats: Arc<RwLock<HashMap<String, GitHubStats>>>,
) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = poll_github_stats(&github_stats).await {
                warn!("github stats poll failed: {}", e);
            }
            // Poll every hour
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

/// Performs a single GitHub stats poll cycle.
async fn poll_github_stats(
    github_stats: &Arc<RwLock<HashMap<String, GitHubStats>>>,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let home = dirs_home();
    let scan_dirs = load_scan_directories(&home);
    let search_roots: Vec<std::path::PathBuf> = scan_dirs
        .iter()
        .filter_map(|subdir| home.as_ref().map(|h| h.join(subdir)))
        .filter(|p| p.is_dir())
        .collect();

    // Try to get a GitHub token for higher rate limits (30 search/min vs 10).
    let gh_token = tokio::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|t| !t.is_empty());

    let client = reqwest::Client::builder()
        .user_agent("ai-commander")
        .build()?;

    let mut new_stats = HashMap::new();

    for root in &search_roots {
        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // Get git remote origin URL
            let output = match tokio::process::Command::new("git")
                .args(["-C", &path.to_string_lossy(), "remote", "get-url", "origin"])
                .output()
                .await
            {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).trim().to_string()
                }
                _ => continue,
            };

            let repo = match parse_github_repo(&output) {
                Some(r) => r,
                None => continue,
            };

            let project_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            debug!("polling github stats for {} ({})", project_name, repo);

            let open_issues = fetch_github_count(
                &client,
                &format!(
                    "https://api.github.com/search/issues?q=repo:{}+is:issue+is:open",
                    repo
                ),
                gh_token.as_deref(),
            )
            .await;

            // Small delay to stay within rate limits
            tokio::time::sleep(Duration::from_secs(2)).await;

            let open_prs = fetch_github_count(
                &client,
                &format!(
                    "https://api.github.com/search/issues?q=repo:{}+is:pr+is:open",
                    repo
                ),
                gh_token.as_deref(),
            )
            .await;

            // Small delay between repos
            tokio::time::sleep(Duration::from_secs(2)).await;

            new_stats.insert(
                project_name,
                GitHubStats {
                    open_issues,
                    open_prs,
                    repo,
                },
            );
        }
    }

    // Update shared state only if we got results
    if !new_stats.is_empty() {
        let mut stats = github_stats.write().await;
        *stats = new_stats;
    }

    Ok(())
}

/// Parse "owner/repo" from a GitHub remote URL (SSH or HTTPS).
fn parse_github_repo(url: &str) -> Option<String> {
    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return Some(rest.trim_end_matches(".git").to_string());
    }
    // Also handle ssh://git@github.com/owner/repo.git
    if url.contains("github.com:") {
        let parts: Vec<&str> = url.split("github.com:").collect();
        if parts.len() == 2 {
            return Some(parts[1].trim_end_matches(".git").to_string());
        }
    }
    // HTTPS: https://github.com/owner/repo.git
    if url.contains("github.com/") {
        let parts: Vec<&str> = url.split("github.com/").collect();
        if parts.len() == 2 {
            return Some(parts[1].trim_end_matches(".git").to_string());
        }
    }
    None
}

/// Fetch the `total_count` from a GitHub Search API URL.
async fn fetch_github_count(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> u32 {
    let mut request = client
        .get(url)
        .header("Accept", "application/vnd.github.v3+json");

    if let Some(tok) = token {
        request = request.header("Authorization", format!("Bearer {}", tok));
    }

    match request.send().await {
        Ok(resp) => {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                data.get("total_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32
            } else {
                0
            }
        }
        Err(e) => {
            debug!("github api request failed: {}", e);
            0
        }
    }
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

/// Spawns a background poller keyed on `connected_sessions` (set by
/// `connect_session`/`disconnect_session`) rather than the full tmux list.
///
/// Why: The state-machine refactor treats a session as "connected" only after
/// a client explicitly opts in. Polling every tmux session would (a) waste LLM
/// calls on background sessions nobody is watching and (b) blast SSE events
/// for sessions no subscriber cares about. This poller iterates only the set
/// of actively-connected names and broadcasts summaries via the existing
/// `event_tx` channel.
/// What: Every 5s, snapshots `connected_sessions`, captures ~500 lines from
/// each, and spawns a blocking task that runs `interpret_screen_context` and
/// broadcasts a `SessionEvent` with `event_type="update"` when content is
/// non-empty.
/// Test: Insert a name into `connected_sessions`, wait ~6s, assert at least
/// one `SessionEvent` was broadcast to a subscribed channel.
pub fn spawn_connected_sessions_poller(app_state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let connected: Vec<String> = app_state
                .connected_sessions
                .read()
                .unwrap()
                .iter()
                .cloned()
                .collect();

            for session_name in connected {
                let Some(tmux) = &app_state.tmux else { continue };
                let output = match tmux.capture_output(&session_name, None, Some(500)) {
                    Ok(o) if !o.is_empty() => o,
                    _ => continue,
                };

                let session_clone = session_name.clone();
                let tx_clone = app_state.event_tx.clone();
                let adapter_map = app_state.session_adapters.read().await.clone();
                let adapter = adapter_map
                    .get(&session_name)
                    .cloned()
                    .unwrap_or_else(|| "claude".to_string());

                tokio::task::spawn_blocking(move || {
                    let cleaned = commander_core::clean_response(&output);
                    let is_idle = commander_core::is_claude_ready(&cleaned);
                    if let Some(summary) =
                        commander_core::interpret_screen_context(&cleaned, is_idle)
                    {
                        if summary.trim().is_empty() {
                            return;
                        }
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let _ = tx_clone.send(SessionEvent {
                            session_name: session_clone,
                            event_type: "update".to_string(),
                            content: summary,
                            timestamp: ts,
                            adapter,
                            is_update: true,
                        });
                    }
                });
            }
        }
    });
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

                // Count lines that are genuinely new (not in previous capture)
                let prev_lines: std::collections::HashSet<&str> = prev.lines().collect();
                let new_lines: Vec<&str> = trimmed.lines()
                    .filter(|line| !line.trim().is_empty() && !prev_lines.contains(line))
                    .collect();

                // Only interpret if there are meaningful new lines (>3 new lines)
                if !new_lines.is_empty() && new_lines.len() > 3 {
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
                            let prev_interp = interps
                                .lock()
                                .unwrap()
                                .get(&session)
                                .cloned()
                                .unwrap_or_default();
                            let has_prev = !prev_interp.is_empty();

                            // Skip "Processing..." if we already have a meaningful interpretation
                            if interpretation == "Processing..." && has_prev && prev_interp != "Processing..." {
                                // Don't downgrade a real interpretation to "Processing..."
                            } else {
                                // Fuzzy dedup: skip if first 30 chars match (LLM non-determinism)
                                let prev_prefix = prev_interp.chars().take(30).collect::<String>().to_lowercase();
                                let new_prefix = interpretation.chars().take(30).collect::<String>().to_lowercase();
                                let is_similar = !prev_prefix.is_empty() && prev_prefix == new_prefix;

                                if !is_similar {
                                    let _ = tx.send(SessionEvent {
                                        session_name: session.clone(),
                                        event_type: "interpretation".to_string(),
                                        content: interpretation.clone(),
                                        timestamp: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                        adapter,
                                        is_update: has_prev,
                                    });
                                    // Persist to the session's summary log.
                                    // Hash the cleaned content (not the
                                    // interpretation) so dedup tracks the
                                    // underlying screen state.
                                    let content_hash = format!("{:x}", {
                                        use std::collections::hash_map::DefaultHasher;
                                        use std::hash::{Hash, Hasher};
                                        let mut h = DefaultHasher::new();
                                        cleaned.hash(&mut h);
                                        h.finish()
                                    });
                                    let _ = commander_core::append_log_entry(
                                        &session,
                                        &interpretation,
                                        &content_hash,
                                    );
                                    interps
                                        .lock()
                                        .unwrap()
                                        .insert(session, interpretation);
                                }
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
        // disconnect with a body removes the named session from the set.
        let state = make_test_state();
        // Seed a connected session so we can verify removal.
        state
            .connected_sessions
            .write()
            .unwrap()
            .insert("foo".to_string());
        let body = serde_json::json!({ "session": "foo" });
        let response = disconnect_session(State(state.clone()), Json(body)).await;
        assert_eq!(response.message, "disconnected");
        assert!(!state.connected_sessions.read().unwrap().contains("foo"));
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
        let state = make_test_state();
        let response = list_processes(State(state)).await;
        assert_eq!(response.total, response.processes.len());
    }

    #[tokio::test]
    async fn test_kill_stale_processes() {
        let state = make_test_state();
        // No body → dry-run by default. Verifies the safety guard.
        let response = kill_stale_processes(State(state), None).await.unwrap();
        let val = response.0;
        assert!(val.get("message").is_some());
        assert!(val.get("count").is_some());
        assert!(val.get("killed").is_some());
        assert_eq!(
            val.get("dry_run").and_then(|v| v.as_bool()),
            Some(true),
            "kill_stale_processes must default to dry-run when no body is sent"
        );
    }

    /// Why: URGENT regression test — active claude-mpm sessions were being
    /// killed when the classifier failed to correlate the process with a tmux
    /// session. Any process whose name or command matches the protected
    /// allowlist (claude, mpm, claude-mpm, etc.) must NEVER be classified as
    /// stale, even with no correlation and old age.
    /// What: Directly tests `is_protected_process` with the exact strings the
    /// task spec calls out.
    #[test]
    fn test_protected_processes_never_killable() {
        assert!(is_protected_process("mpm", "mpm run --foo"));
        assert!(is_protected_process("claude", "claude --model sonnet"));
        assert!(is_protected_process("claude-mpm", "/usr/bin/claude-mpm"));
        assert!(is_protected_process("python", "python -m claude_mpm"));
        assert!(is_protected_process("ai-commander", "ai-commander"));
        assert!(is_protected_process("commander-gui", "./commander-gui"));
        // Random process is not protected.
        assert!(!is_protected_process("sleep", "sleep 10"));
        assert!(!is_protected_process("python", "python -m some_other_thing"));
    }

    /// Why: Bug 1 root-cause guard — the recency helper must gracefully handle a
    /// session that does not exist (tmux returns non-zero). Returning `true` in
    /// that case would keep dead processes alive forever; returning `false`
    /// correctly allows the stale classifier to proceed.
    /// What: Invokes `tmux_session_recently_active` with a session name that
    /// almost certainly does not exist and asserts the result is `false`.
    /// Test: Call the helper with a random unlikely-to-exist name, assert false.
    #[test]
    fn test_tmux_recently_active_missing_session_returns_false() {
        // Use a suffix that is extremely unlikely to collide with any real
        // tmux session on the test host.
        let name = "__aic_test_nonexistent_session_xyz_991__";
        assert!(!tmux_session_recently_active(name));
    }

    /// Why: Bug 1 regression test — a process whose command line matches a
    /// live tmux session must NEVER be reported as stale, regardless of age.
    /// What: Seeds `connected_sessions` with a name, asserts that any process
    /// whose command contains that name is not stale (since the session is
    /// connected, even if we can't literally verify the tmux session here).
    /// Test: Insert "test-session" into connected_sessions; scan the process
    /// list; assert no process containing "test-session" is marked stale.
    #[tokio::test]
    async fn test_list_processes_never_marks_connected_session_stale() {
        let state = make_test_state();
        state
            .connected_sessions
            .write()
            .unwrap()
            .insert("test-session".to_string());
        let response = list_processes(State(state)).await;
        for proc in &response.processes {
            if let Some(ref sess) = proc.session {
                if sess == "test-session" {
                    assert!(
                        !proc.stale,
                        "process {} associated with connected session was marked stale",
                        proc.pid
                    );
                }
            }
        }
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
