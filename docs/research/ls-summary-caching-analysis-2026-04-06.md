# /ls Session Summary Caching Analysis

**Date:** 2026-04-06
**Goal:** Understand how `/ls` gets and displays session summaries; identify where to add checksum-based invalidation.

---

## 1. handle_list Flow (handlers.rs:1119-1238)

`handle_list` does the following per tmux session:

1. Calls `state.list_tmux_sessions_with_status()` -- captures **fresh tmux output** (15 lines) for each session (state.rs:2239).
2. For each session, calls `get_session_summary(preview, is_active)` (handlers.rs:1173).
3. `get_session_summary` calls `commander_core::interpret_screen_context()` (handlers.rs:1111) which makes a **live LLM call** to OpenRouter (summarizer.rs:426-471).

**Key finding: There is NO cache.** Every `/ls` invocation makes one LLM API call per tmux session. The "stale summary" problem is not a caching bug -- it is that the tmux capture itself may be stale or the LLM may produce inconsistent summaries from similar output.

## 2. Summary Cache -- Does Not Exist

Searched for `summary_cache`, `cached_summary`, `last_summary` across all telegram source files. **No summary cache exists.** The fields that do exist:

- `UserSession::chars_since_last_summary` (session.rs:117) -- counter for **progressive summarization** during active polling, unrelated to `/ls`.
- `UserSession::last_output` (session.rs:84) -- last captured tmux output used for **change detection during polling** (state.rs:1257), not used by `/ls`.

## 3. Session Output Capture for /ls

- **Tmux sessions:** `list_tmux_sessions_with_status()` (state.rs:2220) calls `tmux.capture_output(&s.name, None, Some(15))` to grab last 15 lines of terminal output. This is filtered to remove noise (prompt lines, claude_mpm, etc.) at state.rs:2243-2256.
- **Event-driven sessions:** Only display status string ("active"/"starting"/"idle") from `EventHandleState`. No summary or output is shown (handlers.rs:1210-1231). No LLM call is made.
- `UserSession::last_output` is NOT used by `/ls`. It is only used in the poll loop for change detection.

## 4. Summarization Trigger

- `/ls` triggers a **fresh LLM call for every tmux session on every invocation** via `get_session_summary` -> `interpret_screen_context`.
- Progressive summarization (during active polling) uses `chars_since_last_summary` and triggers every 500 chars of new output (state.rs:1274). This is completely separate from `/ls`.
- There is no idle-transition summary. There is no cached-from-poll-cycle summary.

## 5. Root Cause of "Stale" Summaries

Since there is no cache, the perceived staleness likely comes from:

1. **Tmux capture returns old terminal content** -- if the session has scrolled or the last 15 lines are old output, the LLM summarizes stale content.
2. **LLM inconsistency** -- similar terminal output produces different summaries across calls.
3. **Slow LLM calls** -- multiple sessions mean multiple serial 10s-timeout API calls, so the first session's capture may be outdated by the time the user sees the result.

## 6. Proposed Checksum-Based Invalidation Design

Even though there is no cache today, adding one with checksum invalidation would:
- Eliminate redundant LLM calls when output hasn't changed
- Speed up `/ls` significantly (skip LLM for unchanged sessions)

### Where to store the hash

Add two fields to `UserSession` (session.rs):
```rust
pub ls_summary_cache: Option<String>,      // cached summary text
pub ls_summary_hash: Option<u64>,          // hash of the tmux capture that produced it
```

### Implementation in handle_list

```
for each tmux session:
    capture = tmux.capture_output(name, 15)
    hash = hash(capture)
    if session.ls_summary_hash == Some(hash):
        summary = session.ls_summary_cache  // use cached
    else:
        summary = get_session_summary(capture)  // LLM call
        session.ls_summary_hash = Some(hash)
        session.ls_summary_cache = summary
```

### Challenge: session lookup

`handle_list` iterates tmux sessions (not `UserSession` instances). The tmux session name would need to be matched to a `UserSession` to read/write the cache. Sessions not connected by any chat would have no `UserSession` -- a separate cache map keyed by tmux session name (e.g., on `TelegramState`) would be cleaner.

### Recommended location for cache

Add to `TelegramState`:
```rust
pub ls_summary_cache: RwLock<HashMap<String, (u64, String)>>
// key: tmux session name, value: (output_hash, summary_text)
```

This keeps the cache independent of `UserSession` lifecycle and works for all sessions.

---

## File References

| What | Location |
|------|----------|
| `handle_list` | `handlers.rs:1119-1238` |
| `get_session_summary` | `handlers.rs:1105-1116` |
| `interpret_screen_context` (LLM call) | `summarizer.rs:426-471` |
| `list_tmux_sessions_with_status` | `state.rs:2220-2263` |
| `UserSession` struct | `session.rs:70-128` |
| `last_output` (poll-loop only) | `session.rs:84`, `state.rs:1257` |
| `chars_since_last_summary` (progressive) | `session.rs:117`, `state.rs:1274` |
| Event-driven session listing | `state.rs:2265-2281` |
