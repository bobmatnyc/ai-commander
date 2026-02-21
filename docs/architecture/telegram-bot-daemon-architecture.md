# Telegram Bot Daemon Architecture

**Last Updated:** 2026-02-21
**Status:** Current Implementation (Verified)

---

## Process Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│                          System Architecture                            │
│                                                                          │
│  User Interfaces                  Daemon Layer              Backend     │
│  ─────────────────                ────────────              ────────   │
│                                                                          │
│  ┌──────────────┐                                                       │
│  │   Terminal   │                                                       │
│  │              │                                                       │
│  │  ┌────────┐  │     spawn                                            │
│  │  │  TUI   │──┼──────────┐                                           │
│  │  │ (ratatui) │          │                                           │
│  │  └────┬───┘  │          │                                           │
│  │       │      │          │                                           │
│  │       │ write│          │                                           │
│  │       ▼      │          │                                           │
│  │  pairing_codes.json     │                                           │
│  │              │          │                                           │
│  └──────────────┘          │                                           │
│                            │                                           │
│                            │     ┌─────────────────────────────┐       │
│                            │     │   Telegram Bot Daemon       │       │
│                            │     │   (commander-telegram)      │       │
│                            └────▶│                             │       │
│                                  │  ┌──────────────────────┐   │       │
│  ┌──────────────┐                │  │   Main Event Loop    │   │       │
│  │  Telegram    │◀──polling──────┼─▶│   (teloxide)         │   │       │
│  │    API       │                │  └──────────────────────┘   │       │
│  │              │                │           │                 │       │
│  └──────────────┘                │           │ read/write      │       │
│                                  │           ▼                 │       │
│                                  │  ┌──────────────────────┐   │       │
│                                  │  │   State Management   │   │       │
│                                  │  │   (RwLock HashMap)   │   │       │
│                                  │  └──────────────────────┘   │       │
│                                  │           │                 │       │
│                                  │           │ persist         │       │
│  ┌──────────────────────────┐   │           ▼                 │       │
│  │   State Files            │◀──┼───────────────────────────  │       │
│  │   ~/.local/share/        │   │                             │       │
│  │   commander/state/       │   │  ┌──────────────────────┐   │       │
│  │                          │   │  │  Tmux Orchestrator   │   │       │
│  │  • telegram_sessions.json│   │  │  (session control)   │───┼──────▶│
│  │  • authorized_chats.json │   │  └──────────────────────┘   │       │
│  │  • pairing_codes.json    │   │                             │       │
│  │  • telegram.pid          │   └─────────────────────────────┘       │
│  └──────────────────────────┘                                         │
│                                                                          │
│  ┌──────────────┐                                          ┌─────────┐ │
│  │   Future:    │     spawn                                │  Tmux   │ │
│  │     GUI      │──────────────────────────────────────────│ Sessions│ │
│  │   (Tauri)    │     (same pattern as TUI)                │         │ │
│  └──────────────┘                                          └─────────┘ │
│                                                                          │
└────────────────────────────────────────────────────────────────────────┘
```

---

## Data Flow: Pairing Process

```
┌────────────────────────────────────────────────────────────────────────┐
│                          Pairing Flow                                   │
├────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  Step 1: Generate Code (TUI)                                           │
│  ─────────────────────────────                                          │
│                                                                          │
│  TUI Process                                                            │
│  │                                                                       │
│  ├─▶ User: /telegram                                                    │
│  │                                                                       │
│  ├─▶ generate_code()                                                    │
│  │   └─▶ code = "ABC123" (6 chars, 32^6 combinations)                  │
│  │                                                                       │
│  ├─▶ write_pairing_file()                                               │
│  │   └─▶ pairing_codes.json:                                            │
│  │       {                                                               │
│  │         "ABC123": {                                                   │
│  │           "project": "myproject",                                     │
│  │           "session": "commander-myproject",                           │
│  │           "created_at": "2026-02-21T12:00:00Z",                      │
│  │           "expires_at": "2026-02-21T12:05:00Z"  # 5 min TTL          │
│  │         }                                                             │
│  │       }                                                               │
│  │                                                                       │
│  └─▶ Display: "Code: ABC123 (expires in 5 minutes)"                    │
│                                                                          │
│                                                                          │
│  Step 2: Consume Code (Bot)                                            │
│  ────────────────────────────                                           │
│                                                                          │
│  Bot Daemon (continuously running)                                      │
│  │                                                                       │
│  ├─▶ Telegram: User sends "/pair ABC123"                               │
│  │                                                                       │
│  ├─▶ handle_pair(code = "ABC123")                                       │
│  │   │                                                                   │
│  │   ├─▶ read_pairing_file()                                            │
│  │   │   └─▶ Found code "ABC123"                                        │
│  │   │                                                                   │
│  │   ├─▶ validate_expiration()                                          │
│  │   │   └─▶ Not expired (< 5 min) ✓                                    │
│  │   │                                                                   │
│  │   ├─▶ create_user_session(chat_id, project="myproject")             │
│  │   │   └─▶ sessions.insert(chat_id, UserSession { ... })             │
│  │   │                                                                   │
│  │   ├─▶ authorize_chat(chat_id)                                        │
│  │   │   └─▶ authorized_chats.insert(chat_id)                           │
│  │   │                                                                   │
│  │   ├─▶ remove_code("ABC123")  # Single-use, delete after             │
│  │   │   └─▶ write_pairing_file(codes - "ABC123")                       │
│  │   │                                                                   │
│  │   └─▶ persist_state()                                                │
│  │       └─▶ telegram_sessions.json, authorized_chats.json             │
│  │                                                                       │
│  └─▶ Reply: "Connected to myproject! Send messages to start."          │
│                                                                          │
└────────────────────────────────────────────────────────────────────────┘
```

---

## Communication Mechanisms

### 1. File-Based IPC (Current Implementation)

```
┌─────────────────────────────────────────────────────────────┐
│                   File-Based IPC Pattern                     │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Interface Process          State Files       Bot Daemon    │
│  ────────────────           ───────────       ──────────    │
│                                                              │
│  TUI or GUI                                                  │
│       │                                                      │
│       │ write                                                │
│       ▼                                                      │
│  pairing_codes.json                                          │
│       {                                                      │
│         "ABC123": { ... }   ◀─────────── read               │
│       }                                        │             │
│                                                │             │
│  projects.db                                   │             │
│       {                          read ────────▶│             │
│         "myproject": { ... }     write ◀───────┤             │
│       }                                        │             │
│                                                │             │
│  telegram_sessions.json        ◀─────────── read/write      │
│       {                                        │             │
│         123456789: {                           │             │
│           "session": "...",                    │             │
│           "project": "..."                     │             │
│         }                                      │             │
│       }                                        │             │
│                                                │             │
│  telegram.pid                  ◀─────────── write PID       │
│       12345                                    │             │
│                                read PID ───────┘             │
│                                                              │
│  Properties:                                                 │
│  ✓ No shared memory                                          │
│  ✓ No sockets or ports                                       │
│  ✓ Atomic file operations                                    │
│  ✓ Works across process boundaries                           │
│  ✓ Survives process restarts                                 │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 2. Optional HTTP API (Future Enhancement)

