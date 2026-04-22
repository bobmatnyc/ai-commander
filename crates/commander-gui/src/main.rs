#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod state;

use tauri::{Emitter, Manager};

/// Auto-connect every running tmux session on startup.
///
/// Why: Users expect the app to "just work" on launch — requiring a manual
/// click to connect each existing tmux session creates friction and leaves
/// the ChatView silent for sessions that are actively producing output. By
/// enumerating `tmux ls` at startup and replicating the core side effect of
/// `connect_session` (insert into `connected_sessions`), the polling loop
/// (`events::start_session_polling`) immediately begins emitting
/// `session-output` / `chat-event` events for each one.
/// What: Iterates all live tmux sessions, inserts each name into
/// `state.connected_sessions`, and emits a `session-auto-connected` event per
/// session so the frontend can refresh its session list view. Silently
/// no-ops when tmux is unavailable or no sessions exist.
/// Test: Start tmux with two sessions, launch the app, assert both names
/// appear in `connected_sessions` and two `session-auto-connected` events
/// are emitted to the frontend.
async fn auto_connect_running_sessions(app: tauri::AppHandle, state: state::GuiState) {
    // Brief delay so the Tauri window is fully initialised and ready to
    // receive events before we fan out notifications. Without this, very
    // fast startup paths can race the frontend's `listen()` registration.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let Some(tmux) = state.tmux.as_ref() else {
        // No tmux orchestrator — feature unavailable, silent no-op.
        return;
    };

    let sessions = match tmux.list_sessions() {
        Ok(s) => s,
        Err(_) => {
            // tmux server not running or command failed — nothing to connect.
            return;
        }
    };

    if sessions.is_empty() {
        return;
    }

    // Snapshot existing connections so we only auto-connect sessions the user
    // hasn't already manually touched (e.g. a previous disconnect during this
    // run — unlikely at startup but defensive).
    let already_connected: std::collections::HashSet<String> = state
        .connected_sessions
        .read()
        .map(|g| g.clone())
        .unwrap_or_default();

    let mut newly_connected: Vec<String> = Vec::new();
    {
        let mut connected = match state.connected_sessions.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for s in &sessions {
            if already_connected.contains(&s.name) {
                continue;
            }
            connected.insert(s.name.clone());
            newly_connected.push(s.name.clone());
        }
    }

    if newly_connected.is_empty() {
        return;
    }

    eprintln!(
        "[GUI] auto-connected {} running tmux session(s): {:?}",
        newly_connected.len(),
        newly_connected
    );

    // Emit a per-session event so the frontend can update its UI state
    // (mark each row as connected, prompt a session-list refresh, etc.).
    for name in &newly_connected {
        let _ = app.emit(
            "session-auto-connected",
            serde_json::json!({ "session": name }),
        );
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let state = state::GuiState::new().expect("Failed to initialize GUI state");

            let app_handle = app.handle().clone();
            let state_clone = state.clone();

            // Start background polling
            tauri::async_runtime::spawn(async move {
                events::start_session_polling(app_handle, state_clone).await;
            });

            // Set web-dist path for static file serving if not already set
            if std::env::var("AIC_WEB_DIR").is_err() {
                // Look for web-dist relative to the binary or workspace
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()));
                if let Some(dir) = &exe_dir {
                    // Check bundle Resources/web-dist first
                    let bundle_web = dir.join("../Resources/web-dist");
                    let workspace_web = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .parent().and_then(|p| p.parent())
                        .map(|p| p.join("web-dist"));

                    if bundle_web.is_dir() {
                        std::env::set_var("AIC_WEB_DIR", bundle_web);
                    } else if let Some(ws) = workspace_web {
                        if ws.is_dir() {
                            std::env::set_var("AIC_WEB_DIR", ws);
                        }
                    }
                }
            }

            // Start commander-api REST server
            let api_handle = tauri::async_runtime::spawn(async move {
                use commander_api::{ApiConfig, AppState};
                use commander_adapters::AdapterRegistry;
                use commander_events::EventManager;
                use commander_persistence::{EventStore, WorkStore};
                use commander_work::WorkQueue;

                let state_dir = commander_core::config::state_dir();
                let event_store = EventStore::new(&state_dir);
                let work_store = WorkStore::new(&state_dir);

                let api_state = AppState::new_with_storage(
                    ApiConfig::default(),
                    None,
                    EventManager::new(event_store),
                    WorkQueue::new(work_store),
                    AdapterRegistry::new(),
                    state_dir.clone(),
                );

                if let Err(e) = commander_api::serve(ApiConfig::default(), api_state).await {
                    eprintln!("API server error: {}", e);
                }
            });

            // Store the API server handle for cleanup
            if let Ok(mut handle) = state.api_server_handle.write() {
                *handle = Some(api_handle);
            }

            // Start daemon service (IPC + idle monitor + health checks)
            let daemon_handle = tauri::async_runtime::spawn(async move {
                match commander_daemon::service::DaemonService::new().await {
                    Ok(service) => {
                        if let Err(e) = service.run().await {
                            eprintln!("Daemon service error: {}", e);
                        }
                    }
                    Err(e) => eprintln!("Failed to start daemon: {}", e),
                }
            });

            // Store the daemon handle for cleanup
            if let Ok(mut handle) = state.daemon_handle.write() {
                *handle = Some(daemon_handle);
            }

            // Auto-connect every running tmux session in the background so
            // the app is immediately useful on launch without requiring each
            // session to be clicked. Runs non-blocking; the window opens now
            // and connections fan out ~500ms later.
            let auto_connect_handle = app.handle().clone();
            let auto_connect_state = state.clone();
            tauri::async_runtime::spawn(async move {
                auto_connect_running_sessions(auto_connect_handle, auto_connect_state).await;
            });

            app.manage(state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::connect_session,
            commands::disconnect_session,
            commands::delete_registration,
            commands::unregister_session,
            commands::stop_session,
            commands::send_message,
            commands::send_message_streaming,
            commands::start_bot,
            commands::stop_bot,
            commands::get_bot_status,
            commands::generate_pairing_code,
            commands::check_telegram_connection,
            commands::list_project_directories,
            commands::create_session,
            commands::list_adapters,
            commands::rebuild_from_source,
            commands::generate_web_pairing_code,
            commands::open_in_iterm,
            commands::capture_session_output,
            commands::rename_session,
            commands::set_session_nickname,
            commands::open_in_terminal_app,
            commands::list_processes,
            commands::kill_stale_processes,
            commands::interpret_session,
            commands::get_session_summary,
            commands::get_github_stats,
            commands::list_session_log_dates,
            commands::get_session_log,
            commands::archive_session_logs,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Abort background server tasks when the last window closes
                if let Some(gui_state) = window.app_handle().try_state::<state::GuiState>() {
                    if let Ok(mut handle) = gui_state.api_server_handle.write() {
                        if let Some(h) = handle.take() {
                            h.abort();
                        }
                    }
                    if let Ok(mut handle) = gui_state.daemon_handle.write() {
                        if let Some(h) = handle.take() {
                            h.abort();
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
