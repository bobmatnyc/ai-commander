# Tmux PATH Bug and Session Logging Architecture

**Date:** 2026-03-03
**Scope:** commander-telegram, commander-tmux, launchd service configuration

---

## 1. Tmux Session Detection Bug

### Root Cause

**The launchd service that runs `commander-telegram` has a minimal PATH that does not include `/opt/homebrew/bin`, where tmux lives.**

Launchd's default PATH for user agents is:
```
/usr/bin:/bin:/usr/sbin:/sbin
```

tmux on this machine is installed at:
```
/opt/homebrew/bin/tmux -> ../Cellar/tmux/3.6a/bin/tmux
```

This path is not in launchd's default environment. When the telegram service starts under launchd, `TmuxOrchestrator::new()` runs `which tmux`, which returns empty output, causing tmux to be initialized as `None` in `TelegramState`.

### Exact Failure Chain

**File:** `crates/commander-tmux/src/orchestrator.rs`, lines 36-47
```rust
fn find_tmux() -> Result<String> {
    let output = Command::new("which").arg("tmux").output()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            return Err(TmuxError::NotFound);  // <-- fires when which fails
        }
        Ok(path)
    } else {
        Err(TmuxError::NotFound)  // <-- fires when which returns non-zero
    }
}
```

`which tmux` fails (non-zero exit or empty output) because `/opt/homebrew/bin` is not in launchd's PATH.

**File:** `crates/commander-telegram/src/state.rs`, lines 272-277
```rust
let tmux = TmuxOrchestrator::new().ok();  // Ok(None) when not found
// ...
if tmux.is_none() {
    warn!("tmux not available - project connections will not work");
}
```

`TmuxOrchestrator::new()` fails, `.ok()` turns the error into `None`, and the state initializes with `self.tmux = None`.

**File:** `crates/commander-telegram/src/state.rs`, lines 1277-1291
```rust
pub fn list_tmux_sessions(&self) -> Vec<(String, bool)> {
    let Some(tmux) = &self.tmux else {
        return Vec::new();   // <-- returns empty list silently
    };
    tmux.list_sessions()
        .map(|sessions| { ... })
        .unwrap_or_default()
}
```

**File:** `crates/commander-telegram/src/handlers.rs`, lines 1078-1083
```rust
let sessions = state.list_tmux_sessions_with_status();

if sessions.is_empty() {
    bot.send_message(msg.chat.id, "No tmux sessions found.")  // <-- user sees this
        .await?;
    return Ok(());
}
```

The user sees "No tmux sessions found." not because tmux has no sessions, but because the tmux binary was never found at startup.

### Installed Plist (Confirmed)

`~/Library/LaunchAgents/ai.commander.telegram.plist` only sets `HOME`:
```xml
<key>EnvironmentVariables</key>
<dict>
    <key>HOME</key>
    <string>/Users/masa</string>
</dict>
```

No `PATH` is set. The `.env` config at `~/.ai-commander/config/.env` also has no `PATH` variable — it only contains `TELEGRAM_BOT_TOKEN` and `OPENROUTER_API_KEY`.

### Fix Options

**Option A (Recommended): Add PATH to the plist template**

Edit `scripts/launchd/ai.commander.telegram.plist` and `scripts/launchd/ai.commander.daemon.plist`:
```xml
<key>EnvironmentVariables</key>
<dict>
    <key>HOME</key>
    <string>REAL_HOME</string>
    <key>PATH</key>
    <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
</dict>
```

Then re-run `~/.ai-commander/services.sh launchd-install` to regenerate and reload the plists.

**Option B: Add PATH to `~/.ai-commander/config/.env`**

The start scripts already source this file:
```bash
# In start-telegram.sh:
source "$CONFIG_ENV"
```

Adding `PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin` to `~/.ai-commander/config/.env` would work for this user. However, it is fragile — a different machine might have tmux elsewhere.

**Option C: Use absolute path in `find_tmux()`**

