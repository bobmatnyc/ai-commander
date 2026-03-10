# Telegram Notification Link Text Testing

## Overview
Testing context-aware link text in Telegram notifications based on notification type.

## Implementation
**File:** `crates/commander-telegram/src/bot.rs` (lines 531-541)

The bot now detects notification message content and displays appropriate link text:
- "resumed work" → "Resume {session}"
- "waiting" → "Continue {session}"
- "paused" → "Continue {session}"
- "ready" → "Open {session}"
- "started" → "Open {session}"
- Default → "Connect to {session}"

## Test Cases

### 1. Resume Notification
**Trigger:** Session resumes work after being paused
**Expected Message:** `Session "izzie" resumed work`
**Expected Link Text:** `Resume izzie`
**How to Test:**
```bash
# In session
commander pause

# Resume the session (generates notification)
commander resume
```

### 2. Waiting Notification
**Trigger:** Session is waiting for user input
**Expected Message:** `A session is waiting for your input: "izzie"`
**Expected Link Text:** `Continue izzie`
**How to Test:**
```bash
# Create a session that needs input
commander create izzie
# Let it wait for input
```

### 3. Ready Notification
**Trigger:** Session becomes ready for input
**Expected Message:** `Session "izzie" is ready for input`
**Expected Link Text:** `Open izzie`
**How to Test:**
```bash
# Create new session
commander create izzie
```

### 4. Paused Notification (if implemented)
**Trigger:** Session is paused
**Expected Message:** Contains "paused"
**Expected Link Text:** `Continue izzie`

### 5. Default Fallback
**Trigger:** Unknown notification type
**Expected Link Text:** `Connect to izzie`

## Verification Steps

1. **Visual Check:** Click notification in Telegram, verify link text matches expected
2. **Link Functionality:** Verify links still open the correct session
3. **Multiple Sessions:** Test with multiple sessions to ensure display name extraction works
4. **Edge Cases:**
   - Session names with special characters
   - Session names with/without "commander-" prefix

## Acceptance Criteria

- ✅ Code compiles without errors
- ✅ Resume notifications show "Resume {session}"
- ✅ Waiting notifications show "Continue {session}"
- ✅ Ready notifications show "Open {session}"
- ✅ All links work correctly in Telegram
- ✅ Display names properly strip "commander-" prefix

## Related Files

- `crates/commander-telegram/src/bot.rs` - Main notification handler
- `crates/commander-telegram/src/notifications.rs` - Notification message generators
- `docs/research/option-selection-analysis-2026-02-21.md` - Research identifying the issue