```
┌─────────────────────────────────────────────────────────────┐
│               HTTP API Pattern (Optional)                    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  GUI Client              HTTP API           Bot Daemon      │
│  ──────────              ────────           ──────────      │
│                                                              │
│  Tauri/Electron                                              │
│       │                                                      │
│       │ GET /status                                          │
│       │────────────────────▶ :8080                           │
│       │                       │                              │
│       │                       │ query state                  │
│       │                       └─────────────────▶           │
│       │                                           │           │
│       │ ◀──────────────────────────────────────  │           │
│       │   { "running": true,                     │           │
│       │     "sessions": 3,                       │           │
│       │     "uptime": 3600 }                     │           │
│       │                                                      │
│       │ GET /sessions                                        │
│       │────────────────────▶                                 │
│       │                       │                              │
│       │                       │ list sessions                │
│       │                       └─────────────────▶           │
│       │                                           │           │
│       │ ◀──────────────────────────────────────  │           │
│       │   [                                      │           │
│       │     { "name": "myproject",               │           │
│       │       "status": "idle",                  │           │
│       │       "chat_id": 123456789 }             │           │
│       │   ]                                      │           │
│       │                                                      │
│       │ POST /pair                                           │
│       │────────────────────▶                                 │
│       │   { "project": "myproject" }                         │
│       │                       │                              │
│       │                       │ create pairing               │
│       │                       └─────────────────▶           │
│       │                                           │           │
│       │ ◀──────────────────────────────────────  │           │
│       │   { "code": "ABC123",                    │           │
│       │     "expires_at": "..." }                │           │
│       │                                                      │
│                                                              │
│  Configuration:                                              │
│  • Bot started with: --api-port 8080                         │
│  • localhost only (security)                                 │
│  • No authentication (local trust)                           │
│  • JSON responses                                            │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## State Persistence

```
┌──────────────────────────────────────────────────────────────┐
│                  State File Structure                         │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  ~/.local/share/commander/state/                             │
│  │                                                            │
│  ├─ telegram_sessions.json         (Runtime state)           │
│  │  {                                                         │
│  │    "123456789": {                 // chat_id              │
│  │      "chat_id": 123456789,                                │
│  │      "project_name": "myproject",                         │
│  │      "project_path": "/path/to/project",                  │
│  │      "tmux_session": "commander-myproject",               │
│  │      "is_waiting": false,                                 │
│  │      "last_activity": "2026-02-21T12:30:00Z",            │
│  │      "created_at": "2026-02-21T12:00:00Z"                │
│  │    }                                                       │
│  │  }                                                         │
│  │                                                            │
│  ├─ authorized_chats.json           (Persistent)             │
│  │  [                                                         │
│  │    123456789,                     // Authorized chat IDs  │
│  │    987654321                                              │
│  │  ]                                                         │
│  │                                                            │
│  ├─ pairing_codes.json               (Temporary)             │
│  │  {                                                         │
│  │    "ABC123": {                                            │
│  │      "code": "ABC123",                                    │
│  │      "project_name": "myproject",                         │
│  │      "session_name": "commander-myproject",               │
│  │      "created_at": "2026-02-21T12:00:00Z",               │
│  │      "expires_at": "2026-02-21T12:05:00Z"  // 5 min TTL  │
│  │    }                                                       │
│  │  }                                                         │
│  │                                                            │
│  └─ telegram.pid                     (Process management)    │
│     12345                            // Bot daemon PID       │
│                                                               │
│  Persistence Strategy:                                        │
│  • Sessions: Saved on connect/disconnect + every 30s         │
│  • Authorized: Saved immediately on pairing                   │
│  • Codes: 5-minute TTL, cleaned on access                    │
│  • PID: Written on start, deleted on exit                    │
│                                                               │
│  Restart Behavior:                                            │
│  • Bot loads sessions.json on startup                         │
│  • Validates tmux sessions still exist                        │
│  • Discards expired sessions (>24h old)                       │
│  • Sends rebuild notification to authorized chats            │
│                                                               │
└──────────────────────────────────────────────────────────────┘
```

---

## Deployment Patterns

### Pattern 1: Development (Current)

```
┌─────────────────────────────────────────────────────┐
│          Development Setup (Manual Start)            │
├─────────────────────────────────────────────────────┤
│                                                      │
│  Terminal 1: TUI                                     │
│  ─────────────────                                   │
│  $ ai-commander                                      │
│  > /telegram                     ┌─────────────┐    │
│  ─────────────────────────────▶  │ Bot Daemon  │    │
│                     spawn         │  (spawned)  │    │
│  [ok] Bot started                 └─────────────┘    │
│  Code: ABC123                              │         │
│                                            │         │
│                                   Background process │
│                                   (survives TUI exit)│
│                                                      │
│  Terminal 2: Direct Start                           │
│  ───────────────────────                             │
│  $ commander-telegram                                │
│  [robot] Bot running...                              │
│  [phone] Open Telegram...                            │
│                                                      │
│  Pros:                                               │
│  ✓ Simple for development                            │
│  ✓ Easy debugging                                    │
│                                                      │
│  Cons:                                               │
│  ✗ Manual restart required                           │
│  ✗ No auto-start on reboot                           │
│                                                      │
└─────────────────────────────────────────────────────┘
```

### Pattern 2: Production (systemd)

```
┌─────────────────────────────────────────────────────┐
│           Production Setup (systemd)                 │
├─────────────────────────────────────────────────────┤
│                                                      │
│  System Boot                                         │
│  ──────────                                          │
│       │                                              │
│       ▼                                              │
│  systemd --user                                      │
│       │                                              │
│       ├─▶ commander-telegram.service                │
│       │        │                                     │
│       │        ├─▶ Load env from telegram.env       │
│       │        │   (TELEGRAM_BOT_TOKEN)             │
│       │        │                                     │
│       │        └─▶ ExecStart=commander-telegram     │
│       │                    │                         │
│       │                    ▼                         │
│       │            [Bot Daemon Running]             │
│       │                    │                         │
│       │                    ├─▶ Restart=on-failure   │
│       │                    │   RestartSec=5         │
│       │                    │                         │
│       │                    └─▶ StandardOutput=journal│
│                                                      │
│  User Management                                     │
│  ────────────────                                    │
│  $ systemctl --user status commander-telegram       │
│  ● commander-telegram.service - Commander Bot       │
│     Active: active (running) since ...              │
│     Main PID: 12345                                  │
│                                                      │
│  $ systemctl --user restart commander-telegram      │
│  $ journalctl --user -u commander-telegram -f       │
│                                                      │
│  Pros:                                               │
│  ✓ Auto-start on boot                                │
│  ✓ Auto-restart on crash                             │
│  ✓ Centralized logging                               │
│  ✓ Standard Linux service management                 │
│                                                      │
└─────────────────────────────────────────────────────┘
```

### Pattern 3: GUI-Managed (Future)

```
┌─────────────────────────────────────────────────────┐
│              GUI-Managed Bot (Future)                │
├─────────────────────────────────────────────────────┤
│                                                      │
│  GUI Application Start                              │
│  ─────────────────────                               │
│       │                                              │
│       ├─▶ Check: is_bot_running()?                  │
│       │       │                                      │
│       │       └─▶ Read telegram.pid                 │
│       │           Check if process exists            │
│       │                                              │
│       ├─▶ If not running:                            │
│       │   start_bot_daemon()                         │
│       │        │                                     │
│       │        ├─▶ spawn("commander-telegram")      │
│       │        │   detached=true, stdio=ignore      │
│       │        │                                     │
│       │        └─▶ Write telegram.pid               │
│       │                                              │
│       └─▶ GUI Status Bar:                            │
│           [●] Bot: Running (3 sessions)             │
│                                                      │
│  GUI Status Monitoring                              │
│  ────────────────────                                │
│       │                                              │
│       ├─▶ Option 1: Poll state files (simple)       │
│       │   setInterval(checkSessions, 500ms)         │
│       │        │                                     │
│       │        └─▶ Read telegram_sessions.json      │
│       │                                              │
│       └─▶ Option 2: HTTP API (advanced)             │
│           fetch('http://localhost:8080/status')     │
│                    │                                 │
│                    └─▶ Bot serves HTTP API          │
│                        (optional --api-port flag)    │
│                                                      │
│  Pros:                                               │
│  ✓ User-friendly (no CLI)                            │
│  ✓ Visual status monitoring                          │
│  ✓ One-click start/stop                              │
│                                                      │
└─────────────────────────────────────────────────────┘
```

---

## Dependency Graph

```
┌────────────────────────────────────────────────────────────┐
│                   Component Dependencies                    │
├────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────┐         ┌─────────────────────────┐  │
│  │   TUI Binary    │         │   Telegram Bot Binary   │  │
│  │ (ai-commander)  │         │ (commander-telegram)    │  │
│  └────────┬────────┘         └────────────┬────────────┘  │
│           │                               │                │
│           │ Process spawn                 │                │
│           │ (NOT dependency)              │                │
│           │                               │                │
│           ▼                               ▼                │
│  ┌──────────────────────────────────────────────────────┐ │
│  │          Shared Core Libraries                       │ │
│  │  ┌──────────────┐  ┌────────────────────────────┐   │ │
│  │  │  Models      │  │  Persistence (StateStore)  │   │ │
│  │  │ (Project,    │  │  (JSON, SQLite)            │   │ │
│  │  │  Session)    │  └────────────────────────────┘   │ │
│  │  └──────────────┘                                    │ │
│  │  ┌──────────────┐  ┌────────────────────────────┐   │ │
│  │  │  Adapters    │  │  Tmux Orchestrator         │   │ │
│  │  │ (CC, MPM,    │  │  (session mgmt)            │   │ │
│  │  │  Aider)      │  └────────────────────────────┘   │ │
│  │  └──────────────┘                                    │ │
│  │  ┌──────────────┐                                    │ │
│  │  │  Core Utils  │                                    │ │
│  │  │ (config,     │                                    │ │
│  │  │  pairing)    │                                    │ │
│  │  └──────────────┘                                    │ │
│  └──────────────────────────────────────────────────────┘ │
│           │                               │                │
│           │                               │                │
│           ▼                               ▼                │
│  ┌─────────────────────────────────────────────────────┐  │
│  │              State Files                             │  │
│  │  ~/.local/share/commander/state/                    │  │
│  │  • telegram_sessions.json                            │  │
│  │  • authorized_chats.json                             │  │
│  │  • pairing_codes.json                                │  │
│  │  • projects.db                                       │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                             │
│  Key Insight:                                               │
│  ────────────                                               │
│  TUI and Bot share LIBRARIES, not PROCESSES                 │
│  → Enables independent execution                            │
│  → Clean separation of concerns                             │
│  → GUI can replace TUI without touching bot                 │
│                                                             │
└────────────────────────────────────────────────────────────┘
```

---

## Security Boundaries

```
┌────────────────────────────────────────────────────────────┐
│                   Security Architecture                     │
├────────────────────────────────────────────────────────────┤
│                                                             │
│  Internet                                                   │
│  ────────                                                   │
│       │                                                     │
│       │ HTTPS                                               │
│       ▼                                                     │
│  ┌──────────────────┐                                      │
│  │  Telegram API    │                                      │
│  │  (api.telegram.org)                                     │
│  └────────┬─────────┘                                      │
│           │                                                 │
│           │ HTTPS                                           │
│           │ BOT_TOKEN                                       │
│           ▼                                                 │
│  ╔════════════════════════════════════════════════╗        │
│  ║         Bot Daemon (Secure Boundary)           ║        │
│  ╠════════════════════════════════════════════════╣        │
│  ║                                                 ║        │
│  ║  • BOT_TOKEN in env (not in code)             ║        │
│  ║  • Pairing codes: 5-min TTL, single-use       ║        │
│  ║  • Authorized chats: persisted allowlist       ║        │
│  ║  • State files: 0600 permissions               ║        │
│  ║  • HTTP API: localhost only (optional)         ║        │
│  ║                                                 ║        │
│  ╚═════════════════════════════════════════════════╝        │
│           │                                                 │
│           │ Local IPC (files)                              │
│           │ 0600 permissions                               │
│           ▼                                                 │
│  ┌──────────────────────────────────────────────┐          │
│  │   State Files (User-Only Access)             │          │
│  │   ~/.local/share/commander/state/            │          │
│  │   (chmod 0600, owner-only read/write)        │          │
│  └──────────────────────────────────────────────┘          │
│           │                                                 │
│           │ Unix socket                                     │
│           │ (authenticated)                                 │
│           ▼                                                 │
│  ┌──────────────────────────────────────────────┐          │
│  │   Tmux Sessions                              │          │
│  │   (user-owned, isolated)                     │          │
│  └──────────────────────────────────────────────┘          │
│                                                             │
│  Threat Model:                                              │
│  ─────────────                                              │
│  ✓ Bot compromise → Only bot chat accessible                │
│  ✓ File access → State protected by 0600 perms             │
│  ✓ Code interception → 5-min TTL limits window             │
│  ✓ HTTP API (if enabled) → localhost only, no auth needed  │
│  ✓ Multi-user system → User isolation via file perms       │
│                                                             │
└────────────────────────────────────────────────────────────┘
```

---

*Architecture verified: 2026-02-21*
*Status: Production-ready for daemon operation*
