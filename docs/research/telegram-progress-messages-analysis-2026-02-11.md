# Telegram Progress Messages: Output Collection and Summarization Flow Analysis

**Date:** 2026-02-11
**Context:** User wants progress messages like "receiving...N lines captured" and "summarizing output" to be sent to Telegram as updating progress messages during the output collection phase.

---

## Executive Summary

Progress messages are **NOT currently generated** in the codebase. The user sees only:
1. Typing indicator while output collection is in progress (via `poll_output_loop`)
2. Final summarized response when collection completes

To implement progress messages, we need to:
1. Add progress message generation to `UserSession::add_response_lines()`
2. Create a mechanism to send/update Telegram messages from the background polling task
3. Use `bot.edit_message_text()` to update a single message in-place rather than spamming multiple messages

---

## Current Flow Analysis

### 1. Where Output Collection Happens

#### File: `crates/commander-telegram/src/state.rs`

**Key Method:** `poll_output()` (lines 549-592)

```rust
pub async fn poll_output(&self, chat_id: ChatId) -> Result<Option<(String, Option<MessageId>)>> {
    // 1. Check if session is waiting for response
    if !session.is_waiting {
        return Ok(None);
    }

    // 2. Capture current tmux output (200 lines)
    let current_output = tmux
        .capture_output(&session.tmux_session, None, Some(200))
        .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

    // 3. Compare with last output to find new lines
    if current_output != session.last_output {
        let new_lines = find_new_lines(&session.last_output, &current_output);
        session.add_response_lines(new_lines);  // ← Progress could be tracked here
        session.last_output = current_output.clone();
    }

    // 4. Check if idle and ready to return response
    let is_idle = session.is_idle(1500); // 1.5s idle threshold
    let has_prompt = is_claude_ready(&current_output);

    if is_idle && has_prompt && !session.response_buffer.is_empty() {
        let raw_response = session.get_response();
        let query = session.pending_query.clone().unwrap_or_default();
        let message_id = session.pending_message_id;
        session.reset_response_state();

        // 5. Summarize using OpenRouter API
        let response = summarize_with_fallback(&query, &raw_response).await;  // ← "Summarizing" happens here

        return Ok(Some((response, message_id)));
    }

    Ok(None)
}
```

**Key Observations:**
- Line collection happens at `session.add_response_lines(new_lines)` (line 571)
- Summarization happens at `summarize_with_fallback()` (line 586)
- **No progress messages currently generated**

---

#### File: `crates/commander-telegram/src/session.rs`

**Key Methods:**

```rust
pub struct UserSession {
    pub response_buffer: Vec<String>,  // ← Lines accumulate here
    pub last_output_time: Option<Instant>,
    pub pending_query: Option<String>,
    pub is_waiting: bool,
    // ... other fields
}

// Lines 74-82: Where new lines are added
pub fn add_response_lines(&mut self, lines: Vec<String>) {
    for line in lines {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            self.response_buffer.push(trimmed.to_string());  // ← Progress could be emitted here
        }
    }
    self.last_output_time = Some(Instant::now());
}

// Lines 92-94: Get accumulated response
pub fn get_response(&self) -> String {
    self.response_buffer.join("\n")
}
```

**Progress Tracking Opportunity:**
- `response_buffer.len()` gives the current line count
- This could be used to generate "receiving...N lines captured" messages

---

### 2. Where Summarization Happens

#### File: `crates/commander-core/src/summarizer.rs`

**Key Methods:**

```rust
// Lines 165-179: Async summarization with fallback
pub async fn summarize_with_fallback(query: &str, raw_response: &str) -> String {
    let Some(api_key) = get_api_key() else {
        return clean_response(raw_response);  // No API key → return cleaned raw
    };

    let model = get_model();

    match summarize_async(query, raw_response, &api_key, &model).await {
        Ok(summary) => summary,  // ← Successful summarization
        Err(e) => {
            warn!(error = %e, "Summarization failed, using raw response");
            clean_response(raw_response)  // Fallback to cleaned raw
        }
    }
}

// Lines 71-110: Actual LLM summarization
pub async fn summarize_async(
    query: &str,
    raw_response: &str,
    api_key: &str,
    model: &str,
) -> Result<String, SummarizerError> {
    let user_prompt = format!(
        "User asked: {}\n\nRaw response:\n{}\n\nProvide a conversational summary:",
        query, raw_response
    );

    // OpenRouter API call with Claude Sonnet 4
    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    // Parse response
    let json: serde_json::Value = response.json().await?;
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| SummarizerError::ParseError("No content in response".to_string()))
}
```

