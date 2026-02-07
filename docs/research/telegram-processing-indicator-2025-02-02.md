# Telegram Bot Processing Indicator Implementation

**Date**: 2025-02-02
**Status**: Research Complete
**Topic**: Show "processing" indicators in Telegram bot for long-running sessions

---

## Executive Summary

The current commander-telegram bot sends a single typing indicator when a message is received, but this indicator expires after 5 seconds. Long-running Claude Code operations (10-60+ seconds) need continuous typing indicators to show users that processing is ongoing. The solution is to send repeated `sendChatAction` calls every 4-5 seconds in the existing polling loop until the response is ready.

---

## Current Message Handling Flow

### 1. User Sends Message (handlers.rs)

```
User message received
    |
    v
handle_message() in handlers.rs:485-522
    |
    +-- Check if connected to project (has_session)
    |
    +-- Send single typing indicator (line 505)
    |       bot.send_chat_action(msg.chat.id, ChatAction::Typing)
    |
    +-- Forward message to Claude Code via tmux (state.send_message)
    |
    +-- Return immediately (non-blocking)
    v
Response polling handled by separate background task
```

### 2. Response Polling Loop (bot.rs)

```
poll_output_loop() in bot.rs:215-246
    |
    v
Every 500ms (POLL_INTERVAL_MS):
    |
    +-- Get all waiting chat IDs
    |
    +-- For each waiting chat:
    |       +-- Call state.poll_output()
    |       +-- If response ready:
    |       |       +-- Send response to user
    |       +-- If not ready:
    |               +-- Continue polling (no typing indicator sent!)
    v
Loop continues until response detected
```

### 3. Response Detection Logic (state.rs)

```
poll_output() in state.rs:299-345
    |
    +-- Capture tmux output
    +-- Compare with last output for new lines
    +-- Check idle detection (1.5s since last output change)
    +-- Check if Claude Code is at prompt (ready)
    +-- If idle AND ready AND has content:
            +-- Return summarized response
    v
Otherwise return None (still processing)
```

---

## Problem Analysis

**The Issue**: A single `sendChatAction` call only shows the typing indicator for **5 seconds**. Claude Code operations can take 10-60+ seconds. After the initial 5 seconds, users see no activity indicator.

**User Experience Impact**:
- User sends message, sees "typing..." for 5 seconds
- Indicator disappears while Claude Code is still processing
- User may think the bot is frozen/broken
- User might send duplicate messages

---

## Telegram Bot API: sendChatAction

### Available ChatAction Types (from teloxide-core)

| Action | Use Case |
|--------|----------|
| `Typing` | Processing text, running commands |
| `UploadPhoto` | Preparing to send a photo |
| `RecordVideo` | Recording video |
| `UploadVideo` | Uploading video |
| `RecordVoice` | Recording voice message |
| `UploadVoice` | Uploading voice message |
| `UploadDocument` | Uploading a document |
| `FindLocation` | Finding a location |
| `RecordVideoNote` | Recording video note |
| `UploadVideoNote` | Uploading video note |

**For commander-telegram**: Use `Typing` for all Claude Code operations since responses are text-based.

### Important Behavior Notes

1. **5-second expiration**: Status is set for 5 seconds or less
2. **Auto-clear on message**: When bot sends any message, typing status clears automatically
3. **Repetition required**: For long operations, must resend every 4-5 seconds

