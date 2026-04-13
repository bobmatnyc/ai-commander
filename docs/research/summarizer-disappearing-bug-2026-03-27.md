# Bug Investigation: Summarized Output "Disappears"

**Date:** 2026-03-27
**Investigator:** Claude Code
**Status:** Root causes identified — two distinct bugs found

---

## Executive Summary

There are two independent bugs causing summarized output to disappear or never be delivered. The primary bug is that a new incoming message while a response is in-flight calls `start_response_collection()`, which clears the `response_buffer` containing the accumulated output mid-collection, and resets `is_summarizing` to `false` — causing the old response cycle's completion path to re-enter the `PollResult::Summarizing` branch and loop forever without ever summarizing. The secondary bug is a logical flaw in the two-poll summarization state machine: on the second poll visit (when `is_summarizing == true`), `reset_response_state()` is called before `summarize_with_fallback()` returns, which clears `pending_query` and the buffer; the resulting summary is then sent as `PollResult::Complete`, but this is fine — however, the `is_idle` condition required to reach that second poll pass depends on no new output arriving, and in practice new tmux output keeps flowing so `is_idle` never becomes `true`, stalling indefinitely.

---

## Bug 1 (PRIMARY): New Message Resets In-Flight Response

### Evidence from Logs

At `04:26:43.015` the session has `buffer_len=77` and is actively collecting output. At `04:26:43.055` a new message arrives:

```
04:26:43.055  INFO  Regular message received  text="Let's also progressively summarize every 5 lines."
04:26:43.736  DEBUG  Message sent to project
04:26:43.835  DEBUG  poll_output: new tmux output captured  new_lines=2  buffer_len=2
```

The buffer has dropped from 77 to 2 lines in the space of a single poll. This is not tmux output changing — it is `start_response_collection()` running mid-collection.

### Code Path

`handlers.rs` receives the user's message and calls `state.send_message()`.

`state.rs:1292`:
```rust
session.start_response_collection(message, last_output, message_id);
```

`session.rs:241-252` — `start_response_collection()`:
```rust
pub fn start_response_collection(&mut self, query: &str, current_output: String, message_id: Option<MessageId>) {
    self.response_buffer.clear();           // <-- WIPES the in-flight buffer
    self.last_output = current_output;
    self.last_output_time = Some(Instant::now());
    self.pending_query = Some(query.to_string());
    self.is_waiting = true;
    self.pending_message_id = message_id;
    self.last_progress_line_count = 0;
    self.is_summarizing = false;            // <-- RESETS summarizing flag
    self.last_incremental_summary_line_count = 0;
    self.send_time = Some(Instant::now());
}
```

The 77 lines already collected for the previous query are wiped. `is_summarizing` is reset to `false`. The original response cycle has lost all its work.

### What Happens Next

After the reset, the new response cycle begins collecting output from scratch. Meanwhile `last_output` was set to the current tmux capture, so `find_new_lines` only picks up lines *after* this point — the entire in-flight response to the first query is permanently lost.

---

## Bug 2 (SECONDARY): Idle Detection Blocks the Summarizing State Machine

### The Two-Poll Design

The summarization path is designed as a two-poll state machine:

**Poll N (first time idle+prompt detected):**
```rust
// state.rs:1681-1684
if needs_summarization && !session.is_summarizing {
    session.is_summarizing = true;
    return Ok(PollResult::Summarizing);   // tells bot "show spinner"
}
```

**Poll N+1 (idle+prompt still true, is_summarizing == true):**
```rust
// state.rs:1698-1720
session.reset_response_state();           // clears buffer, is_summarizing, etc.
let response = summarize_with_fallback(&query, &raw_response).await;
return Ok(PollResult::Complete(response, ...));
```

The design intent is that Poll N signals the UI and Poll N+1 does the actual summarization.

### The Flaw

Poll N+1 requires that `is_idle && has_prompt` is still `true`. The `is_idle` check is:
```rust
// session.rs:266-270
pub fn is_idle(&self, idle_threshold_ms: u128) -> bool {
    self.last_output_time
        .map(|t| t.elapsed().as_millis() > idle_threshold_ms)
        .unwrap_or(false)
}
```

The idle threshold is 1500ms. But the poll loop fires every ~500ms and tmux is continuously outputting new lines (the bot itself is responding in tmux). Any new tmux line between Poll N and Poll N+1 calls `add_response_lines()`, which calls:
```rust
// session.rs:262
self.last_output_time = Some(Instant::now());
```

This resets the idle timer. So `is_idle` will never become `true` while Claude Code is actively producing output, even a single character every 500ms is enough to prevent completion.

The logs show this playing out: `is_idle=false, has_prompt=true` for the entire session from `04:25:35` through `04:27:07` (nearly 2 minutes), steadily incrementing `buffer_len` from 1 to 50+. The `Summarizing` branch is never reached because `is_idle` never flips to `true` while output is flowing.

### Observable Symptom

The progress message "Receiving...N lines captured" keeps incrementing but no summary or final response ever arrives. The session hangs indefinitely unless the 5-minute timeout fires, at which point it sends a timeout error, not the actual summary.