**Progress Opportunity:**
- Before calling `summarize_async()`, could send "Summarizing output..." message
- After completion, could update message with summary

---

### 3. Background Polling Task

#### File: `crates/commander-telegram/src/bot.rs`

**Key Function:** `poll_output_loop()` (lines 232-275)

```rust
async fn poll_output_loop(bot: Bot, state: Arc<TelegramState>) {
    use teloxide::types::{ChatId, ChatAction, ReplyParameters};

    let mut poll_interval = interval(Duration::from_millis(500)); // Poll every 500ms

    loop {
        poll_interval.tick().await;

        // Get all chat IDs waiting for responses
        let waiting_ids = state.get_waiting_chat_ids().await;

        for chat_id in waiting_ids {
            // Show typing indicator
            let _ = bot.send_chat_action(ChatId(chat_id), ChatAction::Typing).await;

            // Poll for output
            match state.poll_output(ChatId(chat_id)).await {
                Ok(Some((response, message_id))) => {
                    // Send final response
                    let send_result = if let Some(msg_id) = message_id {
                        bot.send_message(ChatId(chat_id), &response)
                            .reply_parameters(ReplyParameters::new(msg_id))
                            .await
                    } else {
                        bot.send_message(ChatId(chat_id), &response).await
                    };

                    if let Err(e) = send_result {
                        warn!(chat_id = %chat_id, error = %e, "Failed to send response");
                    }
                }
                Ok(None) => {
                    // Still collecting, continue polling
                }
                Err(e) => {
                    warn!(chat_id = %chat_id, error = %e, "Error polling output");
                }
            }
        }
    }
}
```

**Current Behavior:**
- Every 500ms, sends typing indicator (shows as "typing..." in Telegram)
- When complete, sends final response as a new message
- **No intermediate progress messages**

---

### 4. Telegram Bot API for Updating Messages

#### Current Usage

**Sending Messages:** `bot.send_message()` (line 253 in bot.rs, line 680 in handlers.rs)

**Message Actions:**
- `bot.send_chat_action(ChatId(chat_id), ChatAction::Typing)` - Shows typing indicator

#### Available for Updating Messages

**From teloxide documentation:**

```rust
// Edit message text (in-place update)
bot.edit_message_text(chat_id, message_id, new_text)
    .await?;

// Example usage:
let sent_msg = bot.send_message(chat_id, "Starting...").await?;
tokio::time::sleep(Duration::from_secs(2)).await;
bot.edit_message_text(chat_id, sent_msg.id, "Processing...").await?;
```

**Key Capability:**
- `edit_message_text()` allows updating a message without sending multiple messages
- Requires storing the `MessageId` of the progress message
- Can be called repeatedly to update the same message

---

## Implementation Architecture

### Option 1: Progress Messages in UserSession (Recommended)

**Location:** `crates/commander-telegram/src/session.rs`

**Add to UserSession:**

```rust
pub struct UserSession {
    // ... existing fields
    pub progress_message_id: Option<MessageId>,  // NEW: Track progress message
}
```

**Modify `add_response_lines()`:**

```rust
pub fn add_response_lines(&mut self, lines: Vec<String>) -> Option<String> {
    let old_count = self.response_buffer.len();

    for line in lines {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            self.response_buffer.push(trimmed.to_string());
        }
    }
    self.last_output_time = Some(Instant::now());

    let new_count = self.response_buffer.len();

    // Generate progress message if line count increased significantly
    if new_count > old_count && (new_count % 5 == 0 || new_count - old_count >= 5) {
        Some(format!("📥 Receiving...{} lines captured", new_count))
    } else {
        None
    }
}
```

**Key Design Decisions:**
- Return `Option<String>` to emit progress messages only when significant (every 5 lines)
- Prevents spamming updates on every single line
- Still allows caller to decide whether to send message

---

### Option 2: Progress Messages in poll_output (Alternative)

**Location:** `crates/commander-telegram/src/state.rs`

**Modify `poll_output()` to return progress updates:**

