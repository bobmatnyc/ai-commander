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
    let mut dirs = Vec::new();
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;

    // Check ~/.claude/projects/ for Claude Code projects
    let cc_path = PathBuf::from(&home).join(".claude/projects");
    if let Ok(entries) = std::fs::read_dir(&cc_path) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    dirs.push(ProjectDirectory {
                        name: name.to_string(),
                        path: entry.path().to_string_lossy().to_string(),
                        project_type: "claude-code".to_string(),
                    });
                }
            }
        }
    }

    // Check ~/.claude-mpm/projects/ for MPM projects
    let mpm_path = PathBuf::from(&home).join(".claude-mpm/projects");
    if let Ok(entries) = std::fs::read_dir(&mpm_path) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    dirs.push(ProjectDirectory {
                        name: name.to_string(),
                        path: entry.path().to_string_lossy().to_string(),
                        project_type: "mpm".to_string(),
                    });
                }
            }
        }
    }

    // Also check current working directory for any projects
    if let Ok(current_dir) = std::env::current_dir() {
        // Check if current directory has .claude or package.json
        if current_dir.join(".claude").exists() || current_dir.join("package.json").exists() {
            if let Some(name) = current_dir.file_name().and_then(|n| n.to_str()) {
                dirs.push(ProjectDirectory {
                    name: name.to_string(),
                    path: current_dir.to_string_lossy().to_string(),
                    project_type: "current-dir".to_string(),
                });
            }
        }
    }

    Ok(dirs)
}

#[tauri::command]
pub async fn rebuild_from_source() -> Result<String, String> {
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

    eprintln!(
        "[GUI] rebuild_from_source: running cargo build in {}",
        workspace_root.display()
    );

    let output = tokio::process::Command::new("cargo")
        .args(["build", "-p", "commander-gui", "--release"])
        .current_dir(workspace_root)
        .output()
        .await
        .map_err(|e| format!("Failed to spawn cargo: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}{}", stdout, stderr).trim().to_string();

    if output.status.success() {
        eprintln!("[GUI] rebuild_from_source: build succeeded");
        Ok(combined)
    } else {
        let code = output.status.code().unwrap_or(-1);
        eprintln!("[GUI] rebuild_from_source: build failed (exit {})", code);
        Err(format!("Build failed (exit {}):\n{}", code, combined))
    }
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

    Ok(())
}
