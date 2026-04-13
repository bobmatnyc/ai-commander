# Root Cause: Non-Stop Typing Indicator

**Date:** 2026-03-24
**Status:** Confirmed bug - session stuck in `is_waiting = true` indefinitely

---

## Summary

The Telegram bot sends a `ChatAction::Typing` indicator on every poll cycle (every 500ms) for any session where `is_waiting = true`. When a session gets stuck in the waiting state — meaning `reset_response_state()` is never called — the bot sends typing indicators to that chat forever. The log currently contains tens of thousands of these failures, running continuously since at least 2026-03-12.

---

## Where Typing Is Sent

File: `crates/commander-telegram/src/bot.rs`, lines 429-439

```rust
// Refresh typing indicator to show processing is ongoing
if let Some(tid) = thread_id {
    if let Err(e) = bot.send_chat_action(chat_id, ChatAction::Typing)
        .message_thread_id(tid)
        .await
    { ... }
} else if let Err(e) = bot.send_chat_action(chat_id, ChatAction::Typing).await {
    ...
}
```

This runs inside `poll_output_loop` which ticks every `POLL_INTERVAL_MS = 500ms` for every session returned by `get_waiting_sessions()`.

`get_waiting_sessions()` in `state.rs` line 1653-1660 simply returns all sessions where `session.is_waiting == true` with no timeout or expiry.

---

## What Causes the Infinite Loop

The lifecycle is:

1. User sends a message -> `send_message()` calls `session.start_response_collection()` which sets `is_waiting = true`.
2. `poll_output_loop` fires every 500ms and sends `ChatAction::Typing` for every waiting session.
3. Poll calls `poll_output()` or `poll_topic_output()`, which detects completion via `is_idle(1500ms) && has_prompt` (i.e., Claude's prompt character is visible in tmux output AND no new output for 1.5s).
4. On completion: `session.reset_response_state()` is called, which sets `is_waiting = false`. This is the ONLY way typing stops.

**The stuck condition:** If `is_idle && has_prompt` never becomes true, the session stays in `is_waiting = true` forever. This happens when:

- The tmux session has exited, crashed, or been detached — `capture_output` returns either empty or stale output without the Claude ready prompt.
- `is_claude_ready()` returns false because the terminal output does not contain the expected prompt characters (`❯`, `╭─`, `╰─`, `bypass permissions`, `│ ❯`, `>`).
- A new message was sent to a session that was connected but the underlying tmux session no longer exists or is not running Claude.
- The session's adapter was classified as `"claude-code"` but is actually running something else that never shows the expected prompt.

There is no timeout. A session that never completes will poll every 500ms and send `ChatAction::Typing` until the bot process is restarted.

---

## Evidence From Logs

The log shows the typing indicator failing with Telegram rate-limit errors starting as far back as 2026-03-12 and continuing until now (2026-03-24), without interruption:

```
[2026-03-12T13:06:20] WARN Failed to send typing indicator chat_id=5235493571 error=Retry after 10s
[2026-03-24T19:39:54] WARN Failed to send typing indicator chat_id=5235493571 error=Retry after 3s
```

This is a single chat_id (`5235493571`) that has been in `is_waiting = true` for approximately 12+ days.

No `poll_output: idle/prompt check` DEBUG lines appear in the log (those would have appeared if `is_idle && has_prompt` ever became true). The session never completed.

There is also secondary evidence from the user's own message at 2026-03-24T05:12:53:
```
Regular message received text=Some("@aic I'm seeing lots of typing but not responses.")
```

---

## The Two Root Causes

### Root Cause 1: No timeout on `is_waiting`

`get_waiting_sessions()` has no time limit. A session that has been waiting for hours or days will continue to be included. There is no maximum wait duration anywhere in the codebase.

### Root Cause 2: `is_claude_ready()` detection failure

The completion gate requires BOTH:
- `session.is_idle(1500)` — 1.5 seconds of no new tmux output
- `is_claude_ready(&current_output)` — specific Claude prompt patterns in the last 10 lines of tmux output

If the tmux session is gone, the `capture_output` call likely returns the same stale last-known output on every poll. `is_idle` will trigger (no new output), but if that stale output does not contain the Claude prompt pattern, `has_prompt` stays false and the completion branch is never reached.

---

## Expected vs Actual Behavior

| | Expected | Actual |
|---|---|---|
| Session completes | `reset_response_state()` called, `is_waiting = false`, typing stops | Never happens if prompt detection fails |
| Session times out | After N seconds/minutes with no completion, session resets | No timeout exists |
| Typing indicator | Sent only while actually waiting for Claude to respond | Sent indefinitely for stuck sessions |
| Rate limit response | Not applicable (typing should complete before rate limiting) | Bot gets rate-limited because typing is sent 2x/second |

---

## Fix Recommendations

### Fix 1 (Critical): Add a maximum wait timeout

In `start_response_collection()` or `get_waiting_sessions()`, track how long a session has been waiting. If waiting > N minutes (e.g., 5 minutes), call `reset_response_state()` with an error response.

In `session.rs`:
```rust
pub wait_start: Option<Instant>,  // add to UserSession
```

In `get_waiting_sessions()` or `poll_output()`, add:
```rust
const MAX_WAIT_SECS: u64 = 300;  // 5 minutes
if let Some(start) = session.wait_start {
    if start.elapsed().as_secs() > MAX_WAIT_SECS {
        session.reset_response_state();
        return Ok(PollResult::Complete(
            "Request timed out. Claude did not respond within 5 minutes.".to_string(),
            session.pending_message_id,
            session.thread_id,
        ));
    }
}
```

`send_time` already exists on `UserSession` and tracks the same thing — it can be reused.

### Fix 2: Detect dead tmux sessions

Before polling, verify the tmux session still exists. If `capture_output` errors or returns empty for multiple consecutive polls, complete the session with an error.

### Fix 3 (Immediate): Restart the bot

The currently stuck session for chat_id `5235493571` will not recover until the bot is restarted, since `is_waiting` is in-memory state and there is no recovery path for stuck sessions on an already-running bot.

```bash
~/.ai-commander/services.sh restart
```

---

## Files Involved

- `crates/commander-telegram/src/bot.rs` — `poll_output_loop()` at line 398, typing sent at lines 430-439
- `crates/commander-telegram/src/state.rs` — `get_waiting_sessions()` at line 1653, `poll_output()` at line 1479, `poll_topic_output()` at line 1053, `reset_response_state()` called only from inside completion branches
- `crates/commander-telegram/src/session.rs` — `UserSession.is_waiting`, `reset_response_state()` at line 216
- `crates/commander-core/src/output_filter.rs` — `is_claude_ready()` at line 175, `is_mpm_ready()` at line 246
- `~/.ai-commander/logs/telegram.log` — contains the continuous failure log
