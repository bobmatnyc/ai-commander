# Research: Session Persistence and Reconnect-on-Restart

Date: 2026-03-26
Topic: Implementing "reconnect last session and signal user on restart"

---

## Executive Summary

The infrastructure for this feature is **almost entirely already built**. The bot persists
sessions to disk on connect/disconnect, loads them on startup, checks whether the running
binary has changed, and sends a "bot rebuilt" notification. The gap is narrow: the rebuild
notification fires only when the binary hash changed (a cargo rebuild), not on plain
SIGTERM-and-restart cycles. The per-user session name is not included in the notification
message text, and sessions restored from plain restarts (same binary) are silently
re-attached without any user message.

The work needed is:
1. Extend the restart notification trigger to cover all restarts, not only binary rebuilds.
2. Include the per-user session name in the notification text.
3. Optionally save sessions on SIGTERM (they are already saved on connect/disconnect, so
   this is low-risk to skip if sessions are always saved before shutdown).

---

## 1. What Is Already Persisted to Disk

### File: `~/.ai-commander/state/telegram_sessions.json`

Schema: `HashMap<i64, PersistedSession>` (key = chat_id or encoded topic key).

`PersistedSession` fields (defined in `crates/commander-telegram/src/session.rs` lines 70-88):

```
chat_id        i64           Telegram chat ID
project_path   String        Filesystem path to the project
project_name   String        Display name
tmux_session   String        tmux session name (e.g. "commander-myproject")
thread_id      Option<i32>   Forum topic thread ID (group mode)
worktree_info  Option<...>   Git worktree metadata
created_at     u64           Unix timestamp
last_activity  u64           Unix timestamp
```

This is written by `save_persisted_sessions()` and read by `load_persisted_sessions()`
in `crates/commander-telegram/src/state.rs` (lines 119-168).

### File: `~/.ai-commander/state/bot_version.json`

Holds the binary hash, last start timestamp, and a monotonic start count. Used by
`check_rebuild()` to detect whether the binary changed between starts.

### File: `~/.ai-commander/state/authorized_chats.json`

The set of `chat_id` values that have been paired with this commander instance.

### File: `~/.ai-commander/state/group_configs.json`

Topic-to-session mappings for forum supergroups.

### Current state of the files at time of research

`telegram_sessions.json` = `{}` (no active session at time of investigation).
`bot_version.json` = `{ start_count: 1 }` (first ever start, so is_first_start=true).

---

## 2. UserSession vs PersistedSession — What Is In-Memory Only

`UserSession` (the live struct, `session.rs` lines 10-56) carries many runtime-only fields
that are not persisted and do not need to be:

| Field                              | Persisted? | Notes                                      |
|------------------------------------|------------|--------------------------------------------|
| chat_id                            | YES        |                                            |
| project_path                       | YES        |                                            |
| project_name                       | YES        |                                            |
| tmux_session                       | YES        |                                            |
| thread_id                          | YES        |                                            |
| worktree_info                      | YES        |                                            |
| response_buffer                    | NO         | In-flight output buffer; ephemeral         |
| last_output_time / last_output     | NO         | Polling state; reset on restore            |
| pending_query / is_waiting         | NO         | Mid-response state; ephemeral              |
| pending_message_id                 | NO         | Telegram MessageId; ephemeral              |
| is_summarizing / counters          | NO         | Progress state; ephemeral                  |
| daemon_session_id                  | NO         | Daemon IPC handle; re-established on use   |
| send_time                          | NO         | Latency measurement; ephemeral             |
| adapter_type                       | NO         | Currently always "claude-code" on restore  |
| original_message_id                | NO         | Per-response; ephemeral                    |
| is_private_chat                    | NO         | Re-detected from message context           |
| at_session_name / stale_poll_count | NO         | Ephemeral routing/health state             |

**Minimum needed to reconnect a session**: `chat_id`, `tmux_session`, `project_name`,
`project_path`, `thread_id`. All five are already persisted in `PersistedSession`.

---

## 3. Bot Startup Sequence

`main()` in `crates/commander-telegram/src/main.rs` calls `bot.start_polling()`.

`start_polling()` in `crates/commander-telegram/src/bot.rs` (lines 125-270) executes in
this order:

