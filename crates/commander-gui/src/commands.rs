use crate::state::GuiState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{Emitter, State};

/// Path to the session display-name override file.
///
/// Why: Centralise so the read/write helpers and any future callers never drift
/// on location.
/// What: Returns `~/.ai-commander/session-overrides.json` (non-existence is
/// fine — the helpers handle it).
/// Test: Set HOME to a tempdir, call this, assert the returned path ends with
/// `.ai-commander/session-overrides.json`.
fn session_overrides_path() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".ai-commander/session-overrides.json")
}

/// Read per-session display-name overrides from disk.
///
/// Why: After a user renames a session, we must prefer their chosen name over
/// the project-nickname lookup in `list_sessions`, otherwise the path-based
/// match re-applies the old project nickname and the rename appears to "stick
/// but then revert".
/// What: Returns `{ tmux_session_name -> display_name }` loaded from
/// `~/.ai-commander/session-overrides.json`. Any IO/parse error yields an
/// empty map — overrides are strictly additive.
/// Test: Write a JSON object with one entry to the path, call this, assert the
/// map contains that single pair; remove the file, assert the map is empty.
fn read_session_overrides() -> HashMap<String, String> {
    std::fs::read_to_string(session_overrides_path())
        .ok()
        .and_then(|s| serde_json::from_str::<HashMap<String, String>>(&s).ok())
        .unwrap_or_default()
}

/// Insert (or update) a single session display-name override.
///
/// Why: `rename_session` must record the user's chosen name as the winning
/// display name so subsequent `list_sessions` polls don't overwrite it with
/// the project-nickname lookup.
/// What: Reads the existing overrides file, inserts `tmux_name -> display_name`,
/// and writes it back (pretty-printed for humans). Silently tolerates read/write
/// errors — an override is a best-effort convenience, not a hard requirement.
/// Test: With HOME set to a tempdir, call with `("foo", "Foo Display")`; assert
/// the written file parses back to a map containing that pair.
fn write_session_override(tmux_name: &str, display_name: &str) {
    let path = session_overrides_path();
    let mut map = read_session_overrides();
    map.insert(tmux_name.to_string(), display_name.to_string());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(&map) {
        let _ = std::fs::write(&path, contents);
    }
}

/// Remove a session from the overrides file (if present).
///
/// Why: After a rename the old tmux session name no longer exists, so its
/// stale override entry would leak forever. Clean it up to keep the file tidy.
/// What: Reads, removes the key, writes back. No-op if the key was absent.
/// Test: Seed a two-entry map, call with one key, assert the remaining entry
/// is still present and the removed one is gone.
fn remove_session_override(tmux_name: &str) {
    let path = session_overrides_path();
    let mut map = read_session_overrides();
    if map.remove(tmux_name).is_some() {
        if let Ok(contents) = serde_json::to_string_pretty(&map) {
            let _ = std::fs::write(&path, contents);
        }
    }
}

/// Interpreted summary of a session's current screen state.
#[derive(Serialize)]
pub struct SessionSummary {
    pub adapter: String,
    pub is_idle: bool,
    pub preview: String,
}

/// Interpret what Claude is doing in a session using LLM analysis.
///
/// Captures the last 100 lines of tmux output, cleans them, then asks
/// OpenRouter/Ollama to produce a human-readable one-sentence interpretation.
/// Falls back to `clean_screen_preview` if the LLM is unavailable or returns
/// an empty/unchanged result.
#[tauri::command]
pub async fn interpret_session(
    name: String,
    state: State<'_, GuiState>,
) -> Result<String, String> {
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;
    let output = tmux
        .capture_output(&name, None, Some(100))
        .map_err(|e| e.to_string())?;

    let cleaned = commander_core::clean_response(&output);
    let is_idle = commander_core::is_claude_ready(&cleaned);

    // interpret_screen_context uses reqwest::blocking internally — run it on a
    // dedicated blocking thread so we do not stall the async executor.
    let cleaned_clone = cleaned.clone();
    let interpretation = tokio::task::spawn_blocking(move || {
        commander_core::interpret_screen_context(&cleaned_clone, is_idle)
    })
    .await
    .map_err(|e| e.to_string())?;

    match interpretation {
        Some(text) if !text.is_empty() => Ok(text),
        _ => Ok(commander_core::clean_screen_preview(&cleaned, 10)),
    }
}

/// Return structured metadata about a session's current screen state.
///
/// Captures the last 200 lines of tmux output, cleans them, and returns:
/// - `adapter`  — detected adapter type ("Claude", "Shell", "Unknown")
/// - `is_idle`  — whether Claude is waiting for user input
/// - `preview`  — last 10 meaningful lines of cleaned output
#[tauri::command]
pub async fn get_session_summary(
    name: String,
    state: State<'_, GuiState>,
) -> Result<SessionSummary, String> {
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;
    let output = tmux
        .capture_output(&name, None, Some(200))
        .map_err(|e| e.to_string())?;

    let cleaned = commander_core::clean_response(&output);
    let adapter = commander_core::detect_adapter(&cleaned);
    let is_idle = commander_core::is_claude_ready(&cleaned);
    let preview = commander_core::clean_screen_preview(&cleaned, 10);

    Ok(SessionSummary {
        adapter: format!("{:?}", adapter),
        is_idle,
        preview,
    })
}

#[derive(Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub created_at: String,
    /// True when this session is in the `connected_sessions` set (actively
    /// monitored). Kept for backward compatibility with frontend code that
    /// still checks this flag directly.
    pub is_connected: bool,
    pub path: Option<String>,
    pub nickname: Option<String>,
    /// Tri-state session lifecycle: "connected", "disconnected", or "registered".
    ///
    /// - `connected`   — tmux session exists AND is in `connected_sessions` (actively monitored).
    /// - `disconnected` — tmux session exists but not currently monitored.
    /// - `registered`   — only a project JSON exists; no tmux session running.
    pub session_state: String,
}

#[derive(Serialize, Deserialize)]
pub struct BotInfo {
    pub running: bool,
    pub pid: Option<u32>,
}

