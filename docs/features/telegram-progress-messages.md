# Telegram Progress Messages

**Status:** Implemented
**Date:** 2026-02-11

## Overview

Real-time progress messages are shown in Telegram during output collection and summarization, providing users with feedback about what's happening behind the scenes.

## User Experience Flow

### Example 1: Quick Response (No Progress)

```
User: "What files are in this directory?"
Bot: [typing indicator for 2s]
Bot: "Found 5 files: main.rs, lib.rs, config.toml, README.md, .gitignore"
```

**Why no progress message?**
- Response completed in < 5 lines before threshold reached
- No progress message shown for fast responses (avoids UI clutter)

---

### Example 2: Long Response with Progress

```
User: "Analyze all the Rust files and suggest improvements"
Bot: [typing indicator]
Bot: [sends new message] "📥 Receiving...5 lines captured"
Bot: [updates same message] "📥 Receiving...10 lines captured"
Bot: [updates same message] "📥 Receiving...15 lines captured"
Bot: [updates same message] "📥 Receiving...20 lines captured"
Bot: [updates same message] "🤖 Summarizing output..."
Bot: [deletes progress message]
Bot: [sends final response as reply to user's message]
     "I analyzed 5 Rust files and found several improvement opportunities:

      1. main.rs could benefit from better error handling...
      2. lib.rs has some duplicated logic that could be extracted...
      ..."
```

**Flow details:**
1. **Output Collection** (0-20 lines)
   - Progress updates every 5 lines
   - Same message edited in-place (no spam)
   - Shows actual line count captured

2. **Summarization** (when LLM API available)
   - Progress message updates to "🤖 Summarizing output..."
   - LLM call happens (may take 2-5 seconds)

3. **Final Response**
   - Progress message deleted
   - Clean final summary sent as reply

---

### Example 3: Fast Response with Summarization

```
User: "Create a hello world program"
Bot: [typing indicator]
Bot: "🤖 Summarizing output..."
Bot: "I created a simple hello world program in main.rs with proper error handling."
```

**Why no "Receiving..." message?**
- Response completed quickly (< 5 lines)
- Went straight to summarization phase

---

### Example 4: Long Response without Summarization (No API Key)

```
User: "Show me the test results"
Bot: [typing indicator]
Bot: "📥 Receiving...5 lines captured"
Bot: [updates] "📥 Receiving...10 lines captured"
Bot: [deletes progress message]
Bot: [raw cleaned output with UI noise removed]
```

**Why no "Summarizing" message?**
- OPENROUTER_API_KEY not configured
- Fallback to cleaned raw output (faster)

---

## Technical Implementation

### Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ poll_output_loop (bot.rs)                                     │
│ - Polls every 500ms                                           │
│ - Maintains progress_messages HashMap<chat_id, MessageId>    │
└──────────────────────────────────────────────────────────────┘
                           │
                           ↓
┌──────────────────────────────────────────────────────────────┐
│ poll_output (state.rs) → Returns PollResult enum:            │
│                                                                │
│ • NoOutput        - Keep waiting, show typing indicator      │
│ • Progress(msg)   - Update progress message every 5 lines    │
│ • Summarizing     - Output done, starting summarization      │
│ • Complete(resp)  - Final response ready to send             │
└──────────────────────────────────────────────────────────────┘
                           │
                           ↓
┌──────────────────────────────────────────────────────────────┐
│ UserSession (session.rs)                                      │
│ - response_buffer: Vec<String>  (accumulated lines)          │
│ - last_progress_line_count: usize  (threshold tracking)      │
│ - is_summarizing: bool  (two-phase completion detection)     │
│                                                                │
│ Methods:                                                      │
│ • should_emit_progress() → true every 5 lines                │
│ • get_progress_message() → "📥 Receiving...N lines captured" │
└──────────────────────────────────────────────────────────────┘
```

### Key Components

#### 1. `PollResult` Enum (state.rs)

```rust
pub enum PollResult {
    Progress(String),                    // "📥 Receiving...N lines captured"
    Summarizing,                         // Trigger "🤖 Summarizing output..."
    Complete(String, Option<MessageId>), // Final response
    NoOutput,                            // Keep waiting
}
```

#### 2. Progress Tracking (session.rs)

```rust
pub struct UserSession {
    // ... existing fields
    pub last_progress_line_count: usize,  // Update every 5 lines
    pub is_summarizing: bool,             // Two-phase completion
}

