use crate::state::GuiState;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Listener};
use tokio::time::{sleep, Duration};

/// Maximum lines to capture from tmux scrollback per poll cycle.
const CAPTURE_LINES: u32 = 500;
/// Poll interval in milliseconds.
const POLL_INTERVAL_MS: u64 = 500;
/// Minimum gap between LLM invocations per session (ms). Keeps Ollama from
/// being hammered while output is streaming.
const LLM_THROTTLE_MS: u64 = 5_000;
/// Minimum gap between LLM invocations during the startup window. Shorter
/// than `LLM_THROTTLE_MS` so the initial burst of output is consolidated
/// into a single summary quickly rather than emitting 2-3 rapid blocks.
const LLM_THROTTLE_STARTUP_MS: u64 = 3_000;
/// Duration of the startup window (ms). After this elapses we fall back to
/// normal block behavior even if no LLM summary has succeeded yet.
const STARTUP_WINDOW_MS: u64 = 15_000;
/// Number of consecutive LLM failures before emitting an `llm_unavailable`
/// event to the UI. Gives transient errors (network blip, Ollama reload)
/// a chance to recover before alarming the user.
const LLM_FAILURE_THRESHOLD: u32 = 2;
/// Max characters of the block buffer sent to the LLM per call.
/// Keeps round-trips fast on local 7-8b models; `interpret_screen_context`
/// truncates internally too, so this is a belt-and-braces cap.
const MAX_BUFFER_CHARS: usize = 4000;