#[tauri::command]
pub async fn list_sessions(state: State<'_, GuiState>) -> Result<Vec<SessionInfo>, String> {
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;
    let sessions = tmux.list_sessions().map_err(|e| e.to_string())?;

    // Load projects to resolve session nicknames. Uses a minimal ProjectStub that
    // only deserializes `name` and `path`, avoiding silent failures from the full
    // Project struct's complex nested fields. Failure is non-fatal — all nicknames
    // stay None and session listing continues normally.
    #[derive(serde::Deserialize, Clone)]
    struct ProjectStub {
        name: String,
        path: String,
        #[serde(default)]
        created_at: Option<String>,
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

    // Per-session display-name overrides (user renames). These win over the
    // project-nickname lookup below, otherwise a rename is silently reverted
    // to the matching project's nickname on the next poll.
    let overrides = read_session_overrides();

    // Snapshot of which sessions are actively monitored. Kept outside the
    // per-iteration closure so we only take the lock once.
    let connected_snapshot: std::collections::HashSet<String> =
        state.connected_sessions.read().unwrap().clone();

    // Build the session list, then deduplicate by tmux session name to guard
    // against any upstream source emitting the same session more than once
    // (e.g. symlinked project paths matching the same session twice).
    let mut seen = std::collections::HashSet::new();
    // Track which projects map to a running tmux session so we can emit
    // "registered-only" placeholders for the rest.
    let mut matched_project_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    let mut results: Vec<SessionInfo> = sessions
        .into_iter()
        .filter_map(|s| {
            if !seen.insert(s.name.clone()) {
                return None; // duplicate tmux name — skip
            }
            let path = std::process::Command::new("tmux")
                .args(["display-message", "-p", "-t", &s.name, "#{pane_current_path}"])
                .output()
                .ok()
                .and_then(|o| {
                    let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if p.is_empty() { None } else { Some(p) }
                });
            let matched_project = projects.iter().find(|p| {
                p.name.replace([' ', '.', '/', ':'], "-") == s.name
                    || p.path == s.name
                    || path.as_deref() == Some(p.path.as_str())
            });
            if let Some(p) = matched_project {
                matched_project_names.insert(p.name.clone());
            }
            let nickname = matched_project.map(|p| p.name.clone());
            // Overrides take highest priority — above project nickname.
            let display_name = overrides.get(&s.name).cloned().or(nickname);
            let is_connected = connected_snapshot.contains(&s.name);
            let session_state = if is_connected { "connected" } else { "disconnected" };
            Some(SessionInfo {
                name: s.name.clone(),
                created_at: s.created_at.to_string(),
                is_connected,
                path,
                nickname: display_name,
                session_state: session_state.to_string(),
            })
        })
        .collect();

    // Append registered-only projects (no matching tmux session).
    for proj in &projects {
        if matched_project_names.contains(&proj.name) {
            continue;
        }
        // Dedup name collisions — if a tmux session already occupies this slot
        // (by sanitized name), skip.
        let sanitized = proj.name.replace([' ', '.', '/', ':'], "-");
        if seen.contains(&sanitized) {
            continue;
        }
        results.push(SessionInfo {
            name: proj.name.clone(),
            created_at: proj.created_at.clone().unwrap_or_default(),
            is_connected: false,
            path: Some(proj.path.clone()),
            nickname: Some(proj.name.clone()),
            session_state: "registered".to_string(),
        });
    }

    Ok(results)
}

/// Connect to a tmux session: add to `connected_sessions` (actively monitored),
/// set as the current view session, and return the full log history so the
/// frontend can pre-populate ChatView.
///
/// Why: Previously connecting only toggled `current_session`, leaving the GUI
/// without historical context on reconnect. The new state machine distinguishes
/// "actively monitored" from "displayed in chat" — connection implies both.
/// What: Inserts `name` into `connected_sessions`, sets it as `current_session`,
/// reads every JSONL log entry for the session, and returns `{history: [...]}`.
/// Test: Seed two log entries under a fake HOME, invoke this command, assert
/// `connected_sessions.contains(&name)` and the returned `history` array has
/// two items with matching `text` values.
#[tauri::command]
pub async fn connect_session(
    name: String,
    state: State<'_, GuiState>,
) -> Result<serde_json::Value, String> {
    let tmux = state.tmux.as_ref().ok_or(
        "Cannot connect: tmux is not available. Make sure tmux is installed and accessible."
    )?;

    if !tmux.session_exists(&name) {
        return Err(format!(
            "Session '{}' does not exist. Available sessions can be seen in the list.",
            name
        ));
    }

    // Add to the actively-monitored set (polled by the summary loop).
    state
        .connected_sessions
        .write()
        .unwrap()
        .insert(name.clone());
    // And mark as the currently-displayed session.
    *state.current_session.write().unwrap() = Some(name.clone());

    // Refresh the tmux session title in case the nickname/project JSON changed
    // since the session was created. Cheap (two tmux option writes) and keeps
    // all attached terminal emulators in sync on reconnect.
    let display = resolve_session_display_name(&name);
    set_tmux_session_title(&name, &display);

    // Full log history for client-side hydration.
    let history: Vec<serde_json::Value> = commander_core::read_all_log_entries(&name)
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

    Ok(serde_json::json!({ "session": name, "history": history }))
}

/// Disconnect a session (stop monitoring it). If `name` is provided, that
/// specific session is removed from `connected_sessions`; if it was the current
/// view session the view is cleared too. If `name` is omitted, the current view
/// session is disconnected.
///
/// Why: The UI needs per-session disconnect so users can stop monitoring a
/// background session without touching the ChatView. The no-arg form preserves
/// compatibility with the old "disconnect current" call path.
/// What: Removes one or zero entries from `connected_sessions` and clears
/// `current_session` iff it matched the disconnected session.
/// Test: Seed two connected sessions; call with Some("a"), assert "a" removed
/// and "b" still present. Call with None while current is "b", assert "b"
/// removed and current_session is None.
#[tauri::command]
pub async fn disconnect_session(
    name: Option<String>,
    state: State<'_, GuiState>,
) -> Result<(), String> {
    match name {
        Some(n) => {
            state.connected_sessions.write().unwrap().remove(&n);
            let mut current = state.current_session.write().unwrap();
            if current.as_deref() == Some(&n) {
                *current = None;
            }
        }
        None => {
            let current_name = state.current_session.read().unwrap().clone();
            if let Some(n) = current_name {
                state.connected_sessions.write().unwrap().remove(&n);
            }
            *state.current_session.write().unwrap() = None;
        }
    }
    Ok(())
}

/// Delete a registered project from disk (no tmux session involved).
///
/// Why: Registered-only sessions (entries in `~/.ai-commander/projects/*.json`
/// with no running tmux session) need a clean way to be removed from the list.
/// Matches the project JSON by either the exact name or the sanitized form so
/// it works whether the frontend sends the pretty name or the tmux-safe slug.
/// What: Iterates `~/.ai-commander/projects/*.json`, deletes the first file
/// whose `name` field (or its sanitized form) matches; also drops any leftover
/// session-override entry for that name.
/// Test: Seed a project JSON, call this command with the matching name, assert
/// the JSON file is gone and the overrides file has no entry for it.
#[tauri::command]
pub async fn delete_registration(name: String) -> Result<(), String> {
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

    // Also drop any dangling display-name override for this key.
    remove_session_override(&name);
    Ok(())
}

/// Capture the current tmux output for a session immediately (synchronous snapshot).
/// Called by the frontend right after connecting so the initial terminal state is shown
/// without waiting for the polling loop.
#[tauri::command]
pub async fn capture_session_output(
    name: String,
    state: State<'_, GuiState>,
) -> Result<String, String> {
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;
    tmux.capture_output(&name, None, Some(500))
        .map_err(|e| e.to_string())
}

/// Unregister a session from the AI Commander project registry.
///
/// Why: Users want to stop tracking a tmux session in the AIC registry without
/// killing the underlying tmux session. `stop_session` destroys tmux; `delete_registration`
/// only targets a project JSON by name/sanitised-name and doesn't consult the session's
/// pane path. This command specifically locates the project JSON whose `path` matches the
/// tmux session's current working directory (the same match rule used in `list_sessions`)
/// so running sessions can be cleanly dissociated from their registration entry.
/// What: Finds the project JSON in `~/.ai-commander/projects/` that matches this session
/// (by sanitized name OR by pane path) and removes it. Leaves the tmux session alive.
/// Also clears any matching session-override entry so the display name reverts cleanly.
/// Test: Seed a project JSON with path matching a running tmux session, invoke this
/// command with the tmux session name, assert the JSON is gone and `tmux.session_exists`
/// still returns true.
#[tauri::command]
pub async fn unregister_session(
    session_name: String,
    _state: State<'_, GuiState>,
) -> Result<(), String> {
    // Look up the session's pane current path so we can match project JSONs by path
    // — matching the same rule used by `list_sessions` for nickname resolution.
    let pane_path = std::process::Command::new("tmux")
        .args(["display-message", "-p", "-t", &session_name, "#{pane_current_path}"])
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
                        let matches = proj_name == session_name
                            || sanitized == session_name
                            || proj_path == session_name
                            || pane_path.as_deref() == Some(proj_path);
                        if matches {
                            std::fs::remove_file(&path)
                                .map_err(|e| format!("failed to remove registration: {}", e))?;
                            removed = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    // Clean up any display-name override attached to this key, regardless of
    // whether a project JSON was found — orphaned overrides are harmless but ugly.
    remove_session_override(&session_name);

    if !removed {
        return Err(format!(
            "No registered project found matching session '{}'",
            session_name
        ));
    }

    Ok(())
}

#[tauri::command]
pub async fn stop_session(name: String, state: State<'_, GuiState>) -> Result<(), String> {
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

    if !tmux.session_exists(&name) {
        return Err(format!("Session '{}' not found", name));
    }

    // For claude-mpm sessions, send /mpm-session-pause before terminating
    // to save session state for potential resume later.
    if let Ok(output) = tmux.capture_output(&name, None, Some(50)) {
        let is_mpm = commander_core::is_mpm_ready(&output)
            || output.contains("claude-mpm")
            || output.contains("Claude MPM");
        if is_mpm {
            eprintln!("[GUI] Sending /mpm-session-pause to '{}' before stopping", name);
            let _ = tmux.send_line(&name, None, "/mpm-session-pause");

            // Wait for pause confirmation — poll tmux output for success indicators
            // Timeout after 30s to avoid hanging indefinitely
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                if tokio::time::Instant::now() > deadline {
                    eprintln!("[GUI] Pause confirmation timeout for '{}', proceeding with stop", name);
                    break;
                }

                if !tmux.session_exists(&name) {
                    eprintln!("[GUI] Session '{}' already gone", name);
                    // Session disappeared — nothing to destroy
                    let current = state.current_session.read().unwrap();
                    if current.as_ref() == Some(&name) {
                        drop(current);
                        *state.current_session.write().unwrap() = None;
                    }
                    return Ok(());
                }

                if let Ok(screen) = tmux.capture_output(&name, None, Some(20)) {
                    // Look for pause confirmation markers
                    if screen.contains("Session paused")
                        || screen.contains("session-pause")
                        || screen.contains("✅")
                        || screen.contains("Paused")
                        || screen.contains("LATEST-SESSION")
                    {
                        eprintln!("[GUI] Pause confirmed for '{}'", name);
                        break;
                    }
                }
            }
        }
    }

    tmux.destroy_session(&name)
        .map_err(|e| format!("Failed to stop session: {}", e))?;

    // If we were connected to this session, disconnect
    let current = state.current_session.read().unwrap();
    if current.as_ref() == Some(&name) {
        drop(current); // Release read lock before acquiring write lock
        *state.current_session.write().unwrap() = None;
    }

    Ok(())
}

#[tauri::command]
pub async fn send_message(
    content: String,
    app: tauri::AppHandle,
    state: State<'_, GuiState>,
) -> Result<(), String> {
    // Note: GUI-specific slash commands (/status, /health, /usage, etc.) should be
    // intercepted in the frontend (InputArea.svelte) before calling this command.
    // Everything that reaches here goes directly to the tmux session / adapter.
    let session_name = state
        .current_session
        .read()
        .unwrap()
        .clone()
        .ok_or("Not connected to a session")?;

    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

    eprintln!("[GUI] Sending message to session '{}': {}", session_name, content);

    // Verify session exists before sending
    if !tmux.session_exists(&session_name) {
        eprintln!("[GUI] Error: Session '{}' not found", session_name);
        return Err(format!("Session '{}' not found", session_name));
    }

    tmux.send_line(&session_name, None, &content)
        .map_err(|e| {
            eprintln!("[GUI] Failed to send message to '{}': {}", session_name, e);
            e.to_string()
        })?;

    // Notify the summary polling loop that a new block has started.
    let _ = app.emit(
        "user-sent",
        serde_json::json!({ "session": session_name }),
    );

    eprintln!("[GUI] Message sent successfully to '{}'", session_name);
    Ok(())
}

/// Send a message using the mpm serve SSE streaming API (port 7777).
///
/// Events emitted to the frontend via `chat-event`:
/// - `{"type": "text", "content": "...", "accumulated": "..."}` — incremental assistant text
/// - `{"type": "tool_use", "name": "...", "input": {...}}` — tool invocation
/// - `{"type": "complete", "content": "...", "cost_usd": 0.0}` — end of response
/// - `{"type": "error", "content": "..."}` — error from the session
///
/// Falls back to `tmux.send_line` when mpm serve is not reachable.
#[tauri::command]
pub async fn send_message_streaming(
    content: String,
    app: tauri::AppHandle,
    state: State<'_, GuiState>,
) -> Result<(), String> {
    use futures_util::StreamExt;

    let session = state
        .current_session
        .read()
        .unwrap()
        .clone()
        .ok_or("Not connected")?;

    let client = reqwest::Client::new();
    let url = format!(
        "http://localhost:7777/api/v1/sessions/{}/messages",
        session
    );

    eprintln!(
        "[GUI] send_message_streaming: POST {} (session='{}')",
        url, session
    );

    // Notify the summary polling loop that a new block has started.
    let _ = app.emit(
        "user-sent",
        serde_json::json!({ "session": session }),
    );

    let response = client
        .post(&url)
        .json(&serde_json::json!({
            "content": content,
            "stream": true
        }))
        .send()
        .await;

    // Suppress the polling-loop interpreter while mpm-serve owns the
    // chat-event channel.  Cleared on all exit paths below.
    state
        .streaming_active
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let streaming_flag = state.streaming_active.clone();

    // Use an inner async block so we can clear the flag uniformly on any exit.
    let result: Result<(), String> = async {
        match response {
            Ok(resp) if resp.status().is_success() => {
                eprintln!("[GUI] send_message_streaming: SSE stream opened");
                let mut stream = resp.bytes_stream();
                let mut accumulated_text = String::new();
                let mut buffer = String::new();

                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|e| e.to_string())?;
                    buffer.push_str(&String::from_utf8_lossy(&chunk));

                    // Process all complete newline-terminated lines in the buffer.
                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        let Some(data) = line.strip_prefix("data: ") else {
                            continue;
                        };

                        let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
                            continue;
                        };

                        let event_type = event
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("unknown");

                        match event_type {
                            "text" | "assistant" => {
                                if let Some(text) =
                                    event.get("content").and_then(|c| c.as_str())
                                {
                                    accumulated_text.push_str(text);
                                    let _ = app.emit(
                                        "chat-event",
                                        serde_json::json!({
                                            "type": "text",
                                            "content": text,
                                            "accumulated": accumulated_text,
                                        }),
                                    );
                                }
                            }
                            "tool_use" => {
                                let name = event
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("unknown");
                                let _ = app.emit(
                                    "chat-event",
                                    serde_json::json!({
                                        "type": "tool_use",
                                        "name": name,
                                        "input": event.get("input"),
                                    }),
                                );
                            }
                            "message_stop" | "result" => {
                                let cost = event.get("cost_usd").and_then(|c| c.as_f64());
                                let _ = app.emit(
                                    "chat-event",
                                    serde_json::json!({
                                        "type": "complete",
                                        "content": accumulated_text,
                                        "cost_usd": cost,
                                    }),
                                );
                            }
                            "error" => {
                                let msg = event
                                    .get("message")
                                    .or_else(|| event.get("content"))
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("Unknown error");
                                let _ = app.emit(
                                    "chat-event",
                                    serde_json::json!({
                                        "type": "error",
                                        "content": msg,
                                    }),
                                );
                            }
                            _ => {}
                        }
                    }
                }

                eprintln!("[GUI] send_message_streaming: SSE stream complete");
                Ok(())
            }
            err => {
                // Log why the SSE path was skipped so it's easy to diagnose.
                match &err {
                    Ok(resp) => eprintln!(
                        "[GUI] send_message_streaming: mpm serve returned {}, falling back to tmux",
                        resp.status()
                    ),
                    Err(e) => eprintln!(
                        "[GUI] send_message_streaming: mpm serve unreachable ({}), falling back to tmux",
                        e
                    ),
                }

                // Release the polling-loop suppression *before* sending via tmux
                // so the polling interpreter can pick up Claude's response.
                streaming_flag.store(false, std::sync::atomic::Ordering::Relaxed);

                let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

                if !tmux.session_exists(&session) {
                    return Err(format!("Session '{}' not found", session));
                }

                tmux.send_line(&session, None, &content)
                    .map_err(|e| e.to_string())?;

                Ok(())
            }
        }
    }
    .await;

    // Ensure the flag is always cleared, even on early returns / errors above.
    state
        .streaming_active
        .store(false, std::sync::atomic::Ordering::Relaxed);

    result
}