Source: [Telegram Bot API sendChatAction](https://core.telegram.org/bots/api#sendchataction), [n8n Community Discussion](https://community.n8n.io/t/how-to-make-telegram-action-typing-work-while-ai-agent-processing/69149)

---

## Recommended Implementation

### Option A: Add Typing to Polling Loop (Recommended)

Modify `poll_output_loop()` in `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs` to send typing indicators for waiting chats:

```rust
// In poll_output_loop(), inside the loop after getting waiting_ids:

for chat_id in waiting_ids {
    // Send typing indicator BEFORE polling (every 500ms is fine,
    // Telegram will maintain for ~5s)
    let _ = bot.send_chat_action(ChatId(chat_id), ChatAction::Typing).await;

    // Then poll for output as before
    match state.poll_output(ChatId(chat_id)).await {
        // ... existing logic
    }
}
```

**Location**: `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs`
**Lines**: 220-244 (inside the polling loop)

**Pros**:
- Simple change in one location
- Uses existing polling infrastructure
- No new tasks or complexity
- Natural fit - sends typing while checking for response

**Cons**:
- Sends typing indicator every 500ms (more than needed, but harmless)

### Option B: Dedicated Typing Task (More Complex)

Spawn a separate task per chat that sends typing indicators every 4 seconds:

```rust
// When starting to wait for response:
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(4));
    loop {
        interval.tick().await;
        if !state.is_waiting(chat_id) {
            break;
        }
        let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;
    }
});
```

**Pros**:
- More precise control over timing
- Only sends when needed

**Cons**:
- More complex task management
- Need to track and cancel tasks
- Overkill for this use case

### Option C: Modify POLL_INTERVAL for Typing (Not Recommended)

Change `POLL_INTERVAL_MS` from 500 to 4000 to match typing expiration.

**Why Not**: Would make response delivery slower (up to 4 seconds delay).

---

## Specific Code Changes

### Implementation (Option A)

**File**: `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs`

**Before** (lines 220-244):
```rust
loop {
    poll_interval.tick().await;

    // Get all chat IDs that are waiting for responses
    let waiting_ids = state.get_waiting_chat_ids().await;

    for chat_id in waiting_ids {
        // Poll for output from this session
        match state.poll_output(ChatId(chat_id)).await {
            // ... handler logic
        }
    }
}
```

**After**:
```rust
loop {
    poll_interval.tick().await;

    // Get all chat IDs that are waiting for responses
    let waiting_ids = state.get_waiting_chat_ids().await;

    for chat_id in waiting_ids {
        // Send typing indicator to show processing is ongoing
        // This refreshes every ~500ms, keeping the indicator visible
        let _ = bot.send_chat_action(
            ChatId(chat_id),
            teloxide::types::ChatAction::Typing
        ).await;

        // Poll for output from this session
        match state.poll_output(ChatId(chat_id)).await {
            // ... handler logic unchanged
        }
    }
}
```

**Add import** at top of file:
```rust
use teloxide::types::ChatAction;  // Add this if not already imported
```

### Optimization: Throttle Typing Indicator

To avoid excessive API calls, only send typing every 4 seconds:

```rust
// Add to UserSession struct in session.rs:
pub last_typing_time: Option<Instant>,

// In poll_output_loop:
for chat_id in waiting_ids {
    // Only send typing if 4+ seconds since last send
    if should_send_typing(&state, chat_id).await {
        let _ = bot.send_chat_action(
            ChatId(chat_id),
            ChatAction::Typing
        ).await;
        state.update_typing_time(chat_id).await;
    }

    match state.poll_output(ChatId(chat_id)).await {
        // ...
    }
}
```

This optimization is optional - Telegram handles frequent `sendChatAction` calls gracefully.

---

## Existing Patterns in Codebase

### Current Typing Indicator Usage

**File**: `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`
**Lines**: 504-506

```rust
// Send typing indicator
bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
    .await?;
```

This is the only place typing is currently sent. It happens once when a message is received.

### Polling Infrastructure

The bot already has a robust polling loop (`poll_output_loop`) that:
- Runs every 500ms
- Tracks which chats are waiting for responses
- Detects when Claude Code is done processing

This infrastructure is ideal for adding continuous typing indicators.

---

## Testing Plan

1. **Unit Test**: Verify typing is sent with waiting sessions
2. **Manual Test**:
   - Send a command that takes 10+ seconds
   - Verify "typing..." indicator persists throughout
   - Verify indicator stops when response arrives

---

## Rollout Considerations

- **API Rate Limits**: Telegram allows frequent `sendChatAction` calls (no known rate limit for this method)
- **Error Handling**: Use `let _ =` to ignore errors (typing is non-critical)
- **Logging**: Consider debug-level logging for typing sends

---

## Summary

| Question | Answer |
|----------|--------|
| Current handling | Single typing indicator on message receipt, expires after 5 seconds |
| Telegram API feature | `sendChatAction` with `Typing` action, expires in 5 seconds |
| Implementation location | `poll_output_loop()` in `bot.rs` lines 220-244 |
| Existing patterns | Uses `ChatAction::Typing` already in handlers.rs |
| Recommended approach | Add typing indicator send inside polling loop for waiting chats |
| Complexity | Low - single location change, ~5 lines of code |

---

## References

- [Telegram Bot API - sendChatAction](https://core.telegram.org/bots/api#sendchataction)
- [n8n Community - Typing Indicator for AI Agents](https://community.n8n.io/t/how-to-make-telegram-action-typing-work-while-ai-agent-processing/69149)
- [Telegraf Issue #1801 - Continuous Chat Actions](https://github.com/telegraf/telegraf/issues/1801)
- Teloxide source: `teloxide-core-0.10.1/src/types/chat_action.rs`
