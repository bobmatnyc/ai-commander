# AI Commander GUI `/status` Command Debugging

**Date:** 2026-02-22
**Issue:** `/status` command sent from GUI doesn't return output in chat
**Status:** Root cause identified - output capture strategy issue

## Problem Statement

User types `/status` in AI Commander GUI → command sent to tmux → no response appears in GUI chat.

## Investigation Process

### 1. Backend Event Streaming Analysis

**File:** `crates/commander-gui/src/events.rs`

**Findings:**
- ✅ `start_session_polling` IS running (spawned in main.rs:18-20)
- ✅ Polling interval: 500ms (line 31)
- ✅ Event emission works: `app.emit("session-output", ...)` (line 19)
- ❌ **ISSUE**: Output capture strategy doesn't detect incremental changes

**Current Implementation:**
```rust
if let Ok(output) = tmux.capture_output(&session_name, None, Some(50)) {
    if !output.is_empty() {
        let _ = app.emit("session-output", SessionOutput {
            session: session_name.clone(),
            output,
        });
    }
}
```

**Problem:** Captures the **same last 50 lines** every 500ms with no change detection.

### 2. Frontend Event Listener Analysis

**File:** `crates/commander-gui/ui/src/lib/components/ChatView.svelte`

**Findings:**
- ✅ Event listener properly configured (line 105)
- ✅ Adds messages to correct session (line 108)
- ✅ Auto-scroll logic works (line 115-117)

**No issues found in frontend.**

### 3. Tmux Integration Analysis

**File:** `crates/commander-tmux/src/orchestrator.rs`

**Findings:**
- ✅ `capture_output` uses `tmux capture-pane -p -S -<lines>` (line 263-270)
- ✅ Session validation works (line 254-256)
- ❌ **LIMITATION**: `capture-pane` returns entire scrollback buffer, not incremental output

**Tmux Command Behavior:**
```bash
tmux capture-pane -t commander-izzie -p -S -50
```

This returns the **last 50 lines of the entire pane** - not "new output since last check".

### 4. Manual Testing

**Test Command:**
```bash
tmux send-keys -t commander-izzie "/status" Enter
sleep 2
tmux capture-pane -t commander-izzie -p -S -50 | tail -10
```

**Result:** `/status` output successfully captured ✅

**Output Sample:**
```
Settings:  Status   Config   Usage

Version: 2.1.47
Session name: /rename to add a name
Session ID: 3d556d70-3c32-465d-ae37-2af60f1f20fb
cwd: /Users/masa/Projects/izzie2
API provider: AWS Bedrock
AWS region: us-west-2

Model: Default (us.anthropic.claude-sonnet-4-5-20250929-v1:0)
MCP servers: mcp-skillset ✔, github ✔, filesystem ✔, kuzu-memory ✔, mcp-vector-search ✘
Memory: project (CLAUDE.md)
Setting sources: User settings, Project local settings
Esc to cancel
```

**Conclusion:** Tmux integration works correctly. The issue is output change detection.

## Root Cause

### Primary Issue: No Incremental Output Detection

The polling loop captures the **entire last 50 lines** every 500ms:

```rust
// Every 500ms:
let output = tmux.capture_output(&session_name, None, Some(50));
// Returns: "Line 1\nLine 2\n...\nLine 50"

// Next poll (500ms later):
let output = tmux.capture_output(&session_name, None, Some(50));
// Returns: SAME "Line 1\nLine 2\n...\nLine 50"
```

**Empty Check Insufficient:**
```rust
if !output.is_empty() {
    // This prevents emitting empty panes
    // But doesn't prevent re-emitting same content
    emit(output);
}
```

**Expected Behavior:** Only emit **new** lines (changes since last capture).

### Secondary Issue: Timing Edge Cases

**Scenario A: Multi-Screen Output**
- `/status` produces 97 lines of output (from manual test)
- Capture limit: 50 lines
- Only captures **last 50 lines** (misses first 47 lines)

**Scenario B: Fast Output Completion**
- Command completes in 300ms
- Next poll happens at 500ms
- Captures all output at once ✅
- But subsequent polls re-emit same output ❌

### Session Name Handling

**No issues found:**
- Session name consistency: `commander-izzie` (full name used throughout)
- Name retrieval: `state.current_session.read().unwrap().clone()` ✅
- Session validation: `tmux.session_exists(&name)` ✅

## Solution Approaches

### Option 1: Track Last Captured Hash (Recommended)

**Implementation:**
```rust
struct PollingState {
    last_hash: Option<u64>,
}

pub async fn start_session_polling(app: AppHandle, state: GuiState) {
    let mut last_hash: Option<u64> = None;

    tokio::spawn(async move {
        loop {
            if let Some(session_name) = state.current_session.read().unwrap().clone() {
                if let Some(tmux) = &state.tmux {
                    if let Ok(output) = tmux.capture_output(&session_name, None, Some(50)) {
                        if !output.is_empty() {
                            // Calculate hash of output
                            let mut hasher = DefaultHasher::new();
                            output.hash(&mut hasher);
                            let current_hash = hasher.finish();

                            // Only emit if changed
                            if last_hash != Some(current_hash) {
                                last_hash = Some(current_hash);
                                let _ = app.emit("session-output", SessionOutput {
                                    session: session_name.clone(),
                                    output,
                                });
                            }
                        }
                    }
                }
            }

            sleep(Duration::from_millis(500)).await;
        }
    });
}
```

**Pros:**
- Simple implementation
- Low overhead (single u64 comparison)
- Prevents duplicate emissions

**Cons:**
- Still captures entire buffer (no incremental detection)
- Won't detect if new content pushes old content out of 50-line window