#[tauri::command]
pub async fn start_bot(state: State<'_, GuiState>) -> Result<BotInfo, String> {
    use commander_telegram::daemon;

    let pid = daemon::start().map_err(|e| format!("Failed to start bot: {}", e))?;

    let mut status = state.bot_status.write().unwrap();
    status.running = true;
    status.pid = Some(pid);

    Ok(BotInfo {
        running: true,
        pid: Some(pid),
    })
}

#[tauri::command]
pub async fn stop_bot(state: State<'_, GuiState>) -> Result<(), String> {
    use commander_telegram::daemon;

    daemon::stop().map_err(|e| format!("Failed to stop bot: {}", e))?;

    let mut status = state.bot_status.write().unwrap();
    status.running = false;
    status.pid = None;

    Ok(())
}

#[tauri::command]
pub async fn get_bot_status(_state: State<'_, GuiState>) -> Result<BotInfo, String> {
    use commander_telegram::daemon;

    let status = daemon::status();

    Ok(BotInfo {
        running: status.running,
        pid: status.pid,
    })
}

#[tauri::command]
pub async fn generate_pairing_code() -> Result<String, String> {
    use commander_telegram::pairing;

    // Create a pairing with empty project/session for GUI-level authorization
    // The empty strings tell the bot to just authorize without auto-connecting
    let code = pairing::create_pairing("", "")
        .map_err(|e| format!("Failed to create pairing: {}", e))?;

    Ok(code)
}