```rust
pub enum PollResult {
    Progress { line_count: usize },  // Collection in progress
    Summarizing,                      // About to summarize
    Complete { response: String, message_id: Option<MessageId> },  // Done
    None,                              // No update
}

pub async fn poll_output(&self, chat_id: ChatId) -> Result<Option<PollResult>> {
    // ... existing code ...

    // After adding new lines
    if current_output != session.last_output {
        let new_lines = find_new_lines(&session.last_output, &current_output);
        session.add_response_lines(new_lines);
        session.last_output = current_output.clone();

        // Emit progress update
        return Ok(Some(PollResult::Progress {
            line_count: session.response_buffer.len()
        }));
    }

    // When starting summarization
    if is_idle && has_prompt && !session.response_buffer.is_empty() {
        // First emit "Summarizing" status
        let should_summarize = summarize_with_fallback needs to be called;
        if should_summarize {
            return Ok(Some(PollResult::Summarizing));
        }

        // Then on next poll, return complete response
        // ... existing summarization code ...
        return Ok(Some(PollResult::Complete { response, message_id }));
    }

    Ok(Some(PollResult::None))
}
```

**Tradeoff:**
- More complex API (enum instead of simple Option)
- Better separation of concerns (poll_output knows about progress, bot.rs handles Telegram)
- Requires refactoring the polling loop

---

### Option 3: Channel-Based Progress Updates (Most Decoupled)

**Architecture:**

```rust
// In state.rs
pub struct TelegramState {
    // NEW: Channel for progress updates
    progress_tx: mpsc::UnboundedSender<ProgressUpdate>,
}

pub enum ProgressUpdate {
    LinesCaptured { chat_id: i64, count: usize },
    Summarizing { chat_id: i64 },
    Complete { chat_id: i64, response: String, message_id: Option<MessageId> },
}

// In session.rs
pub fn add_response_lines(&mut self, lines: Vec<String>, tx: &mpsc::UnboundedSender<ProgressUpdate>) {
    // ... add lines ...

    if new_count % 5 == 0 {
        let _ = tx.send(ProgressUpdate::LinesCaptured {
            chat_id: self.chat_id.0,
            count: new_count
        });
    }
}

// In bot.rs - separate progress handler task
async fn handle_progress_updates(bot: Bot, mut rx: mpsc::UnboundedReceiver<ProgressUpdate>) {
    // Track progress messages for each chat
    let mut progress_messages: HashMap<i64, MessageId> = HashMap::new();

    while let Some(update) = rx.recv().await {
        match update {
            ProgressUpdate::LinesCaptured { chat_id, count } => {
                let text = format!("📥 Receiving...{} lines captured", count);

                // Update existing message or send new one
                if let Some(&msg_id) = progress_messages.get(&chat_id) {
                    let _ = bot.edit_message_text(ChatId(chat_id), msg_id, text).await;
                } else {
                    if let Ok(msg) = bot.send_message(ChatId(chat_id), text).await {
                        progress_messages.insert(chat_id, msg.id);
                    }
                }
            }
            ProgressUpdate::Summarizing { chat_id } => {
                if let Some(&msg_id) = progress_messages.get(&chat_id) {
                    let _ = bot.edit_message_text(
                        ChatId(chat_id),
                        msg_id,
                        "🤖 Summarizing output..."
                    ).await;
                }
            }
            ProgressUpdate::Complete { chat_id, response, message_id } => {
                // Remove progress message
                if let Some(progress_msg_id) = progress_messages.remove(&chat_id) {
                    let _ = bot.delete_message(ChatId(chat_id), progress_msg_id).await;
                }

                // Send final response
                if let Some(msg_id) = message_id {
                    let _ = bot.send_message(ChatId(chat_id), &response)
                        .reply_parameters(ReplyParameters::new(msg_id))
                        .await;
                } else {
                    let _ = bot.send_message(ChatId(chat_id), &response).await;
                }
            }
        }
    }
}
```

**Advantages:**
- Clean separation: output collection doesn't know about Telegram
- Background task handles all message updating logic
- Easy to add more progress types
- Prevents blocking during Telegram API calls

**Disadvantages:**
- More complexity (channels, separate task)
- Slight latency (goes through channel)
- Requires passing `tx` to session methods

---

## Recommended Implementation Plan

### Phase 1: Minimal Progress Messages (Simplest)

**Changes:**

