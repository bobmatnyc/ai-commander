# Bug Investigation: "Could not route to @izzie: JSON error: EOF while parsing a value at line 1 column 0"

**Date:** 2026-03-25
**Status:** Root cause identified

---

## Summary

A Unicode character boundary panic in `user_agent/mod.rs` causes the daemon's Tokio task to crash silently mid-request. The crash drops the Unix socket write half without sending a response. The IPC client reads zero bytes (EOF), then `serde_json::from_str("")` fails with `"EOF while parsing a value at line 1 column 0"`, which surfaces to the user as the routing error.

---

## Error Location

**Where the error message is generated:**
- File: `crates/commander-telegram/src/handlers.rs`, line 1417
- Code: `format!("❌ Could not route to @{}: {}", alias, e)`
- Triggered when `state.send_to_named_session()` returns `Err`

**Where the JSON parse error originates:**
- File: `crates/commander-telegram/src/ipc_client.rs`, line 129
- Code: `let response: JsonRpcResponse = serde_json::from_str(response_line.trim())?`
- `response_line` is empty because the daemon closed the socket without writing anything

---

## Root Cause: Panic on Non-ASCII String Slice

**Panic location:** `crates/commander-agent/src/user_agent/mod.rs`, line 302

```rust
async fn process(&mut self, message: &str, context: &AgentContext) -> Result<AgentResponse> {
    info!(
        "Processing message: {}...",
        &message[..message.len().min(50)]  // LINE 302 - PANICS HERE
    );
```

**Daemon log confirms the panic:**
```
thread 'tokio-runtime-worker' panicked at crates/commander-agent/src/user_agent/mod.rs:302:21:
byte index 50 is not a char boundary; it is inside '"' (bytes 49..52) of
`a bit more character in the end of day message.  "End of day."  Should dump up
day, list any tasks, and prepare me for the next day.  And if nothing new happened
between mailbox cleanups, then just update the sane status message with a new time.`
```

The message contains Unicode curly-quote characters (`"` = U+201C, encoded as 3 bytes: `E2 80 9C`). The byte at position 49 is the first byte of a 3-byte UTF-8 sequence. Slicing `&message[..50]` splits in the middle of that sequence, which is undefined behavior in Rust strings — Rust panics rather than produce invalid UTF-8.

---

## Failure Chain

```
1. User sends: @izzie a bit more character... "End of day." ...
                                              ^
                              curly-quote at byte 49-51

2. send_to_named_session() calls daemon.session_create() — succeeds
   [ipc_client log: session.create response = {"session_id":"b70ef120..."}]

3. send_to_named_session() calls daemon.session_send(session_id, message)
   [new Unix socket connection opened]

4. Daemon's handle_connection() reads the request line — succeeds
   Calls dispatch_request() -> send_to_session() -> process_user_input(message)

5. process_user_input() calls user_agent.process(message, context)
   Inside process(), line 302: &message[..message.len().min(50)] PANICS
   because byte 50 falls inside a 3-byte UTF-8 sequence for '"'

6. Tokio catches the panic at the spawned task boundary (server.rs line 69-73)
   The task is aborted. The writer_stream (socket write half) is dropped.
   No response bytes are written to the client.

7. ipc_client.rs read_line() returns Ok(0) — EOF with empty buffer
   response_line = ""
   serde_json::from_str("") -> Err(EOF while parsing a value at line 1 column 0)

8. This JsonError propagates up via ? through:
   session_send() -> send_to_named_session() -> handlers.rs Err branch

9. Bot sends: "❌ Could not route to @izzie: JSON error: EOF while parsing a value at line 1 column 0"
```

---

## Timeline from Logs

```
04:23:46 - @izzie message received (contains curly quotes)
04:23:47.130 - IPC session.create -> success (session_id assigned)
04:23:47.131 - Session registered with daemon
04:23:47.152 - IPC session.send -> response= [EMPTY - panic occurred]
              (only 22ms elapsed: no API call was ever made)
04:25:46 - User forwards the error text to aic-2 session

04:26:21 - User retries via /connect_izzie deep link
04:26:21.017 - session.create -> success (new session_id)
04:26:47.161 - session.send -> response= [EMPTY again - same panic]
04:26:47.390 - ERROR logged: JSON error: EOF while parsing a value at line 1 column 0
```

The error reproduces every time a message containing a multi-byte UTF-8 character (e.g., Unicode quotes, em-dashes, emoji) falls across byte position 50.

---

## The Fix

**File:** `crates/commander-agent/src/user_agent/mod.rs`, line 302

Replace the unsafe byte-based slice with a char-boundary-safe truncation:

```rust
// BEFORE (panics on multibyte chars at byte position 50):
&message[..message.len().min(50)]

// AFTER (safe: truncate at a valid char boundary):
let preview: String = message.chars().take(50).collect();
info!("Processing message: {}...", preview);
```

Or more concisely using a helper that already exists in the ecosystem:

```rust
info!(
    "Processing message: {}...",
    message.char_indices()
        .nth(50)
        .map(|(i, _)| &message[..i])
        .unwrap_or(message)
);
```

**The same pattern likely exists elsewhere.** Search for `[..` with `.min(50)` or similar byte-length truncations on `&str` values throughout the codebase:

```
grep -rn "\[\.\..*\.min(" crates/
```

Known second occurrence: `crates/commander-agent/src/session_agent/mod.rs:406` — same `&message[..message.len().min(50)]` pattern.

---

## Secondary Issue: Silent Panic Propagation

When the daemon task panics, the IPC client receives EOF with no error detail. The error message shown to the user (`JSON error: EOF while parsing a value`) is confusing and gives no hint of the actual cause.

**Recommended improvement in `ipc_client.rs`:** Distinguish EOF from a real parse error:

```rust
// After read_line:
if response_line.is_empty() {
    return Err(TelegramError::SessionError(
        "Daemon closed connection without responding (possible daemon crash)".to_string()
    ));
}
let response: JsonRpcResponse = serde_json::from_str(response_line.trim())?;
```

This would produce a more actionable error message and make future daemon-side panics easier to diagnose.

---

## Files Involved

| File | Role |
|------|------|
| `crates/commander-agent/src/user_agent/mod.rs:302` | **Bug location** — panics on non-ASCII byte slice |
| `crates/commander-agent/src/session_agent/mod.rs:406` | Same pattern, same risk |
| `crates/commander-telegram/src/ipc_client.rs:129` | Sees empty response, produces JSON parse error |
| `crates/commander-telegram/src/handlers.rs:1417` | Displays the error to the user |
| `crates/commander-daemon/src/ipc/server.rs:69-73` | Tokio task boundary silently swallows the panic |

---

## No Config or Data Files Involved

The error has nothing to do with:
- Empty config files
- Missing `users.json` / `routing.json`
- Empty API responses from external services
- tmux output

The `pairings.json` state file (`~/.ai-commander/state/pairings.json`) is present and valid (`{"entries":{},"version":1}`). The routing system worked correctly — the session was found and the daemon was reached. The failure is purely a Rust string indexing panic triggered by the content of the message itself.