/// Generate a web client pairing code via the daemon's PairingManager.
///
/// The returned code is displayed to the user in the GUI and must be entered
/// in the web client to obtain a session token via `POST /api/auth/pair`.
#[tauri::command]
pub async fn generate_web_pairing_code() -> Result<WebPairingCode, String> {
    use commander_daemon::PairingManager;

    let mut manager =
        PairingManager::new().map_err(|e| format!("Failed to create pairing manager: {}", e))?;

    let code = manager
        .generate_code(None, None)
        .map_err(|e| format!("Failed to generate pairing code: {}", e))?;

    let entry = manager
        .get_entry(&code)
        .ok_or_else(|| "Generated code not found in pairing store".to_string())?;

    let now = chrono::Utc::now();
    let expires_in_seconds = (entry.expires_at - now)
        .num_seconds()
        .max(0) as u64;

    Ok(WebPairingCode {
        code,
        expires_at: entry.expires_at.to_rfc3339(),
        expires_in_seconds,
    })
}

/// Response from `generate_web_pairing_code`.
#[derive(Serialize, Deserialize)]
pub struct WebPairingCode {
    /// 6-character alphanumeric code the user enters in the web client.
    pub code: String,
    /// RFC 3339 timestamp when the code expires.
    pub expires_at: String,
    /// Seconds remaining until expiry.
    pub expires_in_seconds: u64,
}

#[derive(Serialize, Deserialize)]
pub struct TelegramConnection {
    pub connected: bool,
    pub chat_ids: Vec<i64>,
    pub count: usize,
}

#[tauri::command]
pub async fn check_telegram_connection() -> Result<TelegramConnection, String> {
    use commander_core::config;
    use std::fs;

    // Read authorized chats file
    let chats_file = config::authorized_chats_file();

    if !chats_file.exists() {
        return Ok(TelegramConnection {
            connected: false,
            chat_ids: vec![],
            count: 0,
        });
    }

    let content = fs::read_to_string(&chats_file)
        .map_err(|e| format!("Failed to read authorized chats: {}", e))?;

    let chat_ids: Vec<i64> = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse authorized chats: {}", e))?;

    Ok(TelegramConnection {
        connected: !chat_ids.is_empty(),
        chat_ids: chat_ids.clone(),
        count: chat_ids.len(),
    })
}

#[derive(Serialize, Deserialize)]
pub struct ProjectDirectory {
    pub name: String,
    pub path: String,
    pub project_type: String, // "claude-code" or "mpm"
}

#[derive(Serialize, Deserialize)]
pub struct AdapterInfo {
    pub id: String,
    pub name: String,
    pub command: String,
}

#[tauri::command]
pub fn list_adapters() -> Vec<AdapterInfo> {
    vec![
        AdapterInfo { id: "claude-code".to_string(), name: "Claude Code".to_string(), command: "claude".to_string() },
        AdapterInfo { id: "claude-mpm".to_string(), name: "Claude MPM".to_string(), command: "claude-mpm".to_string() },
        AdapterInfo { id: "auggie".to_string(), name: "Auggie".to_string(), command: "auggie".to_string() },
        AdapterInfo { id: "codex".to_string(), name: "Codex".to_string(), command: "codex".to_string() },
        AdapterInfo { id: "shell".to_string(), name: "Shell".to_string(), command: "bash".to_string() },
    ]
}

