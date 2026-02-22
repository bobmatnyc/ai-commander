use crate::state::GuiState;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::time::{sleep, Duration};

#[derive(Clone, Serialize)]
pub struct SessionOutput {
    pub session: String,
    pub output: String,
}

pub async fn start_session_polling(app: AppHandle, state: GuiState) {
    tokio::spawn(async move {
        loop {
            if let Some(session_name) = state.current_session.read().unwrap().clone() {
                if let Some(tmux) = &state.tmux {
                    if let Ok(output) = tmux.capture_output(&session_name, None, Some(50)) {
                        if !output.is_empty() {
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

            sleep(Duration::from_millis(500)).await;
        }
    });
}
