# GUI UX Changes - Visual Guide

## Overview

Three key improvements to the AI Commander GUI based on user feedback:

1. Clean session names (remove "commander-" prefix)
2. Session action tabs
3. Better error handling

## Before and After

### Session List - Before
```
┌─────────────────────────┐
│ Sessions                │
├─────────────────────────┤
│ ○ commander-izzie    ●  │
│ ○ commander-default  ●  │
│ ○ commander-work     ●  │
└─────────────────────────┘
```

### Session List - After
```
┌─────────────────────────┐
│ Sessions                │
├─────────────────────────┤
│ ○ izzie              ●  │  <- Clean names!
│ ○ default            ●  │
│ ○ work               ●  │
└─────────────────────────┘
```

### Chat View - Before (No Session Actions)
```
┌─────────────────────────────────────────────────┐
│                                                 │
│  User: Hello                             10:30 │
│                                                 │
│  Bot: Hi there!                          10:31 │
│                                                 │
└─────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────┐
│ Type message...                         [Send]  │
└─────────────────────────────────────────────────┘
```

### Chat View - After (With Session Actions)
```
┌─────────────────────────────────────────────────┐
│ [Status] [Stop] [Disconnect]                    │  <- New!
├─────────────────────────────────────────────────┤
│                                                 │
│  User: Hello                             10:30 │
│                                                 │
│  Bot: Hi there!                          10:31 │
│                                                 │
└─────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────┐
│ Type message...                         [Send]  │
└─────────────────────────────────────────────────┘
```

## Tab Actions in Detail

### Status Tab
```
Click [Status] → Sends "/status" to session
                ↓
┌──────────────────────────────────────────┐
│  User: /status                    10:35 │
│  System: Session active, 3 tasks  10:35 │
└──────────────────────────────────────────┘
```

### Stop Tab
```
Click [Stop] → Shows confirmation dialog
              ↓
┌────────────────────────────────────────────┐
│  ⚠️  Stop Session                          │
│                                            │
│  Are you sure you want to stop session     │
│  "commander-izzie"? This will terminate    │
│  the session.                              │
│                                            │
│         [Cancel]  [Stop Session]           │
└────────────────────────────────────────────┘
              ↓ (if confirmed)
┌────────────────────────────────────────────┐
│  System: Session "commander-izzie"  10:36 │
│          stopped successfully.             │
└────────────────────────────────────────────┘
Session list updates, tabs disappear
```

### Disconnect Tab
```
Click [Disconnect] → Disconnects gracefully
                    ↓
┌────────────────────────────────────────────┐
│  System: Disconnected from session  10:37 │
│          "commander-izzie".                │
└────────────────────────────────────────────┘
Returns to empty state, tabs disappear
Session continues running in background
```

## Error Handling - Before vs After

### Before (Alert Dialogs)
```
User sends message with no connection
        ↓
┌────────────────────────┐
│  ⚠️  Error             │
│                        │
│  Failed to send:       │
│  Not connected to      │
│  a session             │
│                        │
│        [OK]            │
└────────────────────────┘
```

### After (System Messages)
```
User sends message with no connection
        ↓
┌──────────────────────────────────────────┐
│  System: Error: Not connected to a       │
│  session. Please select a session first. │
│                                    10:38  │
└──────────────────────────────────────────┘
```

## State Flow Diagram

```
         User Opens GUI
              │
              ▼
      ┌──────────────┐
      │ Select       │
      │ Session      │
      └──────┬───────┘
             │
             ▼
      ┌──────────────┐
      │ Connected    │
      │ Tabs Appear  │
      └──────┬───────┘
             │
    ┌────────┼────────┐
    │        │        │
    ▼        ▼        ▼
[Status] [Stop] [Disconnect]
    │        │        │
    │        │        └─→ Disconnects
    │        │             Tabs hide
    │        │
    │        └─→ Destroys session
    │             Tabs hide
    │             Returns to session list
    │
    └─→ Sends /status
         Shows response
         Stays connected
```

## Code Changes Summary

### Frontend (Svelte)

1. **SessionList.svelte**
   - Added `getDisplayName()` function
   - Strips "commander-" prefix for display

2. **ChatView.svelte**
   - Added session actions bar
   - Added three handler functions
   - Added loading state
   - Added confirmation for Stop

3. **InputArea.svelte**
   - Better validation before send
   - System messages instead of alerts

### Backend (Rust)

1. **commands.rs**
   - New `stop_session()` command
   - Enhanced `send_message()` with validation
   - Debug logging added

2. **main.rs**
   - Registered `stop_session` command

## Style Guide

### Tab Button Styles
```css
Normal:  White background, gray border
Hover:   Light gray background
Active:  Darker gray background
Disabled: 50% opacity, no pointer
```

### Message Types
```css
Sent:     Blue background, white text, right aligned
Received: Light gray background, dark text, left aligned
System:   Yellow background, brown text, center aligned
```

## Testing Flow

1. Start GUI: `cargo tauri dev`
2. Create test session: `tmux new-session -d -s commander-test`
3. In GUI:
   - Session shows as "test" (not "commander-test") ✓
   - Click to connect
   - Tabs appear ✓
   - Click Status → `/status` sent ✓
   - Click Disconnect → tabs disappear ✓
   - Reconnect
   - Click Stop → confirmation appears ✓
   - Confirm → session destroyed ✓

## Performance Impact

- Minimal: Only display-time string manipulation
- No additional API calls
- Tabs only render when session connected
- Loading states prevent duplicate requests