#[tauri::command]
pub async fn list_project_directories() -> Result<Vec<ProjectDirectory>, String> {
    use std::collections::HashSet;

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

    let mut dirs: Vec<ProjectDirectory> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;

    // Read scan_directories from config, fall back to defaults.
    let scan_dirs: Vec<String> = {
        let config_path = PathBuf::from(&home).join(".ai-commander").join("config.json");
        if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
                .and_then(|val| {
                    val.get("scan_directories")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect::<Vec<String>>()
                        })
                })
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| DEFAULT_SCAN_DIRECTORIES.iter().map(|s| s.to_string()).collect())
        } else {
            DEFAULT_SCAN_DIRECTORIES.iter().map(|s| s.to_string()).collect()
        }
    };

    // Phase 1: Scan project roots for directories that look like projects.
    // Use canonicalize to deduplicate case-insensitive paths (macOS APFS)
    let scan_roots: Vec<PathBuf> = scan_dirs
        .iter()
        .map(|subdir| PathBuf::from(&home).join(subdir))
        .filter(|p| p.is_dir())
        .collect();

    for root in &scan_roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let has_claude = path.join(".claude").is_dir();
            let has_mpm = path.join(".claude-mpm").is_dir();
            let is_project = PROJECT_MARKERS
                .iter()
                .any(|marker| path.join(marker).exists());
            let is_git = path.join(".git").is_dir();

            if !has_claude && !has_mpm && !is_project && !is_git {
                continue;
            }

            // Canonicalize to resolve symlinks and case-insensitive duplicates
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            let path_str = canonical.to_string_lossy().to_string();
            if !seen_paths.insert(path_str.clone()) {
                continue;
            }

            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let mut adapters: Vec<&str> = Vec::new();
            if has_claude {
                adapters.push("claude-code");
            }
            if has_mpm {
                adapters.push("claude-mpm");
            }

            dirs.push(ProjectDirectory {
                name,
                path: path_str,
                project_type: adapters.join(", "),
            });
        }
    }

    // Phase 2: Load registered projects from StateStore (authoritative source)
    let state_dir = commander_core::config::state_dir();
    let store = commander_persistence::StateStore::new(state_dir);
    if let Ok(projects) = store.load_all_projects() {
        for (_id, project) in projects {
            let p = PathBuf::from(&project.path);
            let canonical = p.canonicalize().unwrap_or(p);
            let path_str = canonical.to_string_lossy().to_string();
            if seen_paths.insert(path_str.clone()) {
                let adapter = project
                    .adapter_type
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| "claude-code".to_string());
                dirs.push(ProjectDirectory {
                    name: project.name,
                    path: path_str,
                    project_type: adapter,
                });
            }
        }
    }

    // Phase 3: Decode ~/.claude/projects/ entries as fallback
    // Directory names are dash-encoded paths: "-Users-masa-Projects-foo" → "/Users/masa/Projects/foo"
    let cc_config_path = PathBuf::from(&home).join(".claude/projects");
    if let Ok(entries) = std::fs::read_dir(&cc_config_path) {
        for entry in entries.flatten() {
            let encoded = entry.file_name();
            let Some(encoded_str) = encoded.to_str() else {
                continue;
            };
            // The encoding: leading '-' represents the root '/', remaining '-' are path separators.
            // "-Users-masa-Projects-hot-flash" → "/Users/masa/Projects/hot-flash"
            // We reconstruct by replacing the first '-' with '/' and all subsequent '-' with '/'.
            // However, project names with hyphens are ambiguous, so we only include paths that
            // actually exist on disk.
            let decoded = format!("/{}", encoded_str.trim_start_matches('-').replace('-', "/"));
            let decoded_path = PathBuf::from(&decoded);
            if !decoded_path.is_dir() {
                continue;
            }
            let canonical = decoded_path.canonicalize().unwrap_or_else(|_| decoded_path.clone());
            let canonical_str = canonical.to_string_lossy().to_string();
            if !seen_paths.insert(canonical_str.clone()) {
                continue;
            }
            let proj_name = canonical
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            dirs.push(ProjectDirectory {
                name: proj_name,
                path: canonical_str,
                project_type: "claude-code".to_string(),
            });
        }
    }

    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(dirs)
}

#[tauri::command]
pub async fn rebuild_from_source(app: tauri::AppHandle) -> Result<String, String> {
    // CARGO_MANIFEST_DIR is embedded at compile time — it points to the
    // commander-gui crate directory.  Walk up two levels to reach the
    // workspace root that owns the top-level Cargo.toml.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()           // crates/
        .and_then(|p| p.parent()) // workspace root
        .ok_or_else(|| "Cannot determine workspace root from CARGO_MANIFEST_DIR".to_string())?;

    let workspace_cargo = workspace_root.join("Cargo.toml");
    if !workspace_cargo.exists() {
        return Err(format!(
            "Source code not found: {} does not exist",
            workspace_cargo.display()
        ));
    }

    let gui_dir = workspace_root.join("crates/commander-gui");

    eprintln!(
        "[GUI] rebuild_from_source: spawning cargo tauri build (fire-and-forget) in {}",
        workspace_root.display()
    );

    // Build frontend, rebuild Tauri app, then reopen automatically
    let app_path = workspace_root
        .join("target/release/bundle/macos/AIC - AI Commander.app");
    let script = format!(
        "cd {:?}/ui && npm run build && cd {:?} && cargo tauri build --bundles app && open {:?}",
        gui_dir, gui_dir, app_path
    );

    std::process::Command::new("sh")
        .args(["-c", &script])
        .current_dir(workspace_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn rebuild: {}", e))?;

    eprintln!("[GUI] rebuild_from_source: build spawned, quitting app");

    // Quit the app so the new build replaces it
    app.exit(0);

    Ok("Rebuilding... app will quit now.".to_string())
}

#[tauri::command]
pub async fn create_session(
    name: String,
    directory: String,
    adapter: String,
    state: State<'_, GuiState>,
) -> Result<(), String> {
    eprintln!("[GUI] Creating session '{}' with adapter '{}' in '{}'", name, adapter, directory);

    // Create a tmux session and launch the adapter CLI inside it.
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

    // Check if session already exists
    if tmux.session_exists(&name) {
        return Err(format!("Session '{}' already exists", name));
    }

    // Create session in specified directory
    tmux.create_session_in_dir(&name, Some(&directory))
        .map_err(|e| e.to_string())?;

    // Set the tmux session title (OSC 0/2 sequence) so terminal emulators
    // attaching to this session show the nickname-or-session-name as the
    // window/tab title. Uses `resolve_session_display_name` which consults
    // overrides and project JSONs — at creation time the project JSON may
    // not exist yet, so this usually resolves to `name` itself.
    let display = resolve_session_display_name(&name);
    set_tmux_session_title(&name, &display);

    // Determine the adapter launch command
    let launch_cmd = match adapter.as_str() {
        "claude-code" => "claude --dangerously-skip-permissions",
        "claude-mpm" => "claude-mpm",  // MPM inherits --dangerously-skip-permissions via its own config
        "auggie" => "auggie",
        "codex" => "codex",
        "shell" => "", // No command needed for bare shell
        _ => "claude --dangerously-skip-permissions",
    };

    // Launch the adapter inside the tmux session
    if !launch_cmd.is_empty() {
        eprintln!("[GUI] Launching adapter '{}' in session '{}'", launch_cmd, name);
        // Small delay to let the shell initialize before sending the command
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        tmux.send_line(&name, None, launch_cmd)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn rename_session(
    old_name: String,
    new_name: String,
    state: State<'_, GuiState>,
) -> Result<(), String> {
    let _tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

    if new_name.trim().is_empty() {
        return Err("Session name cannot be empty".to_string());
    }

    std::process::Command::new("tmux")
        .args(["rename-session", "-t", &old_name, &new_name])
        .output()
        .map_err(|e| format!("Failed to rename session: {}", e))?;

    // Record the user's chosen name as the winning display name for this
    // tmux session. Without this override the path-based project-nickname
    // lookup in `list_sessions` keeps reassigning the old project name on
    // every poll — users see their rename silently revert.
    //
    // The display name IS `new_name` (what the user typed). The tmux session
    // was just renamed to that exact string, so key == value here.
    write_session_override(&new_name, &new_name);
    // The old tmux name no longer exists — drop any stale override for it.
    remove_session_override(&old_name);

    // Also update the matching project JSON's `name` field so the display-name
    // (which comes from the nickname lookup in list_sessions) reflects the rename.
    // Without this, list_sessions would continue matching the old project by path
    // and overriding the new tmux name with the stale nickname.
    //
    // Matching uses the same logic as list_sessions:
    //   sanitized(project.name) == old_name
    //   OR project.path == old_name
    //   OR project.path == <pane_current_path of renamed session>
    let pane_path = std::process::Command::new("tmux")
        .args(["display-message", "-p", "-t", &new_name, "#{pane_current_path}"])
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

    // Fetch the full session list once so we can count how many sessions share
    // the same project path. If more than one session maps to the same project
    // JSON, we must NOT update that file — doing so would rename all of them.
    let all_sessions: Vec<String> = state
        .tmux
        .as_ref()
        .and_then(|t| t.list_sessions().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.name)
        .collect();

    if let Ok(entries) = std::fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |x| x == "json") {
                let Ok(content) = std::fs::read_to_string(&path) else { continue };
                let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) else { continue };
                let proj_name = val.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let proj_path = val.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let sanitized = proj_name.replace([' ', '.', '/', ':'], "-");
                let matches = sanitized == old_name
                    || proj_path == old_name
                    || pane_path.as_deref() == Some(proj_path);
                if matches {
                    // Count how many active tmux sessions share this project path.
                    // If more than one session maps to the same JSON, skip the write:
                    // renaming the JSON would silently rename all of them on next poll.
                    let matching_count = all_sessions.iter().filter(|s| {
                        let session_path = std::process::Command::new("tmux")
                            .args(["display-message", "-p", "-t", s.as_str(), "#{pane_current_path}"])
                            .output()
                            .ok()
                            .and_then(|o| {
                                let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                                if p.is_empty() { None } else { Some(p) }
                            });
                        session_path.as_deref() == Some(proj_path)
                    }).count();

                    if matching_count <= 1 {
                        val["name"] = serde_json::Value::String(new_name.clone());
                        if let Ok(updated) = serde_json::to_string_pretty(&val) {
                            let _ = std::fs::write(&path, updated);
                        }
                    }
                    break;
                }
            }
        }
    }

    // Update current session tracking if the renamed session was active
    let mut current = state.current_session.write().unwrap();
    if current.as_ref().map(|s| s == &old_name).unwrap_or(false) {
        *current = Some(new_name);
    }

    Ok(())
}

