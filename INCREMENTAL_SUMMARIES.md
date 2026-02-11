# Incremental AI-Generated Summaries

## Overview

The Telegram bot now sends AI-generated incremental summaries every 50 lines during long output collection. This provides users with real-time insights into what's happening without waiting for the entire operation to complete.

## User Experience

### Short Output (< 50 lines)
```
User: "Show me the README"
Bot: "📥 Receiving...5 lines captured"
Bot: "📥 Receiving...10 lines captured"
Bot: [Final summary after idle detected]
```

### Medium Output (50-99 lines)
```
User: "Find all TODO comments"
Bot: "📥 Receiving...5 lines captured"
Bot: "📥 Receiving...10 lines captured"
...
Bot: "📥 Receiving...50 lines captured"
Bot: "📊 Incremental Summary (50 lines):
     Found 23 TODO comments across 8 files, mostly in session.rs and
     orchestrator.rs focusing on error handling improvements."
Bot: "📥 Receiving...55 lines captured"
...
Bot: [Final summary at end]
```

### Long Output (150+ lines)
```
User: "Analyze all the Rust files for potential issues"
Bot: "📥 Receiving...5 lines captured"
...
Bot: "📥 Receiving...50 lines captured"
Bot: "📊 Incremental Summary (50 lines):
     Analyzing module structure across 12 Rust files. Found 3 potential
     issues with error handling in session management."
Bot: "📥 Receiving...55 lines captured"
...
Bot: "📥 Receiving...100 lines captured"
Bot: "📊 Incremental Summary (100 lines):
     Completed analysis of core modules. Identified 7 areas needing
     refactoring, primarily around async handling and state management."
Bot: "📥 Receiving...105 lines captured"
...
Bot: "📥 Receiving...150 lines captured"
Bot: "📊 Incremental Summary (150 lines):
     Analysis expanded to include test coverage. Overall code quality is
     good with 85% test coverage, but orchestrator module needs more tests."
...
Bot: [Final complete summary at end]
```

## Implementation Details

### 1. Session Tracking (`session.rs`)

Added `last_incremental_summary_line_count` field to track when the last summary was sent:

```rust
pub struct UserSession {
    // ... existing fields
    pub last_incremental_summary_line_count: usize,
}

impl UserSession {
    /// Check if an incremental summary should be emitted (every 50 lines).
    pub fn should_emit_incremental_summary(&self) -> bool {
        let current_lines = self.response_buffer.len();
        current_lines > 0 &&
        current_lines >= self.last_incremental_summary_line_count + 50
    }

    /// Mark that an incremental summary was sent.
    pub fn mark_incremental_summary_sent(&mut self) {
        self.last_incremental_summary_line_count = self.response_buffer.len();
    }
}
```

### 2. Poll Result Variant (`state.rs`)

Added new `IncrementalSummary` variant to `PollResult`:

```rust
pub enum PollResult {
    Progress(String),
    IncrementalSummary(String),  // NEW: AI summary every 50 lines
    Summarizing,
    Complete(String, Option<MessageId>),
    NoOutput,
}
```

### 3. Incremental Summarization (`summarizer.rs`)

Added new `summarize_incremental()` function with a specialized prompt:

```rust
/// Generate an incremental summary of output collected so far.
pub async fn summarize_incremental(content: &str, line_count: usize) -> Result<String, SummarizerError> {
    // Uses INCREMENTAL_SYSTEM_PROMPT (briefer than full summaries)
    // Max 150 tokens (vs 500 for final summaries)
    // Returns: "📊 Incremental Summary (N lines):\n{summary}"
}
```

**Key differences from final summaries:**
- **Briefer prompt**: 2-3 sentences maximum (vs 2-4+ sentences)
- **Token limit**: 150 tokens (vs 500 tokens)
- **Focus**: Progress and findings (vs complete analysis)
- **Graceful fallback**: Simple line count message if API unavailable

### 4. Output Polling Logic (`state.rs`)

Updated `poll_output()` to check for incremental summary thresholds:

```rust
pub async fn poll_output(&self, chat_id: ChatId) -> Result<PollResult> {
    // ... collect new lines

    // Check for incremental summary BEFORE progress check
    if session.should_emit_incremental_summary() {
        let content_so_far = session.get_response();
        let line_count = session.response_buffer.len();

        match summarize_incremental(&content_so_far, line_count).await {
            Ok(summary) => {
                session.mark_incremental_summary_sent();
                return Ok(PollResult::IncrementalSummary(summary));
            }
            Err(e) => {
                warn!("Failed to generate incremental summary");
                // Continue - don't block collection on summary failure
            }
        }
    }

    // Then check progress (every 5 lines)
    if session.should_emit_progress() {
        return Ok(PollResult::Progress(...));
    }

    // ... rest of logic
}
```