/// Tracks polling state for a single session.
///
/// Why: Summary mode streams a single running summary per "block" (the
/// interval between two user inputs). We need just enough state to (a)
/// detect when tmux output has actually changed, (b) accumulate new output
/// into the current block, (c) throttle LLM calls, and (d) emit periodic
/// "Summarizing…" pulses while waiting for the throttle to expire.
/// What: Hash of last-seen raw output, the accumulated block buffer, the
/// last LLM-call timestamp, the last thinking-pulse timestamp, a first-poll
/// flag for immediate summaries on connect, and the running summary text.
/// Test: Instantiate, feed two identical polls, assert the second is a
/// no-op (hash match); feed a third with new content, assert the buffer
/// grows.
struct SessionState {
    /// Hash of the last seen raw tmux capture (change detection).
    prev_hash: u64,
    /// Accumulated raw output since the last user input.
    block_buffer: String,
    /// Timestamp (ms since UNIX epoch) of the last LLM call.
    last_llm_call_ms: u64,
    /// Timestamp (ms since UNIX epoch) of the last "Summarizing…" thinking pulse.
    last_thinking_ms: u64,
    /// True until the first successful poll for this session. On first poll with
    /// existing buffer content, we bypass the 5s throttle for an immediate summary.
    is_first_poll: bool,
    /// Last summary text, passed back to the LLM as context for the next update.
    current_summary: String,
    /// False until either the first LLM summary succeeds or the startup window
    /// elapses. While false, new_block events are suppressed (so the initial
    /// banner + first-ready output consolidate into a single block) and the
    /// LLM is polled more aggressively.
    startup_complete: bool,
    /// Timestamp (ms since UNIX epoch) when this session's state was created.
    /// Used to compute elapsed time inside the startup window.
    startup_start_ms: u64,
    /// Number of consecutive LLM failures (None returned). Reset to 0 on any
    /// successful summary. Once this reaches `LLM_FAILURE_THRESHOLD` an
    /// `llm_unavailable` event is emitted to the UI.
    llm_failure_count: u32,
    /// True after we've emitted `llm_unavailable` for this session, to avoid
    /// hammering the UI with repeat banners.
    llm_unavailable_emitted: bool,
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

/// Payload of the `user-sent` Tauri event emitted by `send_message` /
/// `send_message_streaming` when the user sends new input.
#[derive(Clone, Deserialize)]
struct UserSentEvent {
    session: String,
}

fn hash_str(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Keep only the last `MAX_BUFFER_CHARS` of `buffer` (on a char boundary).
///
/// Why: `interpret_screen_context` is fed the block buffer directly; a
/// runaway block shouldn't blow past the LLM's context window.
/// What: Returns the tail slice of `buffer` up to `MAX_BUFFER_CHARS`.
/// Test: A short string is returned unchanged; a string longer than the cap
/// returns exactly the last `MAX_BUFFER_CHARS` chars.
fn truncate_tail(buffer: &str) -> &str {
    if buffer.len() <= MAX_BUFFER_CHARS {
        return buffer;
    }
    let start = buffer.len() - MAX_BUFFER_CHARS;
    // Snap forward to the next char boundary.
    let mut idx = start;
    while idx < buffer.len() && !buffer.is_char_boundary(idx) {
        idx += 1;
    }
    &buffer[idx..]
}

pub async fn start_session_polling(app: AppHandle, state: GuiState) {
    let session_states: Arc<Mutex<HashMap<String, SessionState>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Listen for `user-sent` events emitted by send_message /
    // send_message_streaming. Resets the block for that session so the next
    // LLM summary starts fresh instead of piling onto the previous block.
    {
        let session_states = Arc::clone(&session_states);
        let app_listen = app.clone();
        app.listen("user-sent", move |event| {
            if let Ok(payload) = serde_json::from_str::<UserSentEvent>(event.payload()) {
                // 1. Emit immediate "Working…" placeholder so the user sees
                //    instant feedback that their input was received.
                let _ = app_listen.emit(
                    "chat-event",
                    serde_json::json!({
                        "type": "thinking",
                        "content": "Working\u{2026}",
                        "session": payload.session,
                    }),
                );

                // 2. Reset the block so the next LLM summary starts fresh.
                let mut states = session_states.lock().unwrap();
                if let Some(s) = states.get_mut(&payload.session) {
                    s.block_buffer.clear();
                    s.current_summary.clear();
                    s.last_llm_call_ms = 0;
                    s.last_thinking_ms = 0;
                }
                drop(states);
                let _ = app_listen.emit(
                    "chat-event",
                    serde_json::json!({
                        "type": "new_block",
                        "session": payload.session,
                    }),
                );
            }
        });
    }

    let mut cleanup_counter: u32 = 0;

    loop {
        // Poll every session in `connected_sessions`, not just the one currently
        // displayed in ChatView. `poll_once` emits `chat-event` payloads tagged
        // with the session name, and the ChatView already filters events by
        // session name, so multi-session polling routes correctly on the
        // frontend without further changes.
        let connected: Vec<String> = state
            .connected_sessions
            .read()
            .unwrap()
            .iter()
            .cloned()
            .collect();

        if let Some(tmux) = &state.tmux {
            for session_name in &connected {
                if let Ok(raw_output) = tmux.capture_output(session_name, None, Some(CAPTURE_LINES)) {
                    if !raw_output.is_empty() {
                        poll_once(
                            &app,
                            &state,
                            &session_states,
                            session_name,
                            &raw_output,
                        );
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

        sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
    }
}

/// Execute one poll cycle for a session.
///
/// Why: Isolates the per-poll logic so the main loop stays tiny and this
/// is testable/readable on its own.
/// What: Detects changes, accumulates new output, emits a `session-output`
/// event for the raw feed, and throttles an LLM summary call that updates
/// the running summary bubble.
/// Test: Drive with two identical `raw_output` values — the second poll
/// must not trigger an LLM call (throttle + unchanged hash).
fn poll_once(
    app: &AppHandle,
    state: &GuiState,
    session_states: &Arc<Mutex<HashMap<String, SessionState>>>,
    session_name: &str,
    raw_output: &str,
) {
    let hash = hash_str(raw_output);

    // ── Accumulate block buffer & emit raw session-output feed ──
    let (block_snapshot, should_call_llm, should_pulse_thinking) = {
        let mut states = session_states.lock().unwrap();
        let entry = states.entry(session_name.to_string()).or_insert_with(|| SessionState {
            prev_hash: 0,
            block_buffer: String::new(),
            last_llm_call_ms: 0,
            last_thinking_ms: 0,
            is_first_poll: true,
            current_summary: String::new(),
            startup_complete: false,
            startup_start_ms: now_ms(),
            llm_failure_count: 0,
            llm_unavailable_emitted: false,
        });

        if entry.prev_hash == hash {
            // No change since last poll — skip entirely.
            return;
        }

        // Append new content. Suffix-anchor match against `raw_output`; if the
        // buffer ends with a run also present in `raw_output`, only the delta
        // past that run is appended. Otherwise `raw_output` is a fresh screen
        // and replaces the accumulated tail.
        let delta = compute_delta(&entry.block_buffer, raw_output);
        if !delta.is_empty() {
            entry.block_buffer.push_str(&delta);
        }
        entry.prev_hash = hash;

        // Behavior 2: on the first poll, if the buffer already has content
        // (session was mid-work when we connected), reset last_llm_call_ms to 0
        // so the elapsed check below passes immediately instead of waiting 5s.
        let is_first = entry.is_first_poll;
        if is_first {
            entry.is_first_poll = false;
            if !entry.block_buffer.trim().is_empty() {
                entry.last_llm_call_ms = 0;
            }
        }

        let now = now_ms();
        // Auto-exit startup window after STARTUP_WINDOW_MS even if no LLM
        // summary succeeded — don't block block progression forever on a
        // broken LLM.
        if !entry.startup_complete
            && now.saturating_sub(entry.startup_start_ms) >= STARTUP_WINDOW_MS
        {
            entry.startup_complete = true;
        }
        let in_startup = !entry.startup_complete;
        let throttle = if in_startup { LLM_THROTTLE_STARTUP_MS } else { LLM_THROTTLE_MS };

        let elapsed = now.saturating_sub(entry.last_llm_call_ms);
        let buffer_nonempty = !entry.block_buffer.trim().is_empty();
        let should_call = elapsed >= throttle && buffer_nonempty;

        // Behavior 3: emit a "Summarizing…" pulse every ~2s while we're
        // accumulating but haven't fired the LLM yet.
        let should_pulse = buffer_nonempty
            && !should_call
            && now.saturating_sub(entry.last_thinking_ms) >= 2_000;
        if should_pulse {
            entry.last_thinking_ms = now;
        }

        (entry.block_buffer.clone(), should_call, should_pulse)
    };

    // Emit the raw session-output feed (used by Raw view + activity indicator).
    let _ = app.emit(
        "session-output",
        SessionOutput {
            session: session_name.to_string(),
            content: String::new(),
            full_content: raw_output.to_string(),
        },
    );

    // mpm-serve owns the chat-event channel during streaming — skip the LLM.
    if state.streaming_active.load(Ordering::Relaxed) {
        return;
    }

    // Behavior 3: emit a lightweight "Summarizing…" pulse every ~2s while the
    // buffer is accumulating but the LLM throttle hasn't expired yet. This
    // gives the user a visual heartbeat rather than silence.
    if should_pulse_thinking {
        let _ = app.emit(
            "chat-event",
            serde_json::json!({
                "type": "thinking",
                "content": "Summarizing\u{2026}",
                "session": session_name,
            }),
        );
    }

    if !should_call_llm {
        return;
    }

    // Mark the call time *before* dispatching so we don't race-fire a second
    // call when the blocking task is slow.
    {
        let mut states = session_states.lock().unwrap();
        if let Some(s) = states.get_mut(session_name) {
            s.last_llm_call_ms = now_ms();
        }
    }

    // Hand the raw (unfiltered) block buffer to `interpret_screen_context`;
    // its internal `prepare_for_llm` strips chrome and caps length, so we
    // just ensure we don't spend CPU on a massive runaway block.
    let content = truncate_tail(&block_snapshot).to_string();
    let hash_hex = format!("{:x}", hash);
    let session_for_task = session_name.to_string();
    let app_for_task = app.clone();
    let states_for_task = Arc::clone(session_states);

    tokio::task::spawn_blocking(move || {
        // `is_ready=true` disables the spinner pre-filter; we want a real
        // summary of whatever the LLM can extract from the block.
        let summary = commander_core::interpret_screen_context(&content, true)
            .filter(|s| !s.trim().is_empty());

        match summary {
            Some(text) => {
                // Success: reset failure counter, mark startup complete,
                // and emit the update if the text actually changed.
                let changed = {
                    let mut states = states_for_task.lock().unwrap();
                    match states.get_mut(&session_for_task) {
                        Some(s) => {
                            s.llm_failure_count = 0;
                            s.llm_unavailable_emitted = false;
                            s.startup_complete = true;
                            if s.current_summary != text {
                                s.current_summary = text.clone();
                                true
                            } else {
                                false
                            }
                        }
                        None => false,
                    }
                };
                if !changed {
                    return;
                }
                let _ = commander_core::append_log_entry(&session_for_task, &text, &hash_hex);
                let _ = app_for_task.emit(
                    "chat-event",
                    serde_json::json!({
                        "type": "update",
                        "content": text,
                        "session": session_for_task,
                    }),
                );
            }
            None => {
                // LLM returned nothing — could be "no meaningful content" or
                // a hard backend failure. Count the failure and, once we've
                // seen enough in a row, emit BOTH (a) the `llm_unavailable`
                // banner flag for the persistent UI banner and (b) an
                // explicit `error` chat event explaining why the summary
                // view has stopped updating. Without the error event, a
                // broken-LLM session silently stalls while still emitting
                // `session-output` pulses — users reported that as "raw
                // output bleeding into Summary view" because activity
                // counters tick but nothing readable appears.
                let should_emit = {
                    let mut states = states_for_task.lock().unwrap();
                    match states.get_mut(&session_for_task) {
                        Some(s) => {
                            s.llm_failure_count = s.llm_failure_count.saturating_add(1);
                            if s.llm_failure_count >= LLM_FAILURE_THRESHOLD
                                && !s.llm_unavailable_emitted
                            {
                                s.llm_unavailable_emitted = true;
                                true
                            } else {
                                false
                            }
                        }
                        None => false,
                    }
                };
                if should_emit {
                    let _ = app_for_task.emit(
                        "chat-event",
                        serde_json::json!({
                            "type": "llm_unavailable",
                            "session": session_for_task,
                        }),
                    );
                    // Surface a concrete error message in the chat stream
                    // so users see WHY summaries stopped updating, rather
                    // than having to guess from the banner alone.
                    let _ = app_for_task.emit(
                        "chat-event",
                        serde_json::json!({
                            "type": "error",
                            "content": "LLM unavailable — summaries paused. Switch to Raw view to see terminal output.",
                            "session": session_for_task,
                        }),
                    );
                }
            }
        }
    });
}

/// Compute the portion of `raw` that is new relative to `buffer`.
///
/// Why: tmux captures return a sliding window of scrollback; simple
/// concatenation would dup everything already seen.
/// What: If `raw` starts with the tail of `buffer`, returns the unseen
/// suffix; otherwise returns `raw` verbatim (treated as a fresh screen).
/// Test: buffer="abc", raw="abcdef" → returns "def"; buffer="xyz",
/// raw="abc" → returns "abc"; buffer==raw → returns "".
fn compute_delta(buffer: &str, raw: &str) -> String {
    if raw == buffer {
        return String::new();
    }
    // Try a quick "raw is a suffix-extension of buffer" check using the last
    // N chars of buffer as an anchor.
    const ANCHOR_LEN: usize = 200;
    if !buffer.is_empty() {
        let anchor_start = buffer.len().saturating_sub(ANCHOR_LEN);
        let anchor = &buffer[anchor_start..];
        if let Some(pos) = raw.rfind(anchor) {
            let tail_start = pos + anchor.len();
            if tail_start <= raw.len() {
                return raw[tail_start..].to_string();
            }
        }
    }
    raw.to_string()
}
