# Bug: 5-Minute Hard Timeout Fires Despite Active Content Reception

**Date**: 2026-04-01
**Status**: Analyzed, fix approach determined
**Severity**: High (false "stalled" messages during long Claude responses)

## Summary

The 5-minute hard timeout in `poll_topic_output` and `poll_output` checks `session.send_time.elapsed()` -- the time since the user's message was sent. It does NOT account for whether content is actively arriving. For complex tasks where Claude takes >5 minutes to fully respond, the timeout fires and shows "No response received within 5 minutes. The session may have stalled." even though responses HAVE been received and shown to the user via progress/summary updates.

## Key Findings

### 1. SessionState (UserSession) Struct -- `session.rs` lines 11-61

The `UserSession` struct has these time-related fields:

| Field | Type | Purpose |
|-------|------|---------|
| `send_time` | `Option<Instant>` | Set when message is sent (line 265). Used by timeout check. Cleared in `reset_response_state()`. |
| `last_output_time` | `Option<Instant>` | Updated every time new content arrives via `add_response_lines()` (line 278). Used for idle detection (1.5s threshold). |

**There is NO `last_activity_time` or `last_activity` field on `UserSession`.** The `PersistedSession` struct has a `last_activity: u64` field (line 92), but that is a Unix timestamp for session persistence/validity -- completely unrelated to the polling loop.

### 2. The Timeout Code -- Both Functions

**`poll_topic_output`** (state.rs lines 1085-1103):
```rust
// Fix 1: 5-minute hard timeout
const MAX_WAIT_SECS: u64 = 300;
if let Some(t) = session.send_time {
    if t.elapsed().as_secs() > MAX_WAIT_SECS {
        // ... force-complete with stalled message
        session.reset_response_state();
        return Ok(PollResult::Complete(
            "No response received within 5 minutes. The session may have stalled.".to_string(),
            message_id,
            sess_thread_id,
        ));
    }
}
```

**`poll_output`** (state.rs lines 1570-1587) -- identical logic.

### 3. Where New Content is Detected

In both `poll_topic_output` and `poll_output`, new content is detected at the `current_output != session.last_output` branch (lines 1116-1169 for topic, lines 1600+ for non-topic).

When new content arrives:
- `session.add_response_lines(new_lines)` is called -- this updates `session.last_output_time` to `Instant::now()`
- `session.last_output = current_output.clone()` is set
- `session.chars_since_last_summary += new_chars` is updated

But **`session.send_time` is never updated/reset when new content arrives**. It stays fixed at the moment the user message was sent.

### 4. `reset_response_state()` -- session.rs lines 229-243

Clears everything including `send_time`, `response_buffer`, `is_waiting`, etc. Called when a response cycle completes or times out.

### 5. `start_response_collection()` -- session.rs lines 252-268

Sets `send_time = Some(Instant::now())` along with initializing all other response-cycle state.

## Recommended Fix

### Approach: Reset `send_time` when new content arrives

The simplest fix is to reset `send_time` to `Instant::now()` whenever new content is detected. This means the 5-minute timeout becomes "5 minutes since last new content" rather than "5 minutes since message sent."

**Location**: In the `current_output != session.last_output` branch of both functions.

**In `poll_topic_output`** (around line 1121, after `session.last_output = current_output.clone();`):
```rust
// Reset the hard timeout whenever new content arrives
session.send_time = Some(std::time::Instant::now());
```

**In `poll_output`** (around line 1605, same relative position):
```rust
// Reset the hard timeout whenever new content arrives
session.send_time = Some(std::time::Instant::now());
```

### Why this approach over adding a new field

1. **Simplest change** -- one line per function, no struct changes
2. **`send_time` is only used in two places**: the timeout check and latency logging at completion
3. **Latency logging impact**: The logged latency will reflect time-since-last-activity rather than total response time. If accurate total latency is needed, a separate `response_start_time` field could be added, but that is a separate concern.
4. **The existing `last_output_time` field** already tracks when content last arrived, but it uses a different threshold (1.5s for idle detection). We could check `last_output_time` in the timeout instead, but resetting `send_time` is cleaner and avoids changing the timeout logic structure.

### Alternative approach: Add `last_activity_time` field

If preserving `send_time` for accurate total latency logging is important:

1. Add `last_activity_time: Option<Instant>` to `UserSession`
2. Initialize it in `start_response_collection()` alongside `send_time`
3. Update it in the `current_output != session.last_output` branch
4. Clear it in `reset_response_state()`
5. Change the timeout check to use `last_activity_time` instead of `send_time`
6. Keep `send_time` unchanged for latency calculation

### Both functions need the fix

The bug exists in both:
- `poll_topic_output` (line 1085-1103) -- for forum/group sessions
- `poll_output` (line 1570-1587) -- for direct message sessions

## Files to Modify

| File | Lines | Change |
|------|-------|--------|
| `crates/commander-telegram/src/state.rs` | ~1121 | Add `send_time` reset in `poll_topic_output` new-content branch |
| `crates/commander-telegram/src/state.rs` | ~1605 | Add `send_time` reset in `poll_output` new-content branch |
| (Optional) `crates/commander-telegram/src/session.rs` | struct + methods | Add `last_activity_time` field if preserving original `send_time` for latency |