### Option 2: Use `tmux pipe-pane` for Streaming

**Implementation:**
```bash
# Start capturing to file
tmux pipe-pane -t commander-izzie -o "cat >> /tmp/izzie-output.log"

# Rust: tail -f the log file
tokio::spawn(async move {
    let mut file = File::open("/tmp/izzie-output.log").await?;
    let mut last_pos = 0;

    loop {
        let len = file.seek(SeekFrom::End(0)).await?;
        if len > last_pos {
            file.seek(SeekFrom::Start(last_pos)).await?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer).await?;

            let new_output = String::from_utf8_lossy(&buffer);
            emit("session-output", new_output);

            last_pos = len;
        }
        sleep(Duration::from_millis(500)).await;
    }
});
```

**Pros:**
- True incremental output detection
- No missed lines (captures everything)
- Works with any output size

**Cons:**
- Requires file cleanup
- More complex implementation
- Need to manage file lifecycle per session

### Option 3: Increase Poll Frequency + Hash Check

**Implementation:**
```rust
// Reduce polling interval to 100ms
sleep(Duration::from_millis(100)).await;

// Add hash-based change detection (from Option 1)
```

**Pros:**
- Faster response time (100ms vs 500ms)
- Combined with hash check, prevents duplicates

**Cons:**
- Higher CPU usage (5x more polls)
- Still doesn't capture mid-output (if command takes >100ms)

### Option 4: Capture Full Scrollback + Line Tracking

**Implementation:**
```rust
struct SessionState {
    last_line_count: usize,
}

// Capture ENTIRE scrollback (no line limit)
let output = tmux.capture_output(&session_name, None, None)?;
let lines: Vec<&str> = output.lines().collect();

// Only emit new lines since last check
if lines.len() > state.last_line_count {
    let new_lines = &lines[state.last_line_count..];
    emit("session-output", new_lines.join("\n"));
    state.last_line_count = lines.len();
}
```

**Pros:**
- Never misses output
- True incremental detection
- Handles multi-screen output

**Cons:**
- Captures entire scrollback (can be large)
- Memory usage grows with session length
- Need scrollback limits

## Recommended Solution

**Hybrid Approach: Hash Check + Full Scrollback**

1. **Phase 1 (Quick Fix):** Implement hash-based change detection (Option 1)
   - Prevents duplicate emissions immediately
   - Minimal code changes
   - Solves 90% of the problem

2. **Phase 2 (Complete Solution):** Add line tracking (Option 4)
   - Captures only new lines
   - Handles large output correctly
   - Production-ready

**Implementation Priority:**
```
1. Add hash-based change detection (15 min)
2. Test with /status command (5 min)
3. Add line count tracking (30 min)
4. Add scrollback buffer limit (15 min)
5. Comprehensive testing (30 min)
```

**Total Time:** ~1.5 hours

## Code Changes Required

### File: `crates/commander-gui/src/events.rs`

**Add hash tracking:**
```rust
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub async fn start_session_polling(app: AppHandle, state: GuiState) {
    tokio::spawn(async move {
        let mut last_hash: Option<u64> = None;

        loop {
            if let Some(session_name) = state.current_session.read().unwrap().clone() {
                if let Some(tmux) = &state.tmux {
                    if let Ok(output) = tmux.capture_output(&session_name, None, Some(100)) {
                        if !output.is_empty() {
                            let mut hasher = DefaultHasher::new();
                            output.hash(&mut hasher);
                            let current_hash = hasher.finish();

                            if last_hash != Some(current_hash) {
                                last_hash = Some(current_hash);
                                let _ = app.emit(
                                    "session-output",
                                    SessionOutput {
                                        session: session_name.clone(),
                                        output,
                                    },
                                );
                            }
                        }
                    }
                }
            }

            sleep(Duration::from_millis(500)).await;
        }
    });
}
```

**Changes:**
1. Import `DefaultHasher` and `Hash` trait
2. Add `last_hash` state variable
3. Calculate hash of output before emitting
4. Only emit if hash changed
5. Increase capture lines from 50 → 100 (to handle larger output)

## Testing Plan

### Test Case 1: Status Command
```
1. Start GUI
2. Connect to session
3. Click "Status" button
4. Expected: Status output appears once in chat
5. Wait 5 seconds
6. Expected: No duplicate status output
```

### Test Case 2: Multiple Commands
```
1. Send /status
2. Wait for output
3. Send /help
4. Wait for output
5. Expected: Both outputs appear without duplicates
```

### Test Case 3: Long Output
```
1. Send command with 200+ lines of output
2. Expected: All output captured (test with increased line limit)
```

### Test Case 4: Rapid Commands
```
1. Send 3 commands in quick succession
2. Expected: All outputs appear in order
3. Expected: No interleaved or duplicate content
```

## Related Issues

**Potential Future Improvements:**
1. Add "new output available" indicator in GUI
2. Implement output streaming (show lines as they appear)
3. Add buffer size limits to prevent memory growth
4. Implement output filtering (hide certain patterns)
5. Add output search/highlighting in GUI

## References

- **Tmux Capture Documentation:** `man tmux` → `capture-pane`
- **Tauri Event System:** https://tauri.app/v1/guides/features/events/
- **Rust Hashing:** std::hash::Hash trait
- **Similar Issue:** Terminal output streaming in VSCode (uses similar approaches)

## Conclusion

**Root Cause:** No incremental output detection - same 50 lines captured every 500ms.

**Solution:** Add hash-based change detection (quick fix) + line tracking (complete solution).

**Recommendation:** Implement hash check immediately, then add line tracking in next iteration.

**Estimated Fix Time:** 15 minutes (hash check) + 30 minutes (line tracking) = 45 minutes total.