1. **Modify `UserSession::add_response_lines()` to track significant updates:**

```rust
// In crates/commander-telegram/src/session.rs
pub fn should_emit_progress(&self) -> bool {
    let count = self.response_buffer.len();
    count > 0 && count % 5 == 0  // Emit every 5 lines
}

pub fn get_progress_message(&self) -> String {
    format!("📥 Receiving...{} lines captured", self.response_buffer.len())
}
```

2. **Modify `poll_output()` to send progress updates:**

```rust
// In crates/commander-telegram/src/state.rs
pub async fn poll_output(&self, chat_id: ChatId) -> Result<Option<PollResult>> {
    // ... existing code to add lines ...

    if current_output != session.last_output {
        let new_lines = find_new_lines(&session.last_output, &current_output);
        session.add_response_lines(new_lines);
        session.last_output = current_output.clone();

        // NEW: Return progress if significant
        if session.should_emit_progress() {
            return Ok(Some(PollResult::Progress(session.get_progress_message())));
        }
    }

    // ... rest of method ...
}

pub enum PollResult {
    Progress(String),
    Complete(String, Option<MessageId>),
}
```

3. **Update `poll_output_loop()` to handle progress:**

```rust
// In crates/commander-telegram/src/bot.rs

// Track progress message for each chat
let mut progress_messages: HashMap<i64, MessageId> = HashMap::new();

for chat_id in waiting_ids {
    match state.poll_output(ChatId(chat_id)).await {
        Ok(Some(PollResult::Progress(text))) => {
            // Update or create progress message
            if let Some(&msg_id) = progress_messages.get(&chat_id) {
                let _ = bot.edit_message_text(ChatId(chat_id), msg_id, text).await;
            } else {
                if let Ok(msg) = bot.send_message(ChatId(chat_id), text).await {
                    progress_messages.insert(chat_id, msg.id);
                }
            }
        }
        Ok(Some(PollResult::Complete(response, message_id))) => {
            // Delete progress message
            if let Some(progress_msg_id) = progress_messages.remove(&chat_id) {
                let _ = bot.delete_message(ChatId(chat_id), progress_msg_id).await;
            }

            // Send final response
            // ... existing send logic ...
        }
        _ => {}
    }
}
```

**Estimated Changes:**
- 3 files modified
- ~50 lines of new code
- Minimal complexity
- Works with existing architecture

---

### Phase 2: Add Summarization Progress (Enhancement)

**Additional Changes:**

1. **Add "summarizing" state to PollResult:**

```rust
pub enum PollResult {
    Progress(String),
    Summarizing,  // NEW
    Complete(String, Option<MessageId>),
}
```

2. **Emit Summarizing before LLM call:**

```rust
// In state.rs poll_output()
if is_idle && has_prompt && !session.response_buffer.is_empty() {
    // Check if we've already shown "summarizing" message
    if !session.is_summarizing {
        session.is_summarizing = true;
        return Ok(Some(PollResult::Summarizing));
    }

    // Do actual summarization
    let response = summarize_with_fallback(&query, &raw_response).await;
    session.is_summarizing = false;

    return Ok(Some(PollResult::Complete(response, message_id)));
}
```

3. **Update poll loop to show "Summarizing...":**

```rust
Ok(Some(PollResult::Summarizing)) => {
    if let Some(&msg_id) = progress_messages.get(&chat_id) {
        let _ = bot.edit_message_text(
            ChatId(chat_id),
            msg_id,
            "🤖 Summarizing output..."
        ).await;
    }
}
```

---

## Performance and UX Considerations

### Update Frequency

**Current Polling:** 500ms interval (POLL_INTERVAL_MS)

**Progress Update Strategy:**
- **Too frequent:** Spams Telegram API, rate limiting issues
- **Too rare:** User doesn't see progress

**Recommended:**
- Emit progress every 5 lines captured
- Telegram API rate limit: 30 messages/second per chat
- With 500ms polling, max 2 updates/second (well within limits)

---

### Message Update Latency

**Telegram `edit_message_text()` typical latency:** 100-300ms

**Impact:**
- Progress messages will lag slightly behind actual progress
- Not a problem for user perception (still feels responsive)

**Mitigation:**
- Don't update on every line (batch updates every 5 lines)
- Use async/await to prevent blocking

---

### Error Handling

