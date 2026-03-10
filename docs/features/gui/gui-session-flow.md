# GUI Session Management Flow

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Tauri GUI Application                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌────────────────┐         ┌────────────────────────────┐     │
│  │  SessionList   │         │       ChatView             │     │
│  │  Component     │         │       Component            │     │
│  ├────────────────┤         ├────────────────────────────┤     │
│  │                │         │  • Display messages        │     │
│  │  [+ New]       │────────▶│  • Clear on session change │     │
│  │                │         │  • Show system message     │     │
│  │  ○ Session A   │         │  • Auto-scroll             │     │
│  │  ● Session B   │◀────────│                            │     │
│  │  ○ Session C   │  click  └────────────────────────────┘     │
│  │                │                                             │
│  └────────┬───────┘                                             │
│           │                                                     │
│           │ click "+ New"                                       │
│           ▼                                                     │
│  ┌─────────────────────────────────────────┐                   │
│  │   CreateSessionModal Component          │                   │
│  ├─────────────────────────────────────────┤                   │
│  │  1. Load project directories            │                   │
│  │  2. User selects directory              │                   │
│  │  3. User enters session name            │                   │
│  │  4. Create session via Tauri command    │                   │
│  │  5. Refresh session list                │                   │
│  └─────────────────────────────────────────┘                   │
│                        │                                         │
└────────────────────────┼─────────────────────────────────────────┘
                         │ Tauri IPC
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Rust Backend (Tauri)                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  commands.rs:                                                    │
│  ┌────────────────────────────────────────┐                     │
│  │  list_project_directories()            │                     │
│  │  ├─ Scan ~/.claude/projects/           │                     │
│  │  ├─ Scan ~/.claude-mpm/projects/       │                     │
│  │  └─ Check current directory            │                     │
│  └────────────────────────────────────────┘                     │
│                                                                  │
│  ┌────────────────────────────────────────┐                     │
│  │  create_session(name, directory)       │                     │
│  │  ├─ Validate session doesn't exist     │                     │
│  │  ├─ Call tmux.create_session_in_dir()  │                     │
│  │  └─ Return success/error               │                     │
│  └────────────────────────────────────────┘                     │
│                        │                                         │
└────────────────────────┼─────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Tmux Orchestrator                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  TmuxOrchestrator::create_session_in_dir(name, dir)             │
│  └─ Execute: tmux new-session -d -s name -c dir                 │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Session Switching Flow

```
┌─────────────────────────────────────────────────────────────────┐
│  User clicks "Session B" in SessionList                          │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  SessionList.connect(name)                                       │
│  ├─ invoke('connect_session', { name })                          │
│  └─ Update currentSession store                                  │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  ChatView reactive statement triggered                           │
│  ├─ Detect: currentSession.name changed                          │
│  ├─ Clear: messages.set([])                                      │
│  └─ Add system message: "Connected to session: B"                │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  UI updates                                                      │
│  ├─ SessionList shows "Session B" as active (blue highlight)    │
│  ├─ ChatView clears old messages                                │
│  └─ ChatView shows: [Connected to session: B]                   │
└─────────────────────────────────────────────────────────────────┘
```

## Create Session Flow

```
┌─────────────────────────────────────────────────────────────────┐
│  User clicks "+ New" button                                      │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  CreateSessionModal opens                                        │
│  └─ invoke('list_project_directories')                           │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  Backend scans filesystem                                        │
│  ├─ ~/.claude/projects/         → [ai-commander, my-app, ...]   │
│  ├─ ~/.claude-mpm/projects/     → [backend, frontend, ...]      │
│  └─ current dir (if valid)      → [current-project]             │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  Modal displays directories                                      │
│  ┌──────────────────────────────────────────────────────┐       │
│  │  ○ ai-commander          claude-code                 │       │
│  │    ~/.claude/projects/ai-commander                   │       │
│  │                                                       │       │
│  │  ○ my-app                mpm                         │       │
│  │    ~/.claude-mpm/projects/my-app                     │       │
│  └──────────────────────────────────────────────────────┘       │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  User interaction                                                │
│  ├─ Enter name: "test-session"                                  │
│  ├─ Select directory: ~/.claude-mpm/projects/my-app             │
│  └─ Click "Create Session"                                      │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  invoke('create_session', {                                      │
│    name: 'commander-test-session',                              │
│    directory: '~/.claude-mpm/projects/my-app'                   │
│  })                                                              │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  Backend creates tmux session                                    │
│  ├─ Validate: 'commander-test-session' doesn't exist            │
│  ├─ Execute: tmux new-session -d -s commander-test-session \    │
│  │           -c ~/.claude-mpm/projects/my-app                   │
│  └─ Return: Ok(())                                               │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  Modal closes and refreshes                                      │
│  ├─ Close modal                                                  │
│  └─ Trigger loadSessions()                                       │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  SessionList updates                                             │
│  ├─ Fetches updated session list                                │
│  └─ Shows new session: "test-session" ○                         │
└─────────────────────────────────────────────────────────────────┘
```

## Data Flow Diagram