```
1. check_rebuild()            → (is_rebuild, is_first_start, start_count)
2. state.load_sessions()      → (restored_count, total_count)
3. if is_rebuild && !is_first_start:
       spawn send_rebuild_notification(bot, state, restored_count, total_count)
4. init_orchestrator()        (agents feature)
5. bot.get_me() / set_bot_info()
6. spawn poll_output_loop()
7. spawn poll_notifications_loop()
8. Dispatcher::builder(...).dispatch().await   ← blocks until Ctrl-C
```

**The hook point for sending reconnect notifications is step 3.** The notification is
already sent there; the problem is the condition (`is_rebuild && !is_first_start`) excludes
plain restarts of the same binary.

---

## 4. How State Is Constructed

`TelegramState::new()` in `state.rs` (lines 281-322):
- Calls `load_authorized_chats()` and `load_group_configs()` from disk synchronously.
- Sessions are NOT loaded here; they are loaded asynchronously by `load_sessions()` after
  construction (step 2 above).
- Sessions map is `RwLock<HashMap<i64, UserSession>>`.

`load_sessions()` (state.rs lines 2016-2067):
- Reads `telegram_sessions.json`.
- For each entry: validates age (<24h) and that the tmux session still exists.
- On success, calls `PersistedSession::restore_to_user_session()` and inserts into the map.
- Returns `(restored_count, total_count)`.

---

## 5. When Sessions Are Saved

`save_sessions()` is called in two places:
- `connect()` — after successfully inserting a new session (state.rs line 741).
- `disconnect()` — after removing a session (state.rs line 795).

Sessions are saved immediately on state changes, not only on shutdown. This means that for
normal stop/restart cycles the session file is already current before SIGTERM arrives.
There is no need for a SIGTERM hook purely to flush session state (though one would be
needed if that assumption ever changes).

---

## 6. Graceful Shutdown — Current State

### Dispatcher shutdown

`Dispatcher::builder(...).enable_ctrlc_handler().build().dispatch().await` — teloxide
installs a Ctrl-C handler that drains the dispatcher cleanly. No SIGTERM hook is installed
anywhere in the Rust code.

### Daemon stop (daemon.rs lines 141-181)

`stop()` sends SIGTERM via `kill <pid>`, then polls for exit for 5 seconds, then sends
SIGKILL. The Rust process receives SIGTERM and the tokio runtime shuts down (the default
tokio SIGTERM behaviour is to terminate the runtime without running any cleanup). There is
no `tokio::signal::unix::signal(SignalKind::terminate())` handler.

**Gap**: If the bot is stopped via SIGTERM (the normal path through `stop()` / the shell),
no async cleanup code runs. However, because `save_sessions()` is called at connect and
disconnect time (not at shutdown), this is not a correctness problem for session persistence
— the file is already up to date.

**If a SIGTERM hook is desired** (e.g., to do a last-second save of in-flight state),
the place to add it is in `start_polling()` after building the dispatcher, using
`tokio::signal::unix::signal(SignalKind::terminate())` and `tokio::select!`.

---

## 7. The Rebuild-vs-Restart Distinction

`check_rebuild()` in `version.rs` (lines 177-191):
- Loads `bot_version.json` from disk.
- On the very first start (`start_count == 1`, `is_first_start = true`): returns
  `(false, true, 1)` — no notification.
- On subsequent starts: calls `version.update()` which hashes the current binary
  (size + mtime of the executable on disk). Returns `is_rebuild = true` only if the
  hash changed.

**Consequence**: A plain `scripts/services.sh restart` that does SIGTERM + re-exec of
the same binary returns `is_rebuild = false`, so `send_rebuild_notification` is never
called, and users get no message even though their session was silently re-attached.

---

## 8. The Existing Notification Message

`send_rebuild_notification()` in `bot.rs` (lines 793-836) sends to **all authorized
chat IDs** (not per-user, not per-session). The message texts are:

- 0 sessions: "Bot rebuilt and restarted. No active sessions to restore."
- all restored: "Bot rebuilt and restarted. Successfully restored N session(s)."
- partial: "Bot rebuilt and restarted. Restored N of M session(s). ..."
- none restored: "Bot rebuilt and restarted. Could not restore N session(s) ..."

Missing: the specific session name each user was reconnected to. A user with multiple
chat_ids would receive one broadcast message rather than a per-user message naming their
session.

---

## 9. Recommended Implementation Plan

### Gap 1: Trigger notification on all restarts, not only binary rebuilds