**What if message edit fails?**

```rust
match bot.edit_message_text(ChatId(chat_id), msg_id, text).await {
    Ok(_) => {} // Success
    Err(e) => {
        warn!("Failed to update progress message: {}", e);
        // Remove from tracking, send new message on next update
        progress_messages.remove(&chat_id);
    }
}
```

**Common failures:**
- Message already deleted by user
- Message too old (>48 hours)
- Bot lacks permissions

**Strategy:** Log warning, continue processing, send new message on next update

---

## Testing Strategy

### Manual Testing

1. **Connect to project and send message:**
   ```
   /connect my-project
   Write a function to calculate fibonacci
   ```

2. **Observe:**
   - Progress message appears: "📥 Receiving...5 lines captured"
   - Updates as more output arrives: "📥 Receiving...10 lines captured"
   - Changes to: "🤖 Summarizing output..."
   - Final response replaces progress message

3. **Edge cases to test:**
   - Fast responses (<5 lines)
   - Slow responses (100+ lines)
   - Summarization disabled (no OpenRouter API key)
   - Multiple concurrent users

---

### Integration Testing

```rust
#[tokio::test]
async fn test_progress_messages() {
    let state = create_test_state();
    let chat_id = ChatId(12345);

    // Connect session
    state.connect(chat_id, "test-project").await.unwrap();

    // Send message
    state.send_message(chat_id, "test query", None).await.unwrap();

    // Poll multiple times to simulate progress
    let mut results = Vec::new();
    for _ in 0..10 {
        if let Ok(Some(result)) = state.poll_output(chat_id).await {
            results.push(result);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Verify we got progress updates
    assert!(results.iter().any(|r| matches!(r, PollResult::Progress(_))));

    // Verify final completion
    assert!(results.iter().any(|r| matches!(r, PollResult::Complete(_, _))));
}
```

---

## Summary and Next Steps

### Current State

**Progress messages do NOT exist:**
- User only sees typing indicator during collection
- Final summarized response appears when complete
- No intermediate feedback

**Key Components:**
- Output collection: `state.rs::poll_output()` and `session.rs::add_response_lines()`
- Summarization: `summarizer.rs::summarize_with_fallback()`
- Telegram polling: `bot.rs::poll_output_loop()`

---

### Implementation Path

**Minimal (Phase 1):**
1. Modify `UserSession` to track progress milestones
2. Change `poll_output()` return type to `Option<PollResult>`
3. Update `poll_output_loop()` to send/update progress messages using `bot.edit_message_text()`

**Estimated effort:** 2-3 hours
**Files changed:** 3 (state.rs, session.rs, bot.rs)
**Complexity:** Low

**Enhanced (Phase 2):**
1. Add "Summarizing..." state
2. Track when summarization starts
3. Update message before LLM call

**Estimated effort:** +1 hour
**Files changed:** Same 3 files
**Complexity:** Low

---

### Architecture Recommendations

**Use Option 1 (Progress in UserSession) because:**
- Minimal changes to existing code
- Clear ownership (session knows its own progress)
- Easy to test and debug
- No new concurrency primitives needed

**Avoid Option 3 (Channels) unless:**
- Planning to add many more progress types
- Want to support progress from other sources (not just output collection)
- Need to decouple Telegram logic from state management

---

### Key Design Decisions

1. **Update frequency:** Every 5 lines (prevents spam, stays within rate limits)
2. **Message strategy:** Edit single progress message (cleaner UX than multiple messages)
3. **Cleanup:** Delete progress message when sending final response
4. **Fallback:** If edit fails, send new message (graceful degradation)
5. **State tracking:** Store `progress_message_id` in `UserSession`

---

## Code Snippets for Implementation

### Complete Diff Preview

