#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod state;

use tauri::Manager;

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

            // Start commander-api REST server on port 8765
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

            app.manage(state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::connect_session,
            commands::disconnect_session,
            commands::stop_session,
            commands::send_message,
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