/// Set (or clear) the display nickname for a session without renaming tmux.
///
/// Why: Users want a friendly display label per session that doesn't require
/// changing the underlying tmux session name (which risks breaking existing
/// tooling that matches on the tmux name). The override file is the same one
/// used by `rename_session` — write it directly and let the session list pick
/// the override up on the next poll.
/// What: Writes `session_name -> nickname` to the overrides JSON. An empty
/// (or whitespace-only) nickname removes the entry, reverting the session to
/// its project-name-derived default.
/// Test: Call with a non-empty nickname, assert the overrides file contains
/// the mapping; call again with an empty string, assert the mapping is gone.
#[tauri::command]
pub async fn set_session_nickname(
    session_name: String,
    nickname: String,
    _state: State<'_, GuiState>,
) -> Result<(), String> {
    if nickname.trim().is_empty() {
        // Empty nickname = remove the override (revert to project name)
        remove_session_override(&session_name);
    } else {
        write_session_override(&session_name, nickname.trim());
    }
    Ok(())
}

#[tauri::command]
pub async fn open_in_terminal_app(session_name: String) -> Result<(), String> {
    // Session names may contain shell glob metacharacters (e.g. "cto [cto3]").
    // When AppleScript's `do script` hands the command to the user's login shell
    // (usually zsh), brackets are interpreted as glob patterns, producing
    // `zsh: no matches found: [cto3]`. Single-quote the session name so the
    // shell treats it as a literal string. The escape-single-quote idiom
    // `'\''` closes the quoted span, inserts a literal `'`, and reopens it.
    //
    // We also escape any embedded double quotes so the AppleScript string
    // literal that wraps the shell command isn't broken out of by a crafted
    // session name.
    let shell_safe = session_name.replace('\'', r"'\''");
    let applescript_safe = shell_safe.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"tell application "Terminal"
            activate
            do script "tmux attach -t '{}'"
        end tell"#,
        applescript_safe
    );

    std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .spawn()
        .map_err(|e| format!("Failed to open Terminal.app: {}", e))?;

    Ok(())
}

#[derive(Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub memory_mb: f64,
    pub session: Option<String>,
    pub age_seconds: u64,
    pub stale: bool,
}

/// Minimum age in seconds before a process can be considered stale.
///
/// Why: Freshly-spawned processes (a second-old python helper) should never be
/// killed just because we haven't yet correlated them with a tmux session.
const STALE_MIN_AGE_SECONDS: u64 = 300; // 5 minutes

/// Max tmux inactivity (seconds) before we stop treating the session as "recently active".
///
/// Why: Bug 1 — an active session was killed because the correlated tmux session
/// happened to not appear in our list at the moment of the scan. Sessions with
/// recent pane activity (a running process writing output, a user typing) must
/// never be classified as stale, regardless of correlation. 30 minutes is a
/// conservative threshold that covers long LLM turns and slow test runs without
/// leaving truly-dead sessions alive forever.
const TMUX_RECENT_ACTIVITY_SECONDS: u64 = 30 * 60;

/// Process name/command substrings that are NEVER eligible to be killed.
///
/// Why: URGENT bug — the process monitor was killing active claude-mpm
/// sessions when their command line did not contain the tmux session name
/// verbatim. This allowlist is the last line of defense: AI assistant
/// processes and commander binaries must never be swept by "Clean stale".
/// Mirrors the list in `commander-api/src/handlers/web.rs`.
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

/// Returns true when the process name or command matches a protected pattern.
fn is_protected_process(command: &str) -> bool {
    let cmd_lower = command.to_lowercase();
    PROTECTED_PROCESS_PATTERNS
        .iter()
        .any(|pat| cmd_lower.contains(pat))
}

/// Query live tmux session names (empty on failure — treated as "no sessions").
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

/// Check whether a tmux session had pane activity within the last
/// `TMUX_RECENT_ACTIVITY_SECONDS`.
///
/// Why: Bug 1 guard — even if a session didn't correlate by name match, if its
/// pane was active in the last 30 minutes it is almost certainly still in use
/// (long LLM turn, test run, file watch). Refusing to classify such sessions
/// as stale costs us one `tmux display-message` per candidate process — cheap.
/// What: Runs `tmux display-message -t <session> -p "#{session_activity}"`
/// (milliseconds since tmux epoch) and returns true when `(now - activity_ms)`
/// is less than the threshold. Parse failure → treat as "recently active" to
/// stay on the safe side.
/// Test: Create session `foo`; immediately call with `"foo"`, assert true.
/// Kill `foo` and call again, assert false (session missing → false).
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
    // tmux emits milliseconds since the Unix epoch.
    let activity_ms: u64 = match raw.parse() {
        Ok(v) => v,
        // Any unexpected output → be conservative and keep the session alive.
        Err(_) => return true,
    };
    let activity_secs = activity_ms / 1000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now <= activity_secs {
        // Clock skew — be conservative.
        return true;
    }
    (now - activity_secs) < TMUX_RECENT_ACTIVITY_SECONDS
}

/// Correlate a process with a tmux session by command-line substring match.
///
/// Why: tmux-spawned commands typically include the session name somewhere in
/// their argv (pane titles, working dirs, wrapper scripts). A substring match
/// over the full command line is an imperfect but conservative heuristic —
/// we'd rather over-associate (keep a process alive) than under-associate
/// (accidentally kill an in-use process).
fn correlate_session(command: &str, tmux_sessions: &[String]) -> Option<String> {
    let lower = command.to_lowercase();
    tmux_sessions
        .iter()
        .find(|s| lower.contains(&s.to_lowercase()))
        .cloned()
}

