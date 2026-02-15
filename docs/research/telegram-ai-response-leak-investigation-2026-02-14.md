# Telegram AI Response Leak Investigation

**Date:** 2026-02-14
**Status:** Research Complete

## Executive Summary

Investigation of three related bugs in AI Commander's Telegram integration:
1. Full AI responses leaking to Telegram chat
2. "unknown" project name appearing in connection messages
3. Session link button for input-waiting state (feature request analysis)

## Bug #1: AI Response Leak to Telegram

### Symptoms
User reported full AI responses appearing in Telegram chat:
- "Can you try the chat again?..."
- "I need more context..."

### Root Cause

The response flow from Claude session to Telegram is:

```
User Message -> send_message() -> tmux -> Claude responds
                                          |
                                          v
poll_output() <- captures new lines every 500ms
     |
     v
is_idle && has_prompt && !response_buffer.is_empty()
     |
     +---> needs_summarization?
              |
              +-- YES: summarize_with_fallback(query, raw_response)
              |              |
              |              +-- API success -> return summary
              |              +-- API failure -> clean_response(raw_response) [FALLBACK]
              |
              +-- NO: clean_response(raw_response)
     |
     v
PollResult::Complete(response, ...) -> bot.send_message()
```

**The bug occurs when:**
1. `OPENROUTER_API_KEY` is not set, OR
2. OpenRouter API call fails (timeout, rate limit, etc.)

In these cases, `summarize_with_fallback()` falls back to `clean_response()`, which only filters UI noise (spinners, box drawing, status bars) but passes through Claude's actual conversational responses unchanged.

### Key Code Locations

**Response Collection:**
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs` lines 1057-1139
- `poll_output()` method collects lines from tmux and returns `PollResult::Complete`

**Summarization:**
- `/Users/masa/Projects/ai-commander/crates/commander-core/src/summarizer.rs` lines 177-191
- `summarize_with_fallback()` - attempts API summarization, falls back to `clean_response()`

**Response Filtering:**
- `/Users/masa/Projects/ai-commander/crates/commander-core/src/output_filter.rs` lines 376-395
- `clean_response()` - filters UI noise only, not conversational content

### Recommended Fix

**Option A: Improve fallback behavior**
When summarization fails, instead of passing raw Claude responses, provide a brief status message like:
```
"Response received ({N} lines). Summarization unavailable."
```

**Option B: Add content detection**
Detect when response looks like Claude's conversational output (starts with "Can you", "I need", etc.) and either:
- Truncate to first sentence
- Replace with generic "Claude responded"
- Prompt user to check session directly

**Option C: Require summarization**
Block response delivery when `OPENROUTER_API_KEY` is not set, requiring users to configure it for Telegram integration.

---

## Bug #2: "unknown" Project Name in Connection Message

### Symptom
Connection message shows: "interact with unknown" instead of project name

### Root Cause

When connecting to an unregistered tmux session (one not in Commander's project registry), the `connect()` method returns `tool_id = "unknown"`:

**Location:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs` lines 741-776

```rust
// Fallback to tmux session lookup (unregistered sessions)
for tmux_session_name in &session_candidates {
    if tmux.session_exists(tmux_session_name) {
        // ...
        let session = UserSession::with_thread_id(
            chat_id,
            "unknown".to_string(),  // <- project_path is "unknown"
            display_name.clone(),
            tmux_session_name.clone(),
            thread_id,
        );
        // ...
        return Ok((display_name, "unknown".to_string()));  // <- tool_id is "unknown"
    }
}
```

The `tool_id` "unknown" is then passed to `adapter_display_name()`:

**Location:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs` lines 460-466

```rust
fn adapter_display_name(tool_id: &str) -> &str {
    match tool_id {
        "claude-code" | "cc" => "Claude Code",
        "mpm" => "Claude MPM",
        "aider" => "Aider",
        _ => tool_id,  // <- Falls through, returns "unknown"
    }
}
```

### Recommended Fix

**Option A: Infer adapter from session**
Instead of returning "unknown", attempt to detect the adapter type from the session's screen content using `detect_adapter()` from output_filter.rs.

**Option B: Generic fallback message**
Add a case for "unknown" in `adapter_display_name()`:
```rust
"unknown" => "this session",
```
Result: "interact with this session"

**Option C: Suppress adapter name for unregistered sessions**
When `tool_id == "unknown"`, use a different message format that doesn't mention the adapter.

---

## Feature Request: Session Link Button for Input-Waiting State

### Current State Tracking

Input-waiting state is tracked in multiple locations:

1. **Per-session tracking:**
   - `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/session.rs` line 27
   - `UserSession.is_waiting: bool` flag

2. **Session query methods:**
   - `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs` lines 1156-1173
   - `get_waiting_sessions()` returns all sessions with `is_waiting == true`
   - `get_waiting_chat_ids()` returns just the chat IDs

3. **Notification broadcasts:**
   - `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/notifications.rs`
   - `notify_session_ready()` - when single session becomes ready
   - `notify_sessions_waiting()` - when multiple sessions are waiting

### Where to Add Link Button

**Option A: In notification messages**
Modify `notify_session_ready()` and `notify_sessions_waiting()` to include a callback button.

Note: Current notifications use `push_notification()` which stores plain text messages. Adding buttons would require:
1. Extending `Notification` struct to include optional inline keyboard data
2. Modifying `poll_notifications_loop()` in bot.rs to render buttons

**Option B: In response messages**
Add a "View Session" button to `PollResult::Complete` responses in bot.rs line 328-352.

**Option C: New /sessions command with buttons**
Create a new command that lists waiting sessions with inline buttons:
```
/sessions
Sessions waiting for input:
[izzie-33] [View]
[dci] [View]
```

### Implementation Locations

1. **Inline keyboard creation:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`
   - See existing button usage in `handle_callback()` lines 1157-1220 for callback handling pattern

2. **Session status:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`
   - Use `get_waiting_sessions()` to enumerate waiting sessions
   - Use `get_session_status()` for detailed info per session

3. **Notification enhancement:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/notifications.rs`
   - Extend `Notification` struct if adding buttons to notifications

---

## File Summary

| File | Lines | Purpose |
|------|-------|---------|
| `crates/commander-telegram/src/state.rs` | 1353 | Session state, polling, connection logic |
| `crates/commander-telegram/src/handlers.rs` | 1485 | Command handlers, message routing |
| `crates/commander-telegram/src/bot.rs` | 424 | Polling loops, response delivery |
| `crates/commander-telegram/src/session.rs` | 327 | UserSession struct |
| `crates/commander-telegram/src/notifications.rs` | 297 | Notification queue system |
| `crates/commander-core/src/output_filter.rs` | ~500 | Response filtering, UI noise detection |
| `crates/commander-core/src/summarizer.rs` | 371 | OpenRouter summarization |
| `crates/commander-core/src/notification_parser.rs` | 684 | Status parsing from screen content |
| `crates/ai-commander/src/tui/sessions.rs` | ~300 | TUI session monitoring, notification triggers |
| `crates/ai-commander/src/tui/helpers.rs` | ~100 | Preview extraction |

---

## Action Items

### Critical (Bug Fixes)
1. [ ] Fix AI response leak by improving fallback behavior in `summarize_with_fallback()`
2. [ ] Fix "unknown" adapter by adding graceful handling in `adapter_display_name()`

### Enhancement (Feature Request)
3. [ ] Design inline button UX for session links
4. [ ] Implement button in notification or response messages
5. [ ] Add callback handler for session quick-connect

---

*Research conducted by Claude Opus 4.5*
