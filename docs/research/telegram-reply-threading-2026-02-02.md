# Telegram Bot Reply Threading Investigation

**Date:** 2026-02-02
**Objective:** Enable bot responses to be threaded as replies to the original user message
**Status:** Research Complete - Implementation Ready

## Executive Summary

The Telegram bot currently sends responses as new messages without threading. To enable reply threading, we need to:

1. **Capture the original message ID** when receiving user messages
2. **Store the message ID** in the session or pass it through the polling system
3. **Use `.reply_to(msg.id)`** when sending responses

The teloxide library supports this via the `reply_to()` method on `SendMessage`.

## Current Response Sending Mechanism

### 1. Polling Output Loop (bot.rs:215-246)

The main response sending happens in `poll_output_loop`:

```rust
async fn poll_output_loop(bot: Bot, state: Arc<TelegramState>) {
    // ...
    match state.poll_output(ChatId(chat_id)).await {
        Ok(Some(response)) => {
            // Response sent WITHOUT reply threading
            if let Err(e) = bot.send_message(ChatId(chat_id), &response).await {
                // error handling
            }
        }
        // ...
    }
}
```

**Issue:** This code only has access to `chat_id` and `response` - it does NOT have access to the original message ID.

### 2. Command/Message Handlers (handlers.rs)

Handler functions receive the full `Message` object which includes `msg.id`:

```rust
pub async fn handle_message(
    bot: Bot,
    msg: Message,  // Contains msg.id (MessageId)
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    // ...
    match state.send_message(msg.chat.id, text).await {
        Ok(()) => {
            // Message sent to tmux session
            // Response will be polled and sent back by the polling task
        }
        // ...
    }
}
```

**Issue:** The `msg.id` is available here but NOT passed to the state or stored for later retrieval.

### 3. State Management (state.rs:265-295)

```rust
pub async fn send_message(&self, chat_id: ChatId, message: &str) -> Result<()> {
    // Sends message to tmux session
    // Starts response collection
    session.start_response_collection(message, last_output);
    // ...
}
```

**Issue:** No mechanism to store or retrieve the original message ID.

## Telegram API for Reply Threading

### teloxide `reply_to()` Method

Per [teloxide documentation](https://docs.rs/teloxide/latest/teloxide/payloads/struct.SendMessage.html), the modern approach uses:

```rust
bot.send_message(chat_id, "Response text")
    .reply_to(msg.id)  // or .reply_to(message_id)
    .await?;
```

Key points:
- `reply_to()` is a convenience method from `RequestReplyExt` trait
- Replaces the deprecated `reply_to_message_id()` approach
- Accepts either `MessageId` or `Message` reference
- Creates a proper threaded reply in Telegram UI

## Implementation Approach

### Option A: Store Message ID in Session (Recommended)

**Modifications needed:**

1. **session.rs** - Add field to `UserSession`:
   ```rust
   pub struct UserSession {
       // existing fields...
       pub pending_message_id: Option<MessageId>,
   }
   ```

2. **state.rs** - Modify `send_message()` to accept message ID:
   ```rust
   pub async fn send_message(
       &self,
       chat_id: ChatId,
       message: &str,
       message_id: MessageId,  // NEW
   ) -> Result<()> {
       // ...
       session.pending_message_id = Some(message_id);
       // ...
   }
   ```

3. **state.rs** - Modify `poll_output()` to return message ID with response:
   ```rust
   pub async fn poll_output(&self, chat_id: ChatId) -> Result<Option<(String, Option<MessageId>)>> {
       // ...
       let message_id = session.pending_message_id.take();
       return Ok(Some((response, message_id)));
   }
   ```

4. **bot.rs** - Modify polling loop to use reply_to:
   ```rust
   match state.poll_output(ChatId(chat_id)).await {
       Ok(Some((response, Some(msg_id)))) => {
           bot.send_message(ChatId(chat_id), &response)
               .reply_to(msg_id)
               .await?;
       }
       Ok(Some((response, None))) => {
           // Fallback without reply (shouldn't happen)
           bot.send_message(ChatId(chat_id), &response).await?;
       }
       // ...
   }
   ```

5. **handlers.rs** - Pass message ID to send_message:
   ```rust
   match state.send_message(msg.chat.id, text, msg.id).await {
       // ...
   }
   ```

### Option B: Separate Message ID Queue

Store message IDs in a separate queue structure rather than in the session. More complex but allows for multiple pending messages.

### Recommendation: Option A

Option A is simpler and fits the current 1:1 message-response pattern of the bot.

## Files Requiring Modification

| File | Changes |
|------|---------|
| `crates/commander-telegram/src/session.rs` | Add `pending_message_id: Option<MessageId>` field |
| `crates/commander-telegram/src/state.rs` | Update `send_message()` signature, update `poll_output()` return type |
| `crates/commander-telegram/src/handlers.rs` | Pass `msg.id` to `send_message()` |
| `crates/commander-telegram/src/bot.rs` | Use `.reply_to()` in polling loop |

## Required Import

```rust
use teloxide::types::MessageId;
```

The `teloxide::prelude::*` already includes the `RequestReplyExt` trait for `.reply_to()`.

## Testing Considerations

1. Test that responses appear as replies in Telegram UI
2. Test that commands still work correctly (they respond inline already)
3. Test edge case where message ID might not be available

## Sources

- [SendMessage in teloxide::payloads - Rust](https://docs.rs/teloxide/latest/teloxide/payloads/struct.SendMessage.html)
- [Message in teloxide::types - Rust](https://docs.rs/teloxide/latest/teloxide/types/struct.Message.html)
- [teloxide GitHub Repository](https://github.com/teloxide/teloxide)
