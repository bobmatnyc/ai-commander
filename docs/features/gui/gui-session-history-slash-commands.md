# GUI Session History and Slash Command Implementation

**Date**: 2026-02-21
**Status**: ✅ Implemented

## Overview

Implemented two major features for the AI Commander GUI:
1. **Session-Specific Message History**: Messages are preserved per session
2. **Slash Command Interpreter**: Local command handling with `/send` bypass

## Features Implemented

### 1. Session-Specific Message History

**Problem**: Messages were stored globally - switching sessions cleared everything.

**Solution**: Store messages in a Map structure keyed by session name.

#### Changes to `src/lib/stores/app.ts`

- Added `sessionMessages` writable store: `Map<string, Message[]>`
- Converted `messages` to a derived store that filters by current session
- Added helper functions:
  - `addMessageToSession(sessionName, message)` - Add message to specific session
  - `clearSessionMessages(sessionName)` - Clear messages for specific session

#### Behavior

- Each session maintains its own message history
- Switching sessions shows preserved messages
- No data loss when navigating between sessions
- Initial connection message shown only on first connect

### 2. Slash Command Interpreter

**Commands Supported**:

| Command | Description | Implementation |
|---------|-------------|----------------|
| `/status` | Send status command to session | Invokes `send_message` with '/status' |
| `/list` | List all sessions | Displays session list locally |
| `/disconnect` | Disconnect from current session | Invokes `disconnect_session` |
| `/stop` | Stop current session | Invokes `stop_session` with confirmation |
| `/clear` | Clear message history | Clears session messages |
| `/help` | Show available commands | Displays help text |
| `/send <text>` | Send literal text | Bypasses interpreter, sends raw text |

#### Changes to `src/lib/components/InputArea.svelte`

**New Function**: `handleSlashCommand(command: string)`
- Parses command and arguments
- Routes to appropriate handler
- Shows error for unknown commands

**Updated**: `sendMessage()`
- Detects slash commands (starts with `/` but not `/send`)
- Calls `handleSlashCommand()` for local commands
- Strips `/send` prefix and sends remainder literally
- Regular messages sent normally

**Visual Indicator**:
- Input field gets purple border and light purple background when typing slash command
- Placeholder text updated to hint at slash commands

#### Changes to `src/lib/components/ChatView.svelte`

- Removed session change watcher that cleared messages
- Updated action handlers to use `addMessageToSession()`
- Preserved messages in session history for all operations

#### Changes to `src/lib/components/SessionList.svelte`

- Checks if session has existing messages before adding initial connection message
- Only adds "Connected to session" message on first connection

## Implementation Details

### Store Architecture

```typescript
// Old (global messages)
export const messages = writable<Message[]>([]);

// New (session-specific with derived store)
export const sessionMessages = writable<Map<string, Message[]>>(new Map());
export const messages = derived(
  [sessionMessages, currentSession],
  ([$sessionMessages, $currentSession]) => {
    if (!$currentSession) return [];
    return $sessionMessages.get($currentSession.name) || [];
  }
);
```

### Command Flow

```
User types message
    |
    v
Is it a slash command?
    |
    +-- NO --> Send to tmux directly
    |
    +-- YES
        |
        v
    Is it /send <text>?
        |
        +-- YES --> Strip /send prefix, send remainder to tmux
        |
        +-- NO --> Handle locally via handleSlashCommand()
```

### Slash Command Handler

```typescript
switch (cmd) {
  case '/status':   // Send to tmux
  case '/list':     // Show locally
  case '/disconnect': // Disconnect
  case '/stop':     // Stop with confirmation
  case '/clear':    // Clear history
  case '/help':     // Show help
  default:          // Show error
}
```

## Testing Checklist

**Session History**:
- [x] Connect to session A, send messages
- [x] Connect to session B, send different messages
- [x] Click back to session A → original messages preserved
- [x] Click back to session B → its messages preserved
- [x] Initial connection message only appears once

**Slash Commands**:
- [x] `/help` → shows command list
- [x] `/status` → sends status to tmux
- [x] `/list` → shows all sessions locally
- [x] `/clear` → clears messages
- [x] `/disconnect` → disconnects from session
- [x] `/stop` → stops session with confirmation
- [x] `/send /status` → sends literal "/status" to tmux
- [x] Regular text → sends normally

**Visual Feedback**:
- [x] Input field changes appearance for slash commands
- [x] Placeholder hints at slash command feature
- [x] System messages styled differently

**Edge Cases**:
- [x] Unknown command shows error
- [x] Commands only work when connected
- [x] `/send` works with any text after it
- [x] Session messages persist across reconnections

## Files Modified

1. `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/stores/app.ts`
   - Added session-specific message storage
   - Created derived store for current session messages
   - Added helper functions

2. `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/InputArea.svelte`
   - Added slash command interpreter
   - Added `/send` bypass logic
   - Added visual indicator for slash commands
   - Updated placeholder text

3. `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/ChatView.svelte`
   - Removed global message clearing on session change
   - Updated all message operations to use `addMessageToSession()`
   - Preserved session-specific event handling

4. `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/SessionList.svelte`
   - Added check for existing messages before initial connection message
   - Imported `sessionMessages` for checking message history

## Benefits

1. **Better UX**: Users can switch between sessions without losing context
2. **Intuitive Commands**: Familiar slash command pattern from chat apps
3. **Flexibility**: `/send` provides escape hatch for literal text
4. **Discoverability**: `/help` command shows available options
5. **Safety**: Confirmation prompt for destructive operations
6. **Visual Feedback**: Purple highlighting indicates command mode

## Future Enhancements

Potential improvements:
- Command history (up/down arrows)
- Tab completion for commands
- Aliases for common commands
- Command arguments with flags
- Persistent message history (save to disk)
- Search within session messages
- Export session transcript
- Keyboard shortcuts for common commands
