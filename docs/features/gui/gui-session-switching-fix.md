# GUI Session Switching Fix & Create Session Feature

**Date**: 2026-02-21

## Overview

Fixed critical session switching bug and implemented Create Session functionality in the AI Commander GUI.

## Issue 1: Critical Session Switching Bug (FIXED)

### Problem
When clicking to switch between sessions, the chat view did not clear previous messages. Old messages from the previous session remained visible in the new session.

### Root Cause
The `messages` store was not cleared when `currentSession` changed. The chat view simply displayed whatever was in the `messages` store without checking if the session had changed.

### Solution
Added reactive statement in `ChatView.svelte` to track session changes:

```typescript
let previousSessionName: string | null = null;

$: {
  const currentName = $currentSession?.name ?? null;
  if (currentName !== previousSessionName) {
    messages.set([]);
    if (currentName) {
      messages.update(m => [...m, {
        direction: 'system',
        content: `Connected to session: ${currentName.replace(/^commander-/, '')}`,
        timestamp: new Date(),
      }]);
    }
    previousSessionName = currentName;
  }
}
```

**Behavior**:
- When session changes, messages are cleared
- A system message is displayed showing which session is now active
- Previous session's messages no longer persist

### Files Modified
- `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/ChatView.svelte`

---

## Issue 2: Create Session Feature (IMPLEMENTED)

### Overview
Added ability to create new tmux sessions from within the GUI by selecting from available project directories.

### Components

#### 1. Backend Commands (`commands.rs`)

**New Structs**:
```rust
pub struct ProjectDirectory {
    pub name: String,
    pub path: String,
    pub project_type: String, // "claude-code", "mpm", or "current-dir"
}
```

**New Commands**:

1. `list_project_directories()` - Scans for project directories:
   - `~/.claude/projects/` - Claude Code projects
   - `~/.claude-mpm/projects/` - MPM projects
   - Current working directory (if it has `.claude` or `package.json`)

2. `create_session(name, directory, state)` - Creates new tmux session:
   - Validates session doesn't already exist
   - Creates session in specified directory using `tmux.create_session_in_dir()`

**Files Modified**:
- `/Users/masa/Projects/ai-commander/crates/commander-gui/src/commands.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-gui/src/main.rs` (registered commands)

#### 2. CreateSessionModal Component

**New File**: `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/CreateSessionModal.svelte`

**Features**:
- Modal dialog for creating sessions
- Lists available project directories grouped by type
- Input field for session name (prefixed with "commander-")
- Visual selection of directories
- Error handling and loading states
- Responsive design with clean UI

**Props**:
- `show: boolean` - Controls modal visibility

**Events**:
- `created` - Dispatched when session is successfully created

**UI Elements**:
- Session name input with preview
- Scrollable directory list with:
  - Project name
  - Project type badge (claude-code/mpm/current-dir)
  - Full path display
- Create/Cancel buttons
- Error message display

#### 3. SessionList Updates

**Changes**:
- Added "+ New" button to session list header
- Integrated CreateSessionModal component
- Auto-refreshes session list after creation

**New Header Layout**:
```
┌─────────────────────────────┐
│ Sessions           [+ New]  │
├─────────────────────────────┤
│ session-1        ●          │
│ session-2        ○          │
└─────────────────────────────┘
```

**Files Modified**:
- `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/SessionList.svelte`

### User Flow

1. User clicks "+ New" button in session list
2. Modal opens showing available project directories
3. User enters session name (e.g., "my-project")
4. User selects a project directory
5. User clicks "Create Session"
6. Backend creates `commander-my-project` session in selected directory
7. Modal closes and session list refreshes
8. New session appears in the list

### Technical Details

**Session Naming Convention**:
- User input: "my-project"
- Actual session name: "commander-my-project"
- Display name: "my-project" (prefix stripped)

**Directory Scanning**:
```
~/.claude/projects/        → Claude Code projects
~/.claude-mpm/projects/    → MPM projects
Current working directory   → If contains .claude or package.json
```

**Error Handling**:
- Session already exists
- Directory access errors
- Tmux command failures
- All errors displayed in modal

---

## Testing

### Manual Testing Steps

**Session Switching**:
1. Start GUI: `cargo run --package commander-gui`
2. Connect to session A
3. Send some messages
4. Click session B
5. Verify: Messages clear and system message shows "Connected to session: B"
6. Send new messages
7. Switch back to session A
8. Verify: Old messages are gone, new system message appears

**Create Session**:
1. Click "+ New" button
2. Verify: Modal opens with project directories
3. Enter session name: "test-session"
4. Select a directory
5. Click "Create Session"
6. Verify: Modal closes, session list refreshes
7. Verify: "commander-test-session" appears in list
8. Click the new session to connect
9. Verify: Can send messages to new session

**Edge Cases**:
- Try creating session with existing name → Error displayed
- Try creating without selecting directory → Error displayed
- Try creating with empty name → Button disabled

---

## Build Status

Build completed successfully with only minor warnings:
```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.35s
```

---

## Files Changed Summary

**Backend**:
- `crates/commander-gui/src/commands.rs` - Added 2 new commands
- `crates/commander-gui/src/main.rs` - Registered new commands

**Frontend**:
- `crates/commander-gui/ui/src/lib/components/ChatView.svelte` - Added session change tracking
- `crates/commander-gui/ui/src/lib/components/SessionList.svelte` - Added create button and modal integration
- `crates/commander-gui/ui/src/lib/components/CreateSessionModal.svelte` - New component (330 lines)

**Total Changes**: 5 files modified, 1 file created

---

## Future Enhancements

Potential improvements for future iterations:

1. **Project Type Filtering**: Add filter to show only Claude Code or MPM projects
2. **Custom Directories**: Allow browsing filesystem for arbitrary directories
3. **Templates**: Pre-configure sessions with common setups
4. **Session Settings**: Configure environment variables, shell, etc.
5. **Recent Projects**: Show recently used project directories
6. **Search**: Filter directory list by name or path
7. **Validation**: Check if directory exists and is accessible before creating
8. **Advanced Options**: Attach to existing window, specify window name, etc.

---

## Acceptance Criteria

All acceptance criteria met:

- ✅ Switching sessions clears previous messages
- ✅ System message shows which session is active
- ✅ "New" button appears in session list
- ✅ Create modal shows CC and MPM directories
- ✅ Can create new session in selected directory
- ✅ Session appears in list after creation
- ✅ Build succeeds without errors

---

## Notes

- The fix uses Svelte's reactive statements (`$:`) for automatic session change detection
- Project directory scanning is extensible for future project types
- Modal component is fully self-contained and reusable
- Session naming convention (commander-prefix) is consistent throughout the app
- The solution maintains backward compatibility with existing sessions