In `bot.rs` `start_polling()`, change the condition:

```rust
// Before:
if is_rebuild && !is_first_start {

// After:
if !is_first_start {
```

`is_first_start` is already false for any restart after the first. This single-character
change makes the notification fire on every restart (plain or rebuild).

If finer control is wanted (suppress notification for rapid health-check restarts), add
a minimum uptime check using `BotVersion::age_seconds()` before triggering.

### Gap 2: Send per-user, per-session notification

Replace the current broadcast in `send_rebuild_notification` with a per-session loop.
After `load_sessions()` completes, the state already has the restored sessions in memory.
Iterate over them to get `(chat_id, project_name, thread_id)` per user:

```rust
// Pseudocode — to be placed in bot.rs after load_sessions()
if !is_first_start && restored_count > 0 {
    let bot = self.bot.clone();
    let state = Arc::clone(&self.state);
    tokio::spawn(async move {
        let sessions = state.get_restored_session_summaries().await;
        // sessions: Vec<(chat_id: i64, project_name: String, thread_id: Option<i32>)>
        for (chat_id, project_name, thread_id) in sessions {
            let msg = format!(
                "Bot restarted — reconnected to session <b>{}</b>",
                teloxide::utils::html::escape(&project_name)
            );
            let mut req = bot.send_message(ChatId(chat_id), &msg)
                .parse_mode(ParseMode::Html);
            if let Some(tid) = thread_id {
                req = req.message_thread_id(ThreadId(MessageId(tid)));
            }
            if let Err(e) = req.await {
                warn!(chat_id=%chat_id, error=%e, "Failed to send restart notification");
            }
        }
    });
}
```

A helper `get_restored_session_summaries()` on `TelegramState` would read the sessions
map and return `Vec<(i64, String, Option<i32>)>`.

### Gap 3: Persist session name in notification (already available)

`project_name` is already in `PersistedSession` and in the restored `UserSession`. No
schema change is needed.

### Gap 4 (optional): SIGTERM hook for in-flight session save

Only needed if sessions can be active without ever having been saved (e.g., after
reconnecting to an unregistered tmux session — see the fallback path in `connect()` at
state.rs line 746-777 which does NOT call `save_sessions()`). To fix that gap
independently, add `self.save_sessions().await` after the unregistered session branch
in `connect()`.

---

## 10. File Format Assessment

The current format (`HashMap<i64, PersistedSession>` serialised as JSON) is appropriate
and requires no change. The `PersistedSession` struct already has all required fields.
The key `i64` encodes either a plain `chat_id` or a composite `(chat_id, thread_id)` key
for topic sessions (encoding strategy inherited from the live sessions map).

---

## 11. Summary Table

| Question                              | Answer                                                                           |
|---------------------------------------|----------------------------------------------------------------------------------|
| Persistence file                      | `~/.ai-commander/state/telegram_sessions.json`                                   |
| File format                           | `HashMap<i64, PersistedSession>` as JSON                                         |
| When saved                            | On `connect()` and `disconnect()` (event-driven, not on shutdown)                |
| When loaded                           | `start_polling()` step 2, before polling begins                                  |
| Session restored in-memory?           | Yes — `load_sessions()` puts `UserSession` into the sessions map                 |
| Notification sent?                    | Only when binary hash changed (rebuild), not on plain restart                    |
| Per-user or broadcast?                | Broadcast to all authorized chats; no per-session naming                         |
| SIGTERM hook exists?                  | No                                                                                |
| Minimum persist fields for reconnect  | chat_id, tmux_session, project_name, project_path, thread_id (all present)      |
| Code changes required                 | ~30-50 lines in bot.rs + one helper method on TelegramState                     |

---

## Key File Locations

- `crates/commander-telegram/src/session.rs` — `PersistedSession`, `UserSession`
- `crates/commander-telegram/src/state.rs` — `save_sessions()`, `load_sessions()`
- `crates/commander-telegram/src/bot.rs` — `start_polling()`, `send_rebuild_notification()`
- `crates/commander-telegram/src/version.rs` — `check_rebuild()`, `BotVersion`
- `crates/commander-telegram/src/daemon.rs` — `stop()`, `restart()` (SIGTERM path)
- `~/.ai-commander/state/telegram_sessions.json` — persisted session data
- `~/.ai-commander/state/bot_version.json` — start count and binary hash
