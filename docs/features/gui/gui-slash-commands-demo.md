# GUI Slash Commands - Visual Demo

## Input Field States

### Normal Message Mode
```
┌─────────────────────────────────────────────────────────┐
│ Type message or /help for commands...           [Send] │
└─────────────────────────────────────────────────────────┘
```
- Normal border (gray)
- Regular background (white)

### Slash Command Mode
```
┌─────────────────────────────────────────────────────────┐
│ /status                                          [Send] │
└─────────────────────────────────────────────────────────┘
```
- Purple border (`#8b5cf6`)
- Light purple background (`#faf5ff`)
- Visual indicator that you're in command mode

## Command Examples

### 1. Get Help
```
Input:  /help
Output: Available commands:
          /status - Send status command
          /list - List all sessions
          /disconnect - Disconnect from session
          /stop - Stop this session
          /clear - Clear message history
          /help - Show this help
          /send <text> - Send literal text (bypass interpreter)
```

### 2. List Sessions
```
Input:  /list
Output: Available sessions:
          test-session (connected)
          api-dev
          frontend-debug
```

### 3. Send Status
```
Input:  /status
Output: [System] Sent status command
        [Received] Bot Status: Running
                   Session: test-session
                   Active: Yes
```

### 4. Clear History
```
Input:  /clear
Output: [System] Messages cleared
```

### 5. Disconnect
```
Input:  /disconnect
Output: [System] Disconnected from session "test-session"
```

### 6. Stop Session
```
Input:  /stop
Output: [Confirmation Dialog]
        Stop session "test-session"? This cannot be undone.
        [Cancel] [OK]

        If OK:
        [System] Session stopped
```

### 7. Send Literal Text (Bypass)
```
Input:  /send /status this is literal text
Output: [Sent] /status this is literal text
        (Sent to tmux as-is, not interpreted as command)
```

### 8. Unknown Command
```
Input:  /unknown
Output: [System] Unknown command: /unknown. Type /help for available commands.
```

## Session History Demonstration

### Scenario: Multiple Sessions

#### Session A: "api-dev"
```
[System] Connected to session: api-dev
[Sent] npm run dev
[Received] Server started on port 3000
[Sent] /status
[Received] Process running: node server.js
```

#### Switch to Session B: "frontend"
```
[System] Connected to session: frontend
[Sent] npm start
[Received] Webpack compiled successfully
[Sent] /list
[System] Available sessions:
           api-dev (connected)
           frontend (connected)
```

#### Switch Back to Session A: "api-dev"
```
[System] Connected to session: api-dev
[Sent] npm run dev
[Received] Server started on port 3000
[Sent] /status
[Received] Process running: node server.js
```
**All messages preserved!**

## Message Types and Styling

### Sent Message (Right-aligned, Blue)
```
                                              ┌─────────────────┐
                                              │ Hello world     │
                                              │ 11:30:45 AM     │
                                              └─────────────────┘
```

### Received Message (Left-aligned, Gray)
```
┌─────────────────┐
│ Response here   │
│ 11:30:46 AM     │
└─────────────────┘
```

### System Message (Center-aligned, Yellow)
```
                    ┌─────────────────────────────────┐
                    │ Connected to session: api-dev   │
                    │ 11:30:44 AM                     │
                    └─────────────────────────────────┘
```

## Command Workflow

```
┌──────────────────────────────────────────────────────────────┐
│                        User Input                             │
└──────────────────┬───────────────────────────────────────────┘
                   │
                   ▼
         ┌─────────────────┐
         │ Starts with '/'? │
         └────┬────────┬────┘
              │ YES    │ NO
              │        └────────────┐
              ▼                     ▼
    ┌────────────────┐    ┌──────────────────┐
    │ Starts with    │    │ Send to tmux     │
    │ '/send '?      │    │ as regular msg   │
    └────┬────────┬──┘    └──────────────────┘
         │ YES    │ NO
         │        └────────────┐
         ▼                     ▼
┌──────────────────┐  ┌──────────────────┐
│ Strip '/send'    │  │ Handle locally   │
│ prefix and send  │  │ via interpreter  │
│ to tmux          │  │                  │
└──────────────────┘  └──────────────────┘
                              │
                              ▼
                    ┌──────────────────┐
                    │ /status          │
                    │ /list            │
                    │ /disconnect      │
                    │ /stop            │
                    │ /clear           │
                    │ /help            │
                    │ unknown → error  │
                    └──────────────────┘
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message or execute command |
| `Shift+Enter` | (Future) New line in message |
| `Escape` | (Future) Clear input |

## Benefits

1. **No Data Loss**: Switch between sessions freely
2. **Quick Actions**: Common operations via slash commands
3. **Visual Feedback**: Purple highlighting for command mode
4. **Escape Hatch**: `/send` bypasses interpreter when needed
5. **Help Available**: `/help` always shows available commands
6. **Safe Operations**: Confirmation for destructive actions

## Technical Implementation

### Store Structure
```typescript
sessionMessages: Map {
  "commander-api-dev" => [
    { direction: 'sent', content: 'npm run dev', timestamp: ... },
    { direction: 'received', content: 'Server started', timestamp: ... }
  ],
  "commander-frontend" => [
    { direction: 'sent', content: 'npm start', timestamp: ... },
    { direction: 'received', content: 'Webpack compiled', timestamp: ... }
  ]
}
```

### Current View (Derived Store)
```typescript
messages = derived([sessionMessages, currentSession], ...)
// Automatically shows messages for active session only
```
