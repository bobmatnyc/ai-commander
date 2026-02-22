use crate::state::GuiState;
use serde::{Deserialize, Serialize};
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
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

    if !tmux.session_exists(&name) {
        return Err(format!("Session '{}' not found", name));
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
pub async fn send_message(content: String, state: State<'_, GuiState>) -> Result<(), String> {
    let session_name = state
        .current_session
        .read()
        .unwrap()
        .clone()
        .ok_or("Not connected to a session")?;

    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

    tmux.send_line(&session_name, None, &content)
        .map_err(|e| e.to_string())?;

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
    // Placeholder implementation - will depend on bot pairing mechanism
    // TODO: Implement actual pairing code generation
    Ok("12345678".to_string())
}
