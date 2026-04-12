use crate::state::GuiState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

#[derive(Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub created_at: String,
    pub is_connected: bool,
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

    Ok(sessions
        .into_iter()
        .map(|s| SessionInfo {
            name: s.name.clone(),
            created_at: s.created_at.to_string(),
            is_connected: state
                .current_session
                .read()
                .unwrap()
                .as_ref()
                == Some(&s.name),
        })
        .collect())
}

#[tauri::command]
pub async fn connect_session(name: String, state: State<'_, GuiState>) -> Result<(), String> {
    let tmux = state.tmux.as_ref().ok_or(
        "Cannot connect: tmux is not available. Make sure tmux is installed and accessible."
    )?;

    if !tmux.session_exists(&name) {
        return Err(format!(
            "Session '{}' does not exist. Available sessions can be seen in the list.",
            name
        ));
    }

    *state.current_session.write().unwrap() = Some(name.clone());
    Ok(())
}

#[tauri::command]
pub async fn disconnect_session(state: State<'_, GuiState>) -> Result<(), String> {
    *state.current_session.write().unwrap() = None;
    Ok(())
}

#[tauri::command]
pub async fn stop_session(name: String, state: State<'_, GuiState>) -> Result<(), String> {
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

    if !tmux.session_exists(&name) {
        return Err(format!("Session '{}' not found", name));
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
pub async fn send_message(content: String, state: State<'_, GuiState>) -> Result<(), String> {
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

    eprintln!("[GUI] Message sent successfully to '{}'", session_name);
    Ok(())
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

    let mut dirs: Vec<ProjectDirectory> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;

    // Phase 1: Scan common project roots for directories containing .claude or .claude-mpm
    let scan_roots = [
        PathBuf::from(&home).join("Projects"),
        PathBuf::from(&home).join("projects"),
        PathBuf::from(&home).join("src"),
        PathBuf::from(&home).join("work"),
        PathBuf::from(&home).join("dev"),
    ];

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

            if !has_claude && !has_mpm {
                continue;
            }

            let path_str = path.to_string_lossy().to_string();
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
            let path_str = project.path.clone();
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
            if !seen_paths.insert(decoded.clone()) {
                continue;
            }
            let proj_name = decoded_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            dirs.push(ProjectDirectory {
                name: proj_name,
                path: decoded,
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

    // Build frontend first, then cargo tauri build — fire and forget
    let script = format!(
        "cd {:?}/ui && npm run build && cd {:?} && cargo tauri build --bundles app",
        gui_dir, gui_dir
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
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

    // Check if session already exists
    if tmux.session_exists(&name) {
        return Err(format!("Session '{}' already exists", name));
    }

    eprintln!("[GUI] Creating session '{}' with adapter '{}' in '{}'", name, adapter, directory);

    // Create session in specified directory
    tmux.create_session_in_dir(&name, Some(&directory))
        .map_err(|e| e.to_string())?;

    // Determine the adapter launch command
    let launch_cmd = match adapter.as_str() {
        "claude-code" => "claude",
        "claude-mpm" => "claude-mpm",
        "auggie" => "auggie",
        "codex" => "codex",
        "shell" => "", // No command needed for bare shell
        _ => "claude", // Default to claude
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
pub async fn open_in_iterm(
    session_name: String,
    _state: State<'_, GuiState>,
) -> Result<(), String> {
    // Open iTerm2 and attach to the named tmux session
    let script = format!(
        r#"tell application "iTerm2"
            activate
            create window with default profile
            tell current session of current window
                write text "tmux attach -t {}"
            end tell
        end tell"#,
        session_name
    );

    std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .spawn()
        .map_err(|e| format!("Failed to open iTerm2: {}", e))?;

    Ok(())
}
