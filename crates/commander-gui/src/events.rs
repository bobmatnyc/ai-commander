use crate::state::GuiState;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::time::{sleep, Duration};

#[derive(Clone, Serialize)]
pub struct SessionOutput {
    pub session: String,
    pub output: String,
}

pub async fn start_session_polling(app: AppHandle, state: GuiState) {
    tokio::spawn(async move {
        let last_hashes: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));

        loop {
            if let Some(session_name) = state.current_session.read().unwrap().clone() {
                if let Some(tmux) = &state.tmux {
                    if let Ok(output) = tmux.capture_output(&session_name, None, Some(50)) {
                        if !output.is_empty() {
                            // Calculate hash of output
                            let mut hasher = DefaultHasher::new();
                            output.hash(&mut hasher);
                            let current_hash = hasher.finish();

                            // Check if output changed
                            let mut hashes = last_hashes.lock().unwrap();
                            let last_hash = hashes.get(&session_name).copied();

                            if last_hash != Some(current_hash) {
                                // Output changed - emit event
                                hashes.insert(session_name.clone(), current_hash);
                                drop(hashes); // Release lock before async emit

                                let _ = app.emit(
                                    "session-output",
                                    SessionOutput {
                                        session: session_name.clone(),
                                        output,
                                    },
                                );
                            }
                        }
                    }
                }
            }

            sleep(Duration::from_millis(500)).await;
        }
    });
}