impl UserSession {
    pub fn should_emit_progress(&self) -> bool {
        let current = self.response_buffer.len();
        current > 0 && current >= self.last_progress_line_count + 5
    }

    pub fn get_progress_message(&mut self) -> String {
        let count = self.response_buffer.len();
        self.last_progress_line_count = count;
        format!("📥 Receiving...{} lines captured", count)
    }
}
```

#### 3. Bot Loop with Progress Handling (bot.rs)

```rust
async fn poll_output_loop(bot: Bot, state: Arc<TelegramState>) {
    let mut progress_messages: HashMap<i64, MessageId> = HashMap::new();

    loop {
        match state.poll_output(chat_id).await {
            Ok(PollResult::Progress(msg)) => {
                // Send or update progress message
                if let Some(&msg_id) = progress_messages.get(&chat_id) {
                    bot.edit_message_text(chat_id, msg_id, &msg).await;
                } else {
                    let sent = bot.send_message(chat_id, &msg).await?;
                    progress_messages.insert(chat_id, sent.id);
                }
            }
            Ok(PollResult::Summarizing) => {
                // Update to "Summarizing" message
                if let Some(&msg_id) = progress_messages.get(&chat_id) {
                    bot.edit_message_text(chat_id, msg_id, "🤖 Summarizing...").await;
                } else {
                    let sent = bot.send_message(chat_id, "🤖 Summarizing...").await?;
                    progress_messages.insert(chat_id, sent.id);
                }
            }
            Ok(PollResult::Complete(response, message_id)) => {
                // Delete progress message, send final response
                if let Some(msg_id) = progress_messages.remove(&chat_id) {
                    bot.delete_message(chat_id, msg_id).await;
                }
                bot.send_message(chat_id, &response)
                    .reply_parameters(ReplyParameters::new(message_id))
                    .await?;
            }
            Ok(PollResult::NoOutput) => {
                // Keep polling
            }
        }
    }
}
```

### Summarization Flow (Two-Phase Detection)

The summarization phase uses a two-poll approach to show "Summarizing..." before blocking:

**Poll 1 (Detection):**
```rust
if is_idle && has_prompt && !session.is_summarizing {
    session.is_summarizing = true;
    return Ok(PollResult::Summarizing);  // Bot shows "🤖 Summarizing..."
}
```

**Poll 2 (Execution):**
```rust
if is_idle && has_prompt && session.is_summarizing {
    let response = summarize_with_fallback(&query, &raw).await;  // Actual LLM call
    return Ok(PollResult::Complete(response, message_id));
}
```

**Why two polls?**
- Allows bot loop to show "Summarizing..." message before blocking on LLM call
- LLM calls can take 2-5 seconds - user sees feedback immediately
- Without this, user would see "📥 Receiving...20 lines" then freeze for 5s

---

## Configuration

### Enable Summarization (Optional)

Set environment variable to enable LLM summarization:

```bash
export OPENROUTER_API_KEY="sk-or-v1-..."
export OPENROUTER_MODEL="anthropic/claude-sonnet-4"  # Optional, defaults to sonnet-4
```

**Without API key:**
- No "🤖 Summarizing output..." message
- Fallback to cleaned raw output (faster)
- Progress messages still work

---

## Edge Cases Handled

### 1. Message Editing Failures

```rust
let _ = bot.edit_message_text(chat_id, msg_id, &msg).await;
```

**Scenario:** Message was already deleted by user
**Handling:** Ignore error, continue operation
**Result:** Next progress update creates new message

### 2. Fast Responses (< 5 Lines)

**Behavior:** No progress message shown
**Rationale:** Avoid UI clutter for quick responses
**Example:** "What time is it?" → Direct response, no progress

### 3. Rate Limiting Protection

**Threshold:** Update every 5 lines
**Calculation:** `current_lines >= last_count + 5`
**Effect:** Maximum ~20 updates for 100-line response
**Telegram limit:** 30 messages/second per chat (well within limit)

### 4. Multiple Concurrent Sessions

**Tracking:** `HashMap<i64, MessageId>` per chat
**Isolation:** Each chat has independent progress message
**Cleanup:** Removed from map after completion

### 5. Error Recovery

```rust
Err(e) => {
    warn!(error = %e, "Error polling output");
    if let Some(msg_id) = progress_messages.remove(&chat_id) {
        bot.delete_message(chat_id, msg_id).await;
    }
}
```

**On error:** Delete progress message, clean up state

---

## Testing

### Unit Tests (session.rs)

```rust
#[test]
fn test_progress_messages() {
    let mut session = UserSession::new(...);

    // Add 5 lines - should emit progress
    for i in 1..=5 {
        session.add_response_lines(vec![format!("line {}", i)]);
    }
    assert!(session.should_emit_progress());
    assert_eq!(session.get_progress_message(), "📥 Receiving...5 lines captured");

    // Add 4 more - should NOT emit yet
    for i in 6..=9 {
        session.add_response_lines(vec![format!("line {}", i)]);
    }
    assert!(!session.should_emit_progress());

    // Add 1 more to reach threshold
    session.add_response_lines(vec!["line 10".to_string()]);
    assert!(session.should_emit_progress());
    assert_eq!(session.get_progress_message(), "📥 Receiving...10 lines captured");
}
```

### Manual Testing Scenarios

1. **Short response test:**
   ```
   User: "/status"
   Expected: No progress, direct response
   ```

2. **Long response test:**
   ```
   User: "List all files recursively"
   Expected: "📥 Receiving...5 lines", "...10 lines", "...15 lines", then summary
   ```

3. **Summarization test (with API key):**
   ```
   User: "Analyze the codebase"
   Expected: Progress messages → "🤖 Summarizing output..." → Final summary
   ```

4. **No API key test:**
   ```
   unset OPENROUTER_API_KEY
   User: "Show me the logs"
   Expected: Progress messages → Raw cleaned output (no "Summarizing")
   ```

---

## Performance Considerations

### Overhead

- **Progress message creation:** Negligible (string formatting)
- **Message editing:** ~100ms per edit (Telegram API latency)
- **Update frequency:** Every 5 lines = ~4-10 edits per response
- **Total overhead:** < 1 second for typical responses

### Optimization

- **Update threshold:** 5 lines (tunable, see `should_emit_progress()`)
- **Edit vs Send:** Always edit existing message (prevents spam)
- **Cleanup:** Delete progress before final response (clean UX)

---

## Future Enhancements

1. **Configurable thresholds:**
   ```rust
   pub const PROGRESS_LINE_THRESHOLD: usize = 5;  // Make configurable
   ```

2. **Streaming progress:**
   ```
   "📥 Receiving...10 lines (2.5 KB)"  // Add size information
   ```

3. **ETA estimation:**
   ```
   "📥 Receiving...50 lines (~15s remaining)"  // Based on output rate
   ```

4. **Animated indicators:**
   ```
   "🤖 Summarizing output." → ".." → "..." (rotating animation)
   ```

---

## Related Files

- **Implementation:**
  - `crates/commander-telegram/src/session.rs` - Progress tracking
  - `crates/commander-telegram/src/state.rs` - PollResult enum, poll_output
  - `crates/commander-telegram/src/bot.rs` - poll_output_loop

- **Dependencies:**
  - `crates/commander-core/src/summarizer.rs` - LLM summarization
  - `crates/commander-core/src/output_filter.rs` - Response cleaning

- **Documentation:**
  - `docs/research/telegram-progress-messages-analysis-2026-02-11.md` - Research and design
  - `docs/features/telegram-progress-messages.md` - This file

---

## Rollout Status

- ✅ Implementation complete
- ✅ Unit tests passing
- ✅ Edge cases handled
- ✅ Documentation written
- ⏳ Production testing pending
- ⏳ User feedback pending