Instead of calling `which`, check known tmux locations directly:
```rust
fn find_tmux() -> Result<String> {
    let candidates = [
        "/opt/homebrew/bin/tmux",
        "/usr/local/bin/tmux",
        "/usr/bin/tmux",
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    // Fall back to which
    let output = Command::new("which").arg("tmux").output()?;
    // ...
}
```

Option A is the correct fix since it makes the service environment explicit and correct. Option C is a good additional hardening.

---

## 2. Chat Session Logging Architecture

### Message Flow: Telegram to Claude and Back

The full flow for a user message is:

```
Telegram API
    |
    v (teloxide long-polling or webhook)
bot.rs:238 — Update::filter_message() endpoint
    |
    v
handlers.rs:1232 — handle_message(bot, msg, state)
    |
    +-- authorization check (state.is_authorized)
    |
    v (if connected)
handlers.rs:1321 — state.send_message(chat_id, &text, Some(msg.id))
    |
    v
state.rs:1074 — TelegramState::send_message()
    |
    +-- captures current tmux output as baseline (last_output)
    |
    +-- if daemon_client available:
    |       state.rs:1093 — daemon.session_send(session_id, message)
    |       (JSON-RPC over Unix socket to commander-daemon)
    |
    +-- else (tmux-only mode):
    |       state.rs:1095 — tmux.send_line(tmux_session, None, message)
    |       (runs: tmux send-keys -t <session> -l <message> + Enter)
    |
    v
state.rs:1100 — session.start_response_collection(message, last_output, message_id)
    (sets session.is_waiting = true)

-- Meanwhile, polling loop runs every POLL_INTERVAL_MS --

bot.rs:306 — poll_output_loop() [separate tokio task]
    |
    v (every tick, for each waiting session)
bot.rs:342 — state.poll_output(chat_id)
    |
    v
state.rs:1156 — TelegramState::poll_output()
    |
    +-- tmux.capture_output(tmux_session, None, Some(200))
    |   (runs: tmux capture-pane -t <session> -p -S -200)
    |
    +-- compares current_output to session.last_output
    |
    +-- if changed: adds new lines to session.response_buffer
    |
    +-- checks for completion (idle timeout, output stabilization)
    |
    v (when complete)
PollResult::Complete(response, message_id, thread_id)
    |
    v
bot.rs:413 — sends response via bot.send_message(chat_id, &response)
    (reply-threaded to original message_id)
```

### IPC Path (when daemon is running)

```
state.rs:1093 — daemon.session_send(session_id, message)
    |
    v
crates/commander-daemon/src/ipc/unix.rs — DaemonClient
    (JSON-RPC over Unix socket: ~/.ai-commander/state/daemon.sock)
    |
    v
crates/commander-daemon/src/ipc/server.rs — handle_request()
    |
    v (SessionSend method)
crates/commander-daemon/src/service.rs — DaemonServiceHandle::session_send()
    |
    v
crates/commander-daemon/src/sessions.rs — SessionManager
    |
    v (depending on adapter)
commander-runtime/src/runtime.rs — ClaudeRuntime
    (spawns Claude CLI in tmux session, sends input)
```

### Existing Logging (What's There Now)

Searching all `info!`, `debug!`, `warn!` calls in the message path:

**handlers.rs:1323** — only logs "Message sent to project" at `debug!` level, no message content at `info!`:
```rust
debug!(chat_id = %msg.chat.id, message = %text, "Message sent to project");
```

**handlers.rs:237** (bot.rs) — logs "Regular message received" with message text at `info!` level:
```rust
info!(chat_id = %msg.chat.id, text = ?msg.text(), "Regular message received");
```

**state.rs:1102-1107** — logs at `debug!` level:
```rust
debug!(
    chat_id = %chat_id.0,
    project = %session.project_name,
    message = %message,
    "Message sent to project"
);
```

**bot.rs:465-468** — logs "Response sent to user" at `info!` level but NOT the response content:
```rust
info!(chat_id = %chat_id.0, thread_id = ?target_thread_id, "Response sent to user");
```

