# Handler Hang Investigation — 2026-04-10

**Symptom:** User sent message to chat `5235493571` at `2026-04-10T23:03:24Z`. Log shows `Regular message received` but no response was ever sent. Bot stayed alive until manual restart at `23:32:08Z` (~29 minutes later).

## Timeline

- `23:01:58` — User opened deep link `connect_cto-14` (disconnected from `apex-7`, attached to unregistered tmux session `cto-14`, `adapter_type=claude-code`, `is_event_driven=false`). This is a **terminal (tmux) session**.
- `23:02:02` — Deep link connection successful.
- `23:03:24.167` — `INFO Regular message received chat_id=5235493571` (bot.rs:250).
- `23:03:24.167` → `23:32:08` — **Complete log silence.** Not just for this chat: every chat's `poll_output` loop, `typing_throttle` logs, and all other traffic stop at the same instant. The Tokio dispatcher is frozen.
- `23:32:08` — Bot restarted; session `cto-14` restored from disk.

Other chats were still polling normally up to ~`23:06:47` (log lines 34590+ for unrelated chats), so the freeze propagates a few seconds after the message arrives — consistent with the next task that touches the sessions lock blocking.

## Environment at Time of Hang

- tmux server alive, `cto-14` session still present and attached.
- `claude-mpm serve` not running. Port 7777 has no listener (`curl` fails, `exit=7`). Not relevant — this is a terminal session, not event-driven.
- No summarizer / OpenRouter calls in flight at that time.

## Execution Path

`handle_message` (handlers.rs:1337) for a private, non-`@`, non-reply text on a terminal session:

1. `state.is_authorized` (read lock) — OK
2. `state.has_session` (read lock) — OK
3. `typing_throttled` — OK
4. `state.is_event_driven_session` — returns false
5. **`state.send_message(...)`** (state.rs:1478) — this is where it dies.

Inside `send_message`:
```rust
let mut sessions = self.sessions.write().await;    // line 1483 — WRITE LOCK held
let session = sessions.get_mut(&chat_id.0)?;
let last_output = tmux.capture_output(...)          // line 1489 — BLOCKING syscall
    .unwrap_or_default();
```

## Root Cause

`TmuxOrchestrator::capture_output` (`crates/commander-tmux/src/orchestrator.rs:248`) is a **synchronous** function that shells out to `tmux capture-pane` via `std::process::Command`. It is invoked **while holding the `sessions` RwLock write guard**. If that `tmux` subprocess hangs — tmux server wedged on an attached client, a pane in a bad state, or the command blocking on I/O — the holding Tokio worker thread is pinned and the write guard is never released. Every other task in the dispatcher that needs `sessions.read()` or `.write()` (poll loop, typing throttle, every handler) queues behind it. The entire bot appears dead even though the process is alive. This matches the observed global log silence perfectly.

Likely trigger: `cto-14` was "unregistered" (connected via raw deep link, not through the normal connect flow), so this was the first `capture_output` call on that pane via this code path — an unusual state that may have tripped a tmux edge case.

## Recommended Fixes (in priority order)

1. **Do not hold the write lock across blocking I/O.** Clone the `tmux_session` name under the lock, drop the guard, run `capture_output`, then reacquire a write guard to call `start_response_collection`. Minimal diff, high impact.
2. **Wrap `capture_output` in `tokio::task::spawn_blocking` with a timeout** (e.g. 2s). Never let a stuck tmux command freeze a Tokio worker. Return `Err` on timeout and surface a user-facing error instead of silent hang.
3. **Use `tokio::sync::RwLock::try_write_for(...)` or structure `send_message` so the lock scope is purely in-memory** (no process spawns, no `.await` of I/O).
4. **Shard the `sessions` lock per-chat** (e.g. `DashMap<i64, Mutex<Session>>`) so one stuck session cannot block unrelated chats' poll loops.

Fix #1 alone resolves the global-freeze symptom; #2 prevents the single-chat hang.