/// List commander-related processes and classify each as stale or active.
///
/// Why: The monitor UI needs a list of candidate processes and a stale flag so
/// the user can decide what to kill. The stale flag is the gate that controls
/// whether `kill_stale_processes` would target a row, so the logic must be
/// conservative.
/// What: Scans `ps aux` output, filters to python/node/cargo processes that
/// are not part of AI Commander itself, correlates each with a live tmux
/// session by name, and sets `stale = true` only when ALL of:
///   1. The correlated tmux session (if any) is NOT in `tmux list-sessions`.
///   2. `age_seconds > STALE_MIN_AGE_SECONDS`.
///   3. The process is NOT associated with a session in `connected_sessions`.
/// Test: Spawn a long-running python, create a tmux session whose name appears
/// in its command, assert the returned row has `stale == false`. Kill the
/// tmux session, wait, assert `stale` flips to true only after the age
/// threshold has passed.
#[tauri::command]
pub async fn list_processes(state: State<'_, GuiState>) -> Result<Vec<ProcessInfo>, String> {
    let output = std::process::Command::new("ps")
        .args(["aux"])
        .output()
        .map_err(|e| e.to_string())?;

    let tmux_sessions = live_tmux_sessions();
    let connected: std::collections::HashSet<String> = state
        .connected_sessions
        .read()
        .map(|g| g.clone())
        .unwrap_or_default();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();

    for line in stdout.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 11 {
            continue;
        }

        let command = fields[10..].join(" ");

        let is_tracked = command.contains("python")
            || command.contains("node")
            || command.contains("cargo");

        if !is_tracked {
            continue;
        }

        // Skip our own processes (the GUI, the daemon)
        if command.contains("commander-gui") || command.contains("commander-daemon") {
            continue;
        }

        let pid: u32 = fields[1].parse().unwrap_or(0);
        let cpu: f32 = fields[2].parse().unwrap_or(0.0);

        // Use the RSS column (field index 5) for memory in KB
        let rss_kb: f64 = fields[5].parse().unwrap_or(0.0);
        let memory_mb = rss_kb / 1024.0;

        // age_seconds: parse elapsed time from the TIME field (field 9) — "MM:SS" or "HH:MM:SS"
        let age_seconds = parse_ps_time(fields[9]);

        // Correlate with a live tmux session. If we find one, the process is
        // NOT stale — it is owned by an active session.
        let session = correlate_session(&command, &tmux_sessions);

        // Belt-and-suspenders stale check. A process is stale only when ALL of:
        //   - its correlated tmux session (if any) is NOT alive per `tmux list-sessions`
        //   - the correlated session is NOT in `connected_sessions` (actively monitored)
        //   - the correlated session has NO recent pane activity (guards against a
        //     session being temporarily absent from our list snapshot — Bug 1)
        //   - age exceeds the min threshold so we never kill freshly-spawned helpers
        let session_alive = session
            .as_ref()
            .map(|s| tmux_sessions.iter().any(|t| t == s))
            .unwrap_or(false);
        let is_connected_session = session
            .as_ref()
            .map(|s| connected.contains(s))
            .unwrap_or(false);
        // If correlated, check recency; otherwise we have nothing to query.
        let recently_active = session
            .as_ref()
            .map(|s| tmux_session_recently_active(s))
            .unwrap_or(false);
        // Protected-process guard: AI assistant processes must never be
        // classified as stale, even when correlation fails.
        let protected = is_protected_process(&command);

        let stale = !protected
            && !session_alive
            && !is_connected_session
            && !recently_active
            && age_seconds > STALE_MIN_AGE_SECONDS;

        let name = if command.len() > 80 {
            format!("{}...", &command[..77])
        } else {
            command.to_string()
        };

        processes.push(ProcessInfo {
            pid,
            name,
            cpu,
            memory_mb,
            session,
            age_seconds,
            stale,
        });
    }

    // Sort: stale first, then by memory descending
    processes.sort_by(|a, b| {
        b.stale
            .cmp(&a.stale)
            .then(b.memory_mb.partial_cmp(&a.memory_mb).unwrap_or(std::cmp::Ordering::Equal))
    });

    Ok(processes)
}

/// Parse the CPU time field from `ps aux` (column 9, zero-indexed).
/// Accepts "MM:SS" or "HH:MM:SS" formats and returns total seconds.
fn parse_ps_time(time_str: &str) -> u64 {
    let parts: Vec<&str> = time_str.split(':').collect();
    match parts.as_slice() {
        [mm, ss] => {
            let minutes: u64 = mm.parse().unwrap_or(0);
            let seconds: u64 = ss.parse().unwrap_or(0);
            minutes * 60 + seconds
        }
        [hh, mm, ss] => {
            let hours: u64 = hh.parse().unwrap_or(0);
            let minutes: u64 = mm.parse().unwrap_or(0);
            let seconds: u64 = ss.parse().unwrap_or(0);
            hours * 3600 + minutes * 60 + seconds
        }
        _ => 0,
    }
}

/// Kill every process that `list_processes` flags as stale.
///
/// Why: After Bug 1 (active sessions were being killed), the single source of
/// truth for "is this process killable?" is `list_processes`. We re-run its
/// check here rather than duplicating heuristics. We also re-verify with a
/// fresh `tmux has-session` call per row, which closes the race window where
/// a session came back between list and kill.
/// What: Enumerates stale rows from `list_processes`, double-checks that each
/// row's correlated tmux session is still absent and that the session is not
/// currently in `connected_sessions`, then sends SIGTERM.
/// Test: Seed a python process whose command contains "foo-session", create
/// tmux session "foo-session", call this, assert 0 kills. Kill the tmux
/// session, wait past STALE_MIN_AGE_SECONDS, assert the python is killed.
/// Tauri command: kill stale commander processes (requires `confirm: true`).
///
/// Why: URGENT fix — this command was killing active claude-mpm sessions.
/// The callers now MUST pass `confirm = true` to actually kill; any other
/// value performs a dry-run and returns 0 so the UI can preview the list.
/// A defense-in-depth `is_protected_process` check is applied per row so
/// even if the classifier regresses, AI assistant processes cannot be
/// killed by this path.
/// What: Enumerates stale rows, filters out protected processes, re-checks
/// tmux state, and only calls `kill -TERM` when `confirm == true`.
/// Test: Call with `confirm = false`, assert returned count is 0 and no
/// process is killed. Spawn a process named `claude-mpm` with no tmux
/// correlation and age > STALE_MIN_AGE_SECONDS; call with `confirm = true`;
/// assert the process is NOT killed (protected allowlist).
#[tauri::command]
pub async fn kill_stale_processes(
    state: State<'_, GuiState>,
    confirm: Option<bool>,
) -> Result<u32, String> {
    let confirm = confirm.unwrap_or(false);
    let processes = list_processes(state.clone()).await?;
    let live_sessions = live_tmux_sessions();
    let connected: std::collections::HashSet<String> = state
        .connected_sessions
        .read()
        .map(|g| g.clone())
        .unwrap_or_default();

    let mut killed = 0u32;
    let mut would_kill = 0u32;

    for proc in processes.iter().filter(|p| p.stale) {
        // Defense-in-depth allowlist: never kill protected processes even if
        // the classifier ever regresses.
        if is_protected_process(&proc.name) {
            eprintln!(
                "[GUI] Skipping kill for PID {} — name '{}' is on the protected allowlist",
                proc.pid, proc.name
            );
            continue;
        }

        // Final safety gate: refuse to kill if a correlated session has come
        // back, is actively connected, or had recent pane activity between the
        // list and the kill. Recency protects against the Bug 1 scenario where
        // a session transiently disappears from our list snapshot.
        if let Some(ref s) = proc.session {
            if live_sessions.iter().any(|t| t == s)
                || connected.contains(s)
                || tmux_session_recently_active(s)
            {
                eprintln!(
                    "[GUI] Skipping kill for PID {} — session '{}' is alive/connected/recent",
                    proc.pid, s
                );
                continue;
            }
        }

        if !confirm {
            would_kill += 1;
            eprintln!(
                "[GUI] DRY-RUN: would kill process {} (PID {})",
                proc.name, proc.pid
            );
            continue;
        }

        let result = std::process::Command::new("kill")
            .args(["-TERM", &proc.pid.to_string()])
            .output();

        if result.is_ok() {
            killed += 1;
            eprintln!("[GUI] Killed stale process {} (PID {})", proc.name, proc.pid);
        }
    }

    if !confirm {
        eprintln!(
            "[GUI] DRY-RUN complete: {} process(es) would be killed. Pass confirm=true to kill.",
            would_kill
        );
        return Ok(0);
    }

    Ok(killed)
}