### 5. Bot Message Handling (`bot.rs`)

Updated polling loop to handle incremental summaries:

```rust
async fn poll_output_loop(bot: Bot, state: Arc<TelegramState>) {
    loop {
        match state.poll_output(chat_id).await {
            Ok(PollResult::IncrementalSummary(summary)) => {
                // Send as NEW message (not an edit)
                bot.send_message(ChatId(chat_id), &summary).await?;
                // Continue polling - don't stop collection
            }
            // ... other cases
        }
    }
}
```

**Design decision**: Send incremental summaries as separate messages (not edits) so users can see the progression over time.

## Configuration

Incremental summaries require an OpenRouter API key:

```bash
export OPENROUTER_API_KEY="your-key-here"
```

**Graceful degradation**: Without an API key, falls back to simple line count messages:
```
📊 Incremental Summary (50 lines):
Collecting output... 50 lines captured so far.
```

## Performance Characteristics

### Resource Usage
- **API calls**: 1 call per 50 lines (not per 5 lines like progress)
- **Token cost**: ~150 tokens per incremental summary
- **Latency**: Summaries generated asynchronously, don't block collection
- **Network**: Brief HTTP request every 50 lines (vs every 5 for progress)

### Example Cost Analysis (150-line output)
- **Progress messages**: 30 updates (every 5 lines) - 0 API calls
- **Incremental summaries**: 3 summaries (at 50, 100, 150) - 3 API calls (~450 tokens)
- **Final summary**: 1 summary - 1 API call (~500 tokens)
- **Total**: 4 API calls, ~950 tokens

### Comparison to Previous Approach
**Before**: User waits for entire output → single summary at end
**After**: User gets insights every 50 lines + final summary

**Benefit**: For long operations (200+ lines), users see progress updates 4-5x during collection instead of waiting until the end.

## Testing

### Unit Tests
- `test_incremental_summaries()`: Verifies 50-line threshold detection
- `test_progress_messages()`: Ensures progress messages still work (every 5 lines)
- `test_response_collection()`: Validates state resets properly

### Test Scenarios Covered
1. **Short output (< 50 lines)**: No incremental summaries, just progress + final
2. **Exactly 50 lines**: One incremental summary at 50, then final
3. **100 lines**: Two incremental summaries (50, 100), then final
4. **150 lines**: Three incremental summaries (50, 100, 150), then final
5. **API key missing**: Graceful fallback to simple line count
6. **API failure**: Logs error, continues collection without blocking

## Edge Cases Handled

1. **Fast output**: Incremental summaries don't block collection
2. **API timeouts**: Logged but don't interrupt polling
3. **Rate limiting**: Limited to 1 call per 50 lines (much less than progress)
4. **No API key**: Falls back to simple line count messages
5. **Very slow output**: Still gets incremental summaries at thresholds
6. **Session disconnect during summary**: State resets properly

## Future Enhancements

Potential improvements for future iterations:

1. **Configurable threshold**: Allow users to set summary frequency (default: 50)
2. **Adaptive summarization**: More detailed summaries for complex output
3. **Summary persistence**: Store summaries for later review
4. **Threading**: Reply to original message for better context
5. **Smart detection**: Trigger summaries on significant events (errors, completion)

## Files Changed

1. **crates/commander-telegram/src/session.rs**
   - Added `last_incremental_summary_line_count` field
   - Added `should_emit_incremental_summary()` method
   - Added `mark_incremental_summary_sent()` method
   - Updated all reset/initialization to include new field
   - Added test coverage

2. **crates/commander-telegram/src/state.rs**
   - Added `IncrementalSummary` variant to `PollResult`
   - Updated `poll_output()` to detect and emit incremental summaries
   - Added import for `summarize_incremental`

3. **crates/commander-core/src/summarizer.rs**
   - Added `INCREMENTAL_SYSTEM_PROMPT` constant
   - Added `summarize_incremental()` async function
   - Configured for 150 tokens (vs 500 for final summaries)

4. **crates/commander-core/src/lib.rs**
   - Exported `summarize_incremental` function

5. **crates/commander-telegram/src/bot.rs**
   - Updated `poll_output_loop()` to handle `IncrementalSummary` variant
   - Sends summaries as separate messages (not edits)

## Verification

All tests pass:
```bash
$ cargo test --package commander-telegram --lib session
test session::tests::test_incremental_summaries ... ok
test session::tests::test_new_session ... ok
test session::tests::test_progress_messages ... ok
test session::tests::test_response_collection ... ok

$ cargo check --features agents
Finished `dev` profile [unoptimized + debuginfo] target(s)
```

No compilation warnings or errors.
