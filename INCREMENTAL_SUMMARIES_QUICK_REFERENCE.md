# Incremental Summaries - Quick Reference

## How It Works

```
Lines 1-49:   Progress updates every 5 lines (📥 Receiving...N lines)
Line 50:      First incremental summary (📊 Incremental Summary)
Lines 51-99:  Progress updates continue
Line 100:     Second incremental summary
Lines 101-149: Progress updates continue
Line 150:     Third incremental summary
...and so on...
Final:        Complete summary when idle detected
```

## User Experience Timeline

```
0:00 - User sends: "Analyze all Rust files"
0:01 - Bot: "📥 Receiving...5 lines captured"
0:02 - Bot: "📥 Receiving...10 lines captured"
...
0:10 - Bot: "📥 Receiving...50 lines captured"
0:11 - Bot: "📊 Incremental Summary (50 lines):
            Analyzing module structure across 12 files..."
0:12 - Bot: "📥 Receiving...55 lines captured"
...
0:20 - Bot: "📥 Receiving...100 lines captured"
0:21 - Bot: "📊 Incremental Summary (100 lines):
            Completed core analysis, identified 7 refactoring areas..."
...
0:30 - [Final complete summary]
```

## Key Design Decisions

1. **Threshold: 50 lines**
   - Not too frequent (avoids spam)
   - Not too rare (provides timely insights)
   - Progress messages still every 5 lines for granular feedback

2. **Brief summaries (2-3 sentences)**
   - Quick to generate (150 tokens)
   - Easy to scan on mobile
   - Focuses on findings, not process

3. **Separate messages (not edits)**
   - Shows progression over time
   - User can scroll back to see earlier summaries
   - Clear separation from progress updates

4. **Graceful fallback**
   - Without API key: "Collecting output... N lines captured so far."
   - On API error: Log warning, continue collection
   - Never blocks output collection

5. **Async generation**
   - Summaries generated in background
   - Doesn't slow down output collection
   - Continues polling while LLM processes

## Implementation Pattern

```rust
// In poll_output() - check in this order:
1. Check for incremental summary (every 50 lines)
   → Generate summary asynchronously
   → Return PollResult::IncrementalSummary
   → Continue polling

2. Check for progress update (every 5 lines)
   → Return PollResult::Progress
   → Continue polling

3. Check for completion (idle + prompt detected)
   → Return PollResult::Summarizing (first time)
   → Generate final summary
   → Return PollResult::Complete
   → Stop polling
```

## Testing Scenarios

| Output Length | Progress Messages | Incremental Summaries | Final Summary |
|---------------|-------------------|------------------------|---------------|
| 25 lines      | 5 (every 5)      | 0                      | 1             |
| 50 lines      | 10 (every 5)     | 1 (at 50)              | 1             |
| 100 lines     | 20 (every 5)     | 2 (at 50, 100)         | 1             |
| 150 lines     | 30 (every 5)     | 3 (at 50, 100, 150)    | 1             |
| 200 lines     | 40 (every 5)     | 4 (at 50, 100, 150, 200) | 1           |

## API Cost Estimate

For a 150-line output:
- **Progress messages**: FREE (no API calls)
- **Incremental summaries**: 3 calls × ~150 tokens = 450 tokens
- **Final summary**: 1 call × ~500 tokens = 500 tokens
- **Total**: ~950 tokens (~$0.001 with Claude Sonnet 4)

## Configuration

```bash
# Required for AI summaries
export OPENROUTER_API_KEY="sk-or-..."

# Optional: Choose model (default: anthropic/claude-sonnet-4)
export OPENROUTER_MODEL="anthropic/claude-opus-4-6"
```

## Debugging

Check logs for incremental summary activity:
```bash
# Look for these log messages:
"Incremental summary sent"              # Success
"Failed to generate incremental summary" # API error (non-blocking)
```

## When Summaries Trigger

```rust
// Triggers at: 50, 100, 150, 200, ...
current_lines >= last_summary_line_count + 50
```

## Message Format

**Progress (every 5 lines):**
```
📥 Receiving...50 lines captured
```

**Incremental Summary (every 50 lines):**
```
📊 Incremental Summary (50 lines):
Found 23 TODO comments across 8 files, mostly in session.rs...
```

**Final Summary (at end):**
```
Analysis complete. Found 42 files with 87 potential improvements...
[Detailed summary based on full output]
```

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| API key missing | Falls back to "Collecting output... N lines captured" |
| API timeout | Logs error, continues collecting |
| Very fast output | Summaries generated asynchronously, don't block |
| Session disconnect | State resets properly (last_incremental_summary_line_count = 0) |
| Exactly 50 lines | Summary at 50, then final summary |
| 49 lines | No incremental summary, just final |
| 51 lines | Summary at 50, then final summary |

## Related Code

- **Session tracking**: `crates/commander-telegram/src/session.rs`
- **Poll logic**: `crates/commander-telegram/src/state.rs` (poll_output)
- **Bot handling**: `crates/commander-telegram/src/bot.rs` (poll_output_loop)
- **Summarization**: `crates/commander-core/src/summarizer.rs` (summarize_incremental)
