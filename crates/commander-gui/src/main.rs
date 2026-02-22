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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