/// Proxy GitHub stats from the locally-running commander-api server.
///
/// The REST server listens on `ApiConfig::default().port` (9876) and exposes
/// `/api/github-stats`, which returns `{ "stats": { "<session>": { ... } } }`.
/// The Tauri frontend doesn't speak REST directly (it goes through IPC), so we
/// fetch once here and forward the JSON unchanged.
#[tauri::command]
pub async fn get_github_stats() -> Result<serde_json::Value, String> {
    let port = commander_api::ApiConfig::default().port;
    let url = format!("http://localhost:{}/api/github-stats", port);

    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to reach {}: {}", url, e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub stats request returned {}", response.status()));
    }

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse GitHub stats: {}", e))
}

/// List dates for which summary log files exist for a session.
///
/// Why: The chat view loads today's log on connect and needs to know if any
/// history exists without an extra probe.
/// What: Returns `Vec<String>` of `YYYY-MM-DD` dates, sorted ascending.
/// Test: Seed two log files under a fake HOME, invoke this command, assert
/// the returned vec matches the sorted file stems.
#[tauri::command]
pub async fn list_session_log_dates(name: String) -> Result<Vec<String>, String> {
    Ok(commander_core::list_log_dates(&name))
}

/// Fetch summary log entries for a session on a specific date.
///
/// Why: The chat view replays past summaries as system messages so users see
/// what happened before the GUI was opened.
/// What: Returns `Vec<LogEntry>` (`{ts, text, hash}`). Empty if no entries.
/// Test: Seed a jsonl file with two entries, call with matching date, assert
/// exactly those two entries are returned in order.
#[tauri::command]
pub async fn get_session_log(
    name: String,
    date: String,
) -> Result<Vec<commander_core::LogEntry>, String> {
    Ok(commander_core::read_log_entries(&name, &date))
}

/// Archive all summary logs for a session into a zip file.
///
/// Why: Users want to export/snapshot a session's history in one artifact
/// (e.g. before destroying the session or sharing with a teammate).
/// What: Invokes `commander_core::archive_session_logs` which shells out to
/// the system `zip` CLI. Returns the absolute archive path.
/// Test: Seed a log file under a fake HOME, call this command, assert the
/// returned path exists and is a non-empty file.
#[tauri::command]
pub async fn archive_session_logs(name: String) -> Result<String, String> {
    commander_core::archive_session_logs(&name)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

/// Resolve the preferred display name for a session for window-titling purposes.
///
/// Why: When we open a tmux session in iTerm2 or send a window-title escape to
/// tmux, the label the user actually recognises is the project nickname (e.g.
/// "aic"), not the sanitized tmux session name (e.g. "Projects-ai-commander").
/// Pull from the same sources `list_sessions` does so all three UIs converge on
/// the same label.
/// What: Returns, in priority order: (1) the session-override file entry,
/// (2) the project JSON `name` whose `path` matches the session's pane cwd,
/// (3) the raw tmux session name.
/// Test: Seed an override for "foo" -> "FOO"; call with "foo", assert "FOO".
/// Remove the override, seed a project JSON with a path matching the session,
/// call again, assert the project `name` is returned.
pub(crate) fn resolve_session_display_name(session_name: &str) -> String {
    // 1. Check session-overrides.json (user-set nicknames win over everything).
    let overrides = read_session_overrides();
    if let Some(name) = overrides.get(session_name) {
        if !name.trim().is_empty() {
            return name.clone();
        }
    }

    // 2. Check the project registry — match by path if possible.
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

    // 3. Fall back to the raw tmux session name.
    session_name.to_string()
}

#[tauri::command]
pub async fn open_in_iterm(
    session_name: String,
    _state: State<'_, GuiState>,
) -> Result<(), String> {
    // Open iTerm2 and attach to the named tmux session.
    // If a window already exists, open a new tab rather than a new window.
    // The tab title is set to the session's display name (nickname if available)
    // so users can tell tabs apart by the project label rather than the raw
    // tmux session name.
    //
    // Session names may contain shell metacharacters: spaces (from tmux rename),
    // brackets (e.g. "cto [cto3]" which zsh interprets as a glob pattern and
    // fails with "zsh: no matches found: [cto3]"), parentheses, etc. iTerm2's
    // `write text` hands the string to the shell, so we must shell-quote the
    // target. The escape-single-quote idiom `'\''` safely terminates and
    // re-opens the quoted span so a literal `'` can pass through unchanged.
    //
    // We also escape backslashes and double quotes so a crafted session name
    // cannot break out of the AppleScript string literal that wraps the
    // shell-quoted command.
    let display_name = resolve_session_display_name(&session_name);
    let shell_safe_session = session_name.replace('\'', r"'\''");
    let escaped_session = shell_safe_session
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let escaped_display = display_name.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"tell application "iTerm2"
            activate
            if (count of windows) = 0 then
                create window with default profile
            else
                tell current window
                    create tab with default profile
                end tell
            end if
            tell current session of current tab of current window
                write text "tmux attach -t '{session}'"
                set name to "{display}"
            end tell
        end tell"#,
        session = escaped_session,
        display = escaped_display,
    );

    std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .spawn()
        .map_err(|e| format!("Failed to open iTerm2: {}", e))?;

    // Also mirror the display name into tmux itself so that terminal emulators
    // attaching outside the AppleScript flow (and any "Open in Terminal.app"
    // path) render the same label. We set the tmux session option `set-titles`
    // plus `set-titles-string` so tmux emits the OSC 0/2 sequence to whichever
    // terminal is attached — this does NOT inject text into the running adapter.
    set_tmux_session_title(&session_name, &display_name);

    Ok(())
}

/// Set the terminal window/tab title emitted by tmux for a session.
///
/// Why: tmux can emit OSC 0/2 title-set sequences to whichever terminal is
/// attached (iTerm2, Terminal.app, Ghostty, etc.) via `set-titles on` plus a
/// custom `set-titles-string`. Doing this on the tmux session rather than on
/// the terminal emulator itself means the label follows the session across
/// attach/detach without us having to pipe escape codes through the adapter.
/// What: Sets `set-titles on` and `set-titles-string` to the literal `title`
/// (tmux format strings would be surprising here — we just want the raw
/// label). Session-scoped so sibling sessions stay independent.
/// Test: Call with ("foo", "My Project"); assert
/// `tmux show-options -t foo -v set-titles-string` returns "My Project".
fn set_tmux_session_title(session: &str, title: &str) {
    // Enable title emission for this session. `-q` silences "not supported"
    // warnings on older tmuxes; `-t` scopes it to the given session.
    let _ = std::process::Command::new("tmux")
        .args(["set-option", "-q", "-t", session, "set-titles", "on"])
        .output();
    // The title string. tmux interprets `#` in the string as a format escape;
    // replace it with a harmless variant to avoid misrendering project names
    // that happen to contain `#`.
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