```
Frontend (Svelte)                Backend (Rust)              Tmux
─────────────────               ─────────────               ─────

SessionList.svelte
  │
  ├─ loadSessions() ───────────▶ list_sessions() ──────────▶ list-sessions
  │                              ◀──────────────────────────
  │                              [Session, Session, ...]
  │
  ├─ connect(name) ────────────▶ connect_session(name)
  │                              └─ Update state.current_session
  │
  └─ "+ New" click
      │
      ▼
CreateSessionModal.svelte
  │
  ├─ loadDirectories() ────────▶ list_project_directories()
  │                              ├─ Scan ~/.claude/projects/
  │                              ├─ Scan ~/.claude-mpm/projects/
  │                              └─ Check current dir
  │                              ◀──────────────────────────
  │                              [ProjectDir, ProjectDir, ...]
  │
  └─ handleCreate() ───────────▶ create_session(name, dir)
                                 └─ tmux.create_session_in_dir()
                                    └─────────────────────────▶ new-session -d -s name -c dir
                                 ◀──────────────────────────
                                 Ok(())
      │
      ▼
  Dispatch 'created' event
      │
      ▼
SessionList.handleSessionCreated()
  └─ loadSessions()


ChatView.svelte
  │
  ├─ Subscribe: currentSession ─ Reactive store
  │                              (updates automatically)
  │
  └─ Reactive: $: { ... }
      ├─ Detect session change
      ├─ Clear messages
      └─ Add system message
```

## State Management

```
Svelte Stores (stores/app.ts)
─────────────────────────────

┌──────────────────────────────────────────┐
│  sessions: writable<Session[]>([])       │
│  ├─ Updated by: loadSessions()           │
│  └─ Used by: SessionList                 │
└──────────────────────────────────────────┘

┌──────────────────────────────────────────┐
│  currentSession: writable<Session|null>  │
│  ├─ Updated by: connect()                │
│  └─ Watched by: ChatView (reactive $:)   │
└──────────────────────────────────────────┘

┌──────────────────────────────────────────┐
│  messages: writable<Message[]>([])       │
│  ├─ Cleared by: ChatView (session change)│
│  ├─ Updated by: session-output events    │
│  └─ Displayed by: ChatView               │
└──────────────────────────────────────────┘


Rust State (state.rs)
────────────────────

┌──────────────────────────────────────────┐
│  GuiState {                              │
│    tmux: TmuxOrchestrator,               │
│    current_session: RwLock<Option<String>>│
│    bot_status: RwLock<BotStatus>,        │
│  }                                       │
└──────────────────────────────────────────┘
```

## Error Handling Flow

```
CreateSessionModal
  │
  ├─ No name entered ────────────▶ Button disabled (frontend validation)
  ├─ No directory selected ──────▶ Button disabled (frontend validation)
  │
  └─ Valid input ────────────────▶ invoke('create_session', ...)
      │
      ▼
Backend (commands.rs)
  │
  ├─ Session already exists ────▶ Err("Session 'X' already exists")
  ├─ Directory invalid ──────────▶ Err("Failed to create session: ...")
  ├─ Tmux error ─────────────────▶ Err(tmux_error.to_string())
  │
  └─ Success ────────────────────▶ Ok(())
      │
      ▼
CreateSessionModal
  │
  ├─ Error ──────────────────────▶ Display in modal (red box)
  └─ Success ────────────────────▶ Close modal, refresh list
```

## Component Hierarchy

```
App.svelte
│
├─── Header
│    └─── BotPanel
│
├─── Main
│    │
│    ├─── SessionList
│    │    ├─── Session items (buttons)
│    │    ├─── "+ New" button
│    │    └─── CreateSessionModal ◀─── New component
│    │         ├─── Modal overlay
│    │         ├─── Modal content
│    │         │    ├─── Header (title + close)
│    │         │    ├─── Body
│    │         │    │    ├─── Name input
│    │         │    │    └─── Directory list
│    │         │    └─── Footer (Cancel + Create)
│    │         └─── Error message (if any)
│    │
│    └─── ChatView
│         ├─── Session actions (Status, Stop, Disconnect)
│         ├─── Messages container
│         │    └─── Message items
│         └─── Scroll button
│
└─── MessageInput
```

## Performance Characteristics

**Session Switching**:
- Latency: < 10ms (reactive statement)
- Operations: 2 (clear array, add system message)
- DOM updates: Minimal (Svelte's efficient diffing)

**Create Session**:
- Directory scan: ~50-100ms (filesystem I/O)
- Modal render: ~20-30ms (initial render)
- Session creation: ~200-300ms (tmux command)
- Total user wait: ~500ms (with UI feedback)

**Memory Impact**:
- Modal component: ~10KB (uncompressed)
- Directory list: ~1KB per 100 entries
- State overhead: Negligible (few objects)

## Security Considerations

1. **Path Validation**: Backend validates directory paths exist
2. **Session Name Sanitization**: Prefix prevents conflicts
3. **No Shell Injection**: Uses Rust's Command API (safe)
4. **Directory Restrictions**: Only scans known safe locations
5. **No Arbitrary Execution**: Controlled tmux commands only

## Future Scalability

**Potential Optimizations**:
1. Cache directory scan results (invalidate on filesystem change)
2. Lazy load session history (pagination for 100+ sessions)
3. Virtual scrolling for large directory lists
4. Debounce session list refresh (reduce polling frequency)
5. WebSocket for real-time session updates (replace polling)

**Extension Points**:
1. Plugin architecture for custom project types
2. Remote session support (SSH tunnels)
3. Session templates system
4. Collaborative sessions (multiple users)
5. Session persistence (save/restore state)
