# GUI UX Improvements - Test Plan

## Test Environment Setup

### Prerequisites
```bash
# Ensure tmux is installed
which tmux

# Build the GUI
cd /Users/masa/Projects/ai-commander/crates/commander-gui
cargo tauri dev
```

### Create Test Sessions
```bash
# Create test sessions with "commander-" prefix
tmux new-session -d -s commander-test1
tmux new-session -d -s commander-test2
tmux new-session -d -s commander-production

# Verify they exist
tmux list-sessions
```

## Test Cases

### TC-1: Session Name Display

**Feature**: Strip "commander-" prefix from session names

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Open GUI | GUI loads successfully | [ ] |
| 2    | View session list | Sessions appear in left sidebar | [ ] |
| 3    | Check session names | "commander-test1" displays as "test1" | [ ] |
| 4    |                     | "commander-test2" displays as "test2" | [ ] |
| 5    |                     | "commander-production" displays as "production" | [ ] |
| 6    | Click on session | Connection succeeds (uses full name) | [ ] |

**Edge Cases**:
- [ ] Session without "commander-" prefix displays unchanged
- [ ] Session with multiple hyphens: "commander-my-project" → "my-project"
- [ ] Session name "commander" (edge case) displays as empty or "commander"

### TC-2: Session Action Tabs - Appearance

**Feature**: Tabs appear when session is connected

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Open GUI with no session connected | No tabs visible | [ ] |
| 2    | Click on "test1" session | Session connects | [ ] |
| 3    | Check tab bar | Three tabs appear: Status, Stop, Disconnect | [ ] |
| 4    | Check tab styling | Tabs have white background with gray border | [ ] |
| 5    | Hover over Status tab | Background changes to light gray | [ ] |
| 6    | Check tooltips | Each tab shows helpful tooltip | [ ] |

### TC-3: Status Tab Functionality

**Feature**: Status tab sends /status command

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Connect to session "test1" | Tabs appear | [ ] |
| 2    | Click [Status] button | Button shows loading state | [ ] |
| 3    | Check chat | Message "/status" appears as sent | [ ] |
| 4    | Wait for response | Response appears in chat (if session supports it) | [ ] |
| 5    | Click [Status] again | Command can be sent multiple times | [ ] |

**Edge Cases**:
- [ ] Click Status rapidly → Loading state prevents duplicate sends
- [ ] Session disconnects mid-request → Error message appears

### TC-4: Stop Tab Functionality

**Feature**: Stop tab destroys session with confirmation

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Connect to session "test2" | Tabs appear | [ ] |
| 2    | Click [Stop] button | Confirmation dialog appears | [ ] |
| 3    | Check dialog text | Shows "Are you sure..." with session name | [ ] |
| 4    | Click [Cancel] | Dialog closes, session still connected | [ ] |
| 5    | Click [Stop] again | Dialog appears again | [ ] |
| 6    | Click [Stop Session] | System message confirms session stopped | [ ] |
| 7    | Check session list | Session no longer appears (or marked stopped) | [ ] |
| 8    | Check tabs | Tabs disappear after stop | [ ] |
| 9    | Verify with tmux | `tmux ls` does not show test2 | [ ] |

**Edge Cases**:
- [ ] Session already stopped → Error message shown
- [ ] Stop while sending message → Handled gracefully

### TC-5: Disconnect Tab Functionality

**Feature**: Disconnect tab disconnects from session without destroying it

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Connect to session "production" | Tabs appear | [ ] |
| 2    | Send a message | Message appears in chat | [ ] |
| 3    | Click [Disconnect] button | System message confirms disconnect | [ ] |
| 4    | Check tabs | Tabs disappear | [ ] |
| 5    | Check chat | Messages cleared (fresh state) | [ ] |
| 6    | Check session list | Session still appears in list | [ ] |
| 7    | Verify with tmux | `tmux ls` still shows commander-production | [ ] |
| 8    | Reconnect to session | Can reconnect successfully | [ ] |

**Edge Cases**:
- [ ] Disconnect twice → Second click has no effect (already disconnected)
- [ ] Disconnect with pending message → Handled gracefully

### TC-6: Message Routing

**Feature**: Messages correctly route to tmux session

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Try to send message with no session | System message: "Not connected to a session" | [ ] |
| 2    | Connect to session | Input field enabled | [ ] |
| 3    | Type "hello" and send | Message appears as sent | [ ] |
| 4    | Check tmux directly | `tmux capture-pane -p -t commander-test1` shows message | [ ] |
| 5    | Send "/status" via input | Command sent to tmux | [ ] |
| 6    | Send message with special chars | Message sent correctly (e.g., "test!@#$") | [ ] |