```diff
// crates/commander-telegram/src/session.rs
pub struct UserSession {
    // ... existing fields
+   pub progress_message_id: Option<MessageId>,
+   pub is_summarizing: bool,
}

impl UserSession {
+   pub fn should_emit_progress(&self) -> bool {
+       let count = self.response_buffer.len();
+       count > 0 && count % 5 == 0
+   }
+
+   pub fn get_progress_message(&self) -> String {
+       format!("📥 Receiving...{} lines captured", self.response_buffer.len())
+   }
}

// crates/commander-telegram/src/state.rs
+pub enum PollResult {
+    Progress(String),
+    Summarizing,
+    Complete(String, Option<MessageId>),
+}

-pub async fn poll_output(&self, chat_id: ChatId) -> Result<Option<(String, Option<MessageId>)>> {
+pub async fn poll_output(&self, chat_id: ChatId) -> Result<Option<PollResult>> {
     // ... existing code ...

     if current_output != session.last_output {
         let new_lines = find_new_lines(&session.last_output, &current_output);
         session.add_response_lines(new_lines);
         session.last_output = current_output.clone();
+
+        if session.should_emit_progress() {
+            return Ok(Some(PollResult::Progress(session.get_progress_message())));
+        }
     }

     if is_idle && has_prompt && !session.response_buffer.is_empty() {
+        if !session.is_summarizing {
+            session.is_summarizing = true;
+            return Ok(Some(PollResult::Summarizing));
+        }
+
         let raw_response = session.get_response();
         let query = session.pending_query.clone().unwrap_or_default();
         let message_id = session.pending_message_id;
         session.reset_response_state();

         let response = summarize_with_fallback(&query, &raw_response).await;
+        session.is_summarizing = false;
-        return Ok(Some((response, message_id)));
+        return Ok(Some(PollResult::Complete(response, message_id)));
     }

-    Ok(None)
+    Ok(Some(PollResult::None))
}

// crates/commander-telegram/src/bot.rs
async fn poll_output_loop(bot: Bot, state: Arc<TelegramState>) {
    let mut poll_interval = interval(Duration::from_millis(POLL_INTERVAL_MS));
+   let mut progress_messages: HashMap<i64, MessageId> = HashMap::new();

    loop {
        poll_interval.tick().await;
        let waiting_ids = state.get_waiting_chat_ids().await;

        for chat_id in waiting_ids {
-           let _ = bot.send_chat_action(ChatId(chat_id), ChatAction::Typing).await;

            match state.poll_output(ChatId(chat_id)).await {
+               Ok(Some(PollResult::Progress(text))) => {
+                   if let Some(&msg_id) = progress_messages.get(&chat_id) {
+                       let _ = bot.edit_message_text(ChatId(chat_id), msg_id, text).await;
+                   } else {
+                       if let Ok(msg) = bot.send_message(ChatId(chat_id), text).await {
+                           progress_messages.insert(chat_id, msg.id);
+                       }
+                   }
+               }
+               Ok(Some(PollResult::Summarizing)) => {
+                   if let Some(&msg_id) = progress_messages.get(&chat_id) {
+                       let _ = bot.edit_message_text(
+                           ChatId(chat_id),
+                           msg_id,
+                           "🤖 Summarizing output..."
+                       ).await;
+                   }
+               }
-               Ok(Some((response, message_id))) => {
+               Ok(Some(PollResult::Complete(response, message_id))) => {
+                   // Delete progress message
+                   if let Some(progress_msg_id) = progress_messages.remove(&chat_id) {
+                       let _ = bot.delete_message(ChatId(chat_id), progress_msg_id).await;
+                   }
+
                    let send_result = if let Some(msg_id) = message_id {
                        bot.send_message(ChatId(chat_id), &response)
                            .reply_parameters(ReplyParameters::new(msg_id))
                            .await
                    } else {
                        bot.send_message(ChatId(chat_id), &response).await
                    };

                    if let Err(e) = send_result {
                        warn!(chat_id = %chat_id, error = %e, "Failed to send response");
                    }
                }
-               Ok(None) => {}
+               Ok(Some(PollResult::None)) => {
+                   // Still processing, show typing indicator
+                   let _ = bot.send_chat_action(ChatId(chat_id), ChatAction::Typing).await;
+               }
                Err(e) => {
                    warn!(chat_id = %chat_id, error = %e, "Error polling output");
                }
            }
        }
    }
}
```

---

## Conclusion

Implementing progress messages requires:

1. **Tracking progress state** in `UserSession`
2. **Modifying poll_output()** to return progress updates
3. **Updating poll_output_loop()** to send/edit Telegram messages
4. **Using bot.edit_message_text()** to update a single message

This provides a clean, user-friendly experience where progress is visible without spamming multiple messages.

**Recommended approach:** Start with Phase 1 (minimal progress messages) to validate the UX, then add Phase 2 (summarization progress) if users find it valuable.
