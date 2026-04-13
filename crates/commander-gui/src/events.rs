use crate::state::GuiState;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::time::{sleep, Duration};

/// Maximum lines to capture from tmux scrollback per poll cycle.
const CAPTURE_LINES: u32 = 500;

/// Tracks polling state for a single session.
struct SessionState {
    /// Number of lines last observed (used to detect new content).
    line_count: usize,
    /// Hash of the full captured output (used to detect in-place edits when
    /// the line count stays the same, e.g. Claude overwriting a status line).
    content_hash: u64,
}

#[derive(Clone, Serialize)]
pub struct SessionOutput {
    pub session: String,
    /// Incremental new lines appended since the last event.
    /// Empty when a full refresh is needed instead.
    pub content: String,
    /// Full captured output snapshot.  Always populated so the frontend can
    /// rebuild state after a screen clear or session restart.
    pub full_content: String,
}

fn hash_str(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Strip ANSI escape sequences from tmux output for cleaner display.
fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip ESC sequence: ESC [ ... <terminator-letter>
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            // Other ESC sequences (e.g. ESC M, ESC =) are consumed with the ESC char.
        } else {
            result.push(ch);
        }
    }
    result
}

/// Box-drawing Unicode ranges used by Claude Code's TUI chrome.
const BOX_DRAWING_START: char = '\u{2500}';
const BOX_DRAWING_END: char = '\u{257F}';
const BOX_ELEMENT_START: char = '\u{2580}';
const BOX_ELEMENT_END: char = '\u{259F}';

/// Return true if the line is UI chrome that should be suppressed.
fn is_ui_noise(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Lines composed entirely of box-drawing / block-element characters and whitespace.
    let all_box = trimmed.chars().all(|c| {
        c.is_whitespace()
            || (c >= BOX_DRAWING_START && c <= BOX_DRAWING_END)
            || (c >= BOX_ELEMENT_START && c <= BOX_ELEMENT_END)
    });
    if all_box {
        return true;
    }

    // Lines that start/end with │ (columnar TUI layout — the welcome banner)
    if trimmed.starts_with('│') || trimmed.starts_with('|') {
        return true;
    }

    // Claude-mpm startup chrome
    if trimmed.contains("Welcome back")
        || trimmed.contains("What's new")
        || trimmed.contains("No changelog")
        || trimmed.contains("/mpm-help")
        || trimmed.contains("/mpm-agents")
        || trimmed.contains("/mpm-doctor")
        || trimmed.contains("MPM Commands")
        || trimmed.contains("Type / for autocomplete")
        || trimmed.contains("Loading claude-mpm")
        || trimmed.contains("Syncing hooks")
        || trimmed.contains("Verified")
        || trimmed.contains("bypass permissions")
        || trimmed.contains("shift+tab to cycle")
    {
        return true;
    }

    // Progress bars and spinners
    if trimmed.contains("[█") || trimmed.contains("[■") || trimmed.contains("░") {
        return true;
    }

    false
}

/// Strip ANSI codes from `input` and remove pure UI-chrome lines.
fn clean_output(input: &str) -> String {
    let stripped = strip_ansi(input);
    stripped
        .lines()
        .filter(|line| !is_ui_noise(line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn start_session_polling(app: AppHandle, state: GuiState) {
    let session_states: Arc<Mutex<HashMap<String, SessionState>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut cleanup_counter: u32 = 0;

    loop {
        if let Some(session_name) = state.current_session.read().unwrap().clone() {
            if let Some(tmux) = &state.tmux {
                if let Ok(raw_output) = tmux.capture_output(&session_name, None, Some(CAPTURE_LINES)) {
                    if !raw_output.is_empty() {
                        // Strip ANSI codes and UI-chrome before change detection so that
                        // spurious escape-sequence churn doesn't trigger false positives,
                        // and the frontend always receives clean text.
                        let output = clean_output(&raw_output);
                        let lines: Vec<&str> = output.lines().collect();
                        let current_line_count = lines.len();
                        let current_hash = hash_str(&output);

                        let mut states = session_states.lock().unwrap();
                        let prev = states.get(&session_name);

                        let event = match prev {
                            None => {
                                // First observation — emit full snapshot.
                                Some(SessionOutput {
                                    session: session_name.clone(),
                                    content: output.clone(),
                                    full_content: output.clone(),
                                })
                            }
                            Some(s) if current_line_count > s.line_count => {
                                // New lines appended — emit only the new tail.
                                let new_lines = lines[s.line_count..].join("\n");
                                Some(SessionOutput {
                                    session: session_name.clone(),
                                    content: new_lines,
                                    full_content: output.clone(),
                                })
                            }
                            Some(s) if current_line_count < s.line_count => {
                                // Line count dropped: screen cleared or session restarted.
                                // Emit full snapshot so the frontend can reset.
                                Some(SessionOutput {
                                    session: session_name.clone(),
                                    content: output.clone(),
                                    full_content: output.clone(),
                                })
                            }
                            Some(s) if current_hash != s.content_hash => {
                                // Same line count but content differs (e.g. Claude
                                // overwrote a progress line).  Emit full refresh.
                                Some(SessionOutput {
                                    session: session_name.clone(),
                                    content: String::new(),
                                    full_content: output.clone(),
                                })
                            }
                            _ => None, // No change detected.
                        };

                        // Update tracked state before dropping the lock.
                        states.insert(
                            session_name.clone(),
                            SessionState {
                                line_count: current_line_count,
                                content_hash: current_hash,
                            },
                        );
                        drop(states);

                        if let Some(payload) = event {
                            let _ = app.emit("session-output", payload);
                        }
                    }
                }
            }
        }

        // Periodically clean up stale state entries for destroyed sessions (~every 30s).
        cleanup_counter += 1;
        if cleanup_counter >= 60 {
            cleanup_counter = 0;
            if let Some(tmux) = &state.tmux {
                if let Ok(active_sessions) = tmux.list_sessions() {
                    let active_names: std::collections::HashSet<String> =
                        active_sessions.into_iter().map(|s| s.name).collect();
                    let mut states = session_states.lock().unwrap();
                    states.retain(|name, _| active_names.contains(name));
                }
            }
        }

        sleep(Duration::from_millis(500)).await;
    }
}