**Edge Cases**:
- [ ] Empty message → Send button disabled
- [ ] Very long message → Sent without truncation
- [ ] Message with newlines → Handled appropriately

### TC-7: Error Handling

**Feature**: User-friendly error messages

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Connect to session | Connected | [ ] |
| 2    | Stop session via tmux CLI | `tmux kill-session -t commander-test1` | [ ] |
| 3    | Try to send message in GUI | System message: "Session not found" | [ ] |
| 4    | Check error formatting | No alert() dialog, error in chat | [ ] |
| 5    | Try to stop non-existent session | Error message shown | [ ] |

**Error Message Checklist**:
- [ ] "Not connected to a session" - when sending without connection
- [ ] "Session 'X' not found" - when session doesn't exist
- [ ] "Failed to send message: Y" - when send fails
- [ ] "Failed to stop session: Y" - when stop fails
- [ ] "Failed to disconnect: Y" - when disconnect fails

### TC-8: Visual Consistency

**Feature**: UI remains consistent and responsive

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Resize window | Layout adapts responsively | [ ] |
| 2    | Connect session | Chat area adjusts for tabs | [ ] |
| 3    | Check tab alignment | Tabs aligned properly at top | [ ] |
| 4    | Long session name | Name doesn't overflow or break layout | [ ] |
| 5    | Many messages | Chat scrolls correctly with tabs present | [ ] |

### TC-9: Integration Test

**Feature**: End-to-end workflow

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Start GUI | Loads successfully | [ ] |
| 2    | View 3 sessions | All show without "commander-" prefix | [ ] |
| 3    | Connect to session A | Tabs appear | [ ] |
| 4    | Click Status | /status sent | [ ] |
| 5    | Send custom message | Message sent | [ ] |
| 6    | Disconnect | Tabs disappear, session persists | [ ] |
| 7    | Connect to session B | Tabs reappear | [ ] |
| 8    | Stop session B | Confirmation → Session destroyed | [ ] |
| 9    | Check session list | Only 2 sessions remain | [ ] |
| 10   | Connect to session C | Works normally | [ ] |

### TC-10: Performance Test

**Feature**: UI remains responsive

| Step | Action | Expected Result | Status |
|------|--------|-----------------|--------|
| 1    | Create 10 sessions | All load in session list | [ ] |
| 2    | Click Status 20 times rapidly | No UI freeze, loading states work | [ ] |
| 3    | Send 50 messages | Chat updates smoothly | [ ] |
| 4    | Switch sessions 10 times | No memory leaks or slowdown | [ ] |

## Regression Tests

Ensure existing functionality still works:

- [ ] Session list auto-refresh (every 2 seconds)
- [ ] Message timestamps display correctly
- [ ] Scroll-to-bottom behavior
- [ ] Scroll-up shows scroll button
- [ ] Bot status indicator works
- [ ] Enter key sends message
- [ ] Shift+Enter creates new line (if supported)

## Browser Console Checks

Open DevTools and verify:

- [ ] No JavaScript errors in console
- [ ] No React/Svelte warnings
- [ ] WebSocket connections stable (if applicable)

## Manual Testing Checklist

### Before Release
- [ ] All test cases passed
- [ ] No console errors
- [ ] Code review completed
- [ ] Documentation updated
- [ ] Build succeeds: `cargo build --release -p commander-gui`
- [ ] UI build succeeds: `npm run build`

### Test on Multiple Platforms (if applicable)
- [ ] macOS
- [ ] Linux
- [ ] Windows

## Bug Report Template

If issues found during testing:

```markdown
**Issue**: Brief description
**Steps to Reproduce**:
1. Step one
2. Step two
3. ...

**Expected**: What should happen
**Actual**: What actually happened
**Screenshots**: If applicable
**Console Logs**: Any errors in console
**Test Case**: TC-X from test plan
```

## Success Criteria

All test cases must pass:
- ✅ TC-1: Session names display without prefix
- ✅ TC-2: Tabs appear when connected
- ✅ TC-3: Status tab works correctly
- ✅ TC-4: Stop tab works with confirmation
- ✅ TC-5: Disconnect tab works without destroying session
- ✅ TC-6: Messages route to tmux correctly
- ✅ TC-7: Error messages are user-friendly
- ✅ TC-8: Visual consistency maintained
- ✅ TC-9: End-to-end workflow succeeds
- ✅ TC-10: Performance is acceptable

## Sign-Off

- [ ] Developer: Changes implemented and unit tested
- [ ] QA: Test plan executed, all tests passed
- [ ] Product: UX improvements validated
- [ ] Ready for deployment