Previous sessions in the log confirm this pattern — multiple `5-minute timeout reached — force-completing` events, each delivering the error message instead of the response.

---

## Bug 3 (TERTIARY): Complete Handler Deletes Incremental Summary Messages

Even when a `PollResult::Complete` is eventually produced, the bot.rs handler at line 519-523 deletes both the progress message AND any incremental summary messages:

```rust
// bot.rs:518-524
Ok(PollResult::Complete(mut response, message_id, response_thread_id)) => {
    if let Some(prog_msg_id) = progress_messages.remove(&session_key) {
        let _ = bot.delete_message(chat_id, prog_msg_id).await;
    }
    if let Some(sum_msg_id) = summary_messages.remove(&session_key) {
        let _ = bot.delete_message(chat_id, sum_msg_id).await;  // DELETES summaries
    }
```

Incremental summaries (sent by `PollResult::IncrementalSummary`) are stored in `summary_messages`. When the final Complete fires, they are deleted. This means any incremental summary the user briefly saw is removed from the chat. This is likely the "shown briefly then removed" symptom.

---

## Code Path: Summary Generated to Message Sent (When Working)

1. `state.rs:1646` — `is_idle && has_prompt` becomes true
2. `state.rs:1681` — `!session.is_summarizing` first pass: sets flag, returns `PollResult::Summarizing`
3. `bot.rs:496-515` — sends/edits "Summarizing output..." into `progress_messages`
4. Next poll: `is_idle && has_prompt` must still be true
5. `state.rs:1688-1698` — extracts `raw_response`, calls `session.reset_response_state()`
6. `state.rs:1702` — `summarize_with_fallback()` calls OpenRouter API (~1-3 seconds)
7. `state.rs:1720` — returns `PollResult::Complete(summary, ...)`
8. `bot.rs:517` — deletes progress message, then calls `send_long_message()` with the summary

---

## Where the Summary Disappears

| Scenario | Where Lost |
|---|---|
| User sends new message while response in-flight | `start_response_collection()` wipes the buffer and resets `is_summarizing=false` at `session.rs:221-232` |
| Claude is actively outputting (common case) | `is_idle` never becomes `true`; session hangs until 5-minute timeout fires with error message instead of summary |
| Incremental summary (every 50 lines) shown briefly | Deleted at `bot.rs:522-524` when Complete fires |

---

## Specific Fix Recommendations

### Fix 1 — Reject new messages while session is waiting (session.rs)

File: `crates/commander-telegram/src/state.rs` — in `send_message()` before calling `start_response_collection()`:

The `send_message()` function should check whether the session is currently waiting and either queue the new message or reject it. At minimum, before wiping the buffer, the old response should be finalized or discarded gracefully with a user notification.

Alternatively, `start_response_collection()` should not reset `response_buffer` if `is_waiting == true`; instead a separate "new session" path should be taken.

### Fix 2 — Complete directly without the two-poll dance (state.rs)

File: `crates/commander-telegram/src/state.rs`, lines 1681-1684 (`poll_output`) and lines 1219-1222 (`poll_topic_output`).

The current approach waits for a second idle+prompt confirmation before summarizing. This fails when output is still flowing at completion. The fix is: when `is_idle && has_prompt` is first detected, do the summarization immediately in that same poll — send `Summarizing` signal to the bot asynchronously and return `Complete` in one pass, OR lock the session out of new-lines detection once idle is detected so the timer is not reset.

A simpler fix: record a `completion_detected_at: Option<Instant>` timestamp on first idle detection, and on the second pass check that timestamp rather than re-evaluating `is_idle`.

### Fix 3 — Do not delete incremental summary messages on Complete (bot.rs)

File: `crates/commander-telegram/src/bot.rs`, lines 522-524.

Remove or change the deletion of `summary_messages`. Incremental summaries are useful progress context; they should remain visible. Only the `progress_messages` (the "Receiving...N lines" status) should be deleted on completion.

```rust
// Current (deletes incremental summaries — bug):
if let Some(sum_msg_id) = summary_messages.remove(&session_key) {
    let _ = bot.delete_message(chat_id, sum_msg_id).await;
}

// Fix (keep incremental summaries, just remove from tracking map):
summary_messages.remove(&session_key);  // stop tracking, do not delete
```

---

## Files and Line Numbers

| File | Lines | Issue |
|---|---|---|
| `crates/commander-telegram/src/session.rs` | 241-252 | `start_response_collection()` unconditionally wipes buffer and resets `is_summarizing` |
| `crates/commander-telegram/src/session.rs` | 220-232 | `reset_response_state()` called on new message even when old response in-flight |
| `crates/commander-telegram/src/state.rs` | 1681-1684 | Two-poll summarization design requires second idle check that never fires during active output |
| `crates/commander-telegram/src/state.rs` | 1219-1222 | Same issue in `poll_topic_output` |
| `crates/commander-telegram/src/bot.rs` | 522-524 | Incremental summary messages deleted on Complete |