**Summary of what is currently NOT logged:**
- User message content at `info!` level (only at `debug!`)
- Assistant response content at any log level
- Session ID at the send point
- Token counts or latency
- Timestamps (implicitly captured by the logging framework, but not in structured fields)

### Best Place to Insert Logging

**For user messages:** `state.rs:1074` in `TelegramState::send_message()`, just before returning `Ok(())`. This is the single chokepoint where every user message passes through, whether via daemon or direct tmux.

**For assistant responses:** `bot.rs:413` in `poll_output_loop()`, in the `PollResult::Complete` arm just before sending. This is where the complete response text is available as `response`, along with `chat_id`, `message_id`, and `session_key`.

---

## 3. Recommended Log Format for Evals

### User Message Log Entry

Insert at `state.rs:1100`, after `session.start_response_collection(...)`:

```rust
info!(
    event = "user_message",
    chat_id = %chat_id.0,
    session_id = %session.tmux_session,
    project = %session.project_name,
    message_len = message.len(),
    message = %message,
    via_daemon = session.daemon_session_id.is_some(),
    "User message forwarded to session"
);
```

### Assistant Response Log Entry

Insert at `bot.rs:461`, after `req.await` succeeds:

```rust
info!(
    event = "assistant_response",
    chat_id = %chat_id.0,
    session_key = %session_key,
    response_len = response.len(),
    response = %response,
    has_options = detected_options.is_some(),
    "Assistant response delivered to user"
);
```

### Recommended Structured Fields

For eval/replay purposes the most useful fields are:

| Field | Source | Why |
|-------|--------|-----|
| `timestamp` | Implicit in log framework | When did this happen |
| `session_id` | `session.tmux_session` | Which tmux session was active |
| `chat_id` | `chat_id.0` (Telegram chat ID) | Which user/conversation |
| `user_message` | `message` in `send_message()` | Input to Claude |
| `assistant_response` | `response` in `PollResult::Complete` | Output from Claude |
| `project_name` | `session.project_name` | Which project was targeted |
| `via_daemon` | `session.daemon_session_id.is_some()` | Whether IPC or direct tmux |
| `response_len` | `response.len()` | Rough token proxy |
| `message_id` | `msg.id` from Telegram | For correlating request/response pairs |
| `latency_ms` | Computed between `send_message` and `PollResult::Complete` | Response time |

Model and token counts are not available in the current architecture because the tmux polling approach captures raw terminal output — it doesn't intercept Claude API calls directly. To get token counts and model name, you would need to parse them from the terminal output (Claude CLI prints them at the end of a response) or instrument the `commander-runtime` layer that spawns Claude.

The `session.tmux_session` string (e.g. `commander-myproject`) combined with `chat_id` gives you a unique conversation identifier. A `message_id` field from the Telegram `msg.id` allows correlating user turn with assistant turn if you log both with the same `message_id`.

---

## Summary

| Issue | File | Line | Finding |
|-------|------|------|---------|
| tmux not found | `scripts/launchd/ai.commander.telegram.plist` | — | No PATH set; only HOME |
| `which tmux` fails | `crates/commander-tmux/src/orchestrator.rs` | 37 | Uses `which` which requires `/opt/homebrew/bin` in PATH |
| Silent fallback to None | `crates/commander-telegram/src/state.rs` | 272 | `TmuxOrchestrator::new().ok()` — failure becomes None |
| Empty list returned | `crates/commander-telegram/src/state.rs` | 1277 | `let Some(tmux) = &self.tmux else { return Vec::new(); }` |
| "No tmux sessions found" | `crates/commander-telegram/src/handlers.rs` | 1080 | Message shown when list is empty, regardless of reason |
| Message logging gap | `crates/commander-telegram/src/state.rs` | 1102 | `debug!` only, no `info!` with message content |
| Response logging gap | `crates/commander-telegram/src/bot.rs` | 465 | `info!` says "Response sent" but not response content |
