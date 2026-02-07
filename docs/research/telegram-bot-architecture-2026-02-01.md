# Telegram Bot Architecture Research

**Date:** 2026-02-01
**Researcher:** Claude (Research Agent)
**Scope:** commander-telegram crate implementation analysis

---

## 1. Executive Summary

The Commander Telegram bot (`crates/commander-telegram/`) provides a mobile interface for interacting with Claude Code sessions via Telegram. The current architecture uses a **chat-to-session mapping** stored in in-memory state, with **no authentication or pairing mechanism** for connecting Telegram chats to existing sessions.

### Key Findings

1. **Session mapping is purely chat-id based** - Any user who knows a project name can connect to it
2. **"Session not found" errors** originate from two distinct sources:
   - `TelegramError::NotConnected` - When user's chat_id has no session mapping
   - `TelegramError::SessionError` - When tmux session doesn't exist or attach fails
3. **No start code or pairing mechanism exists** - Users connect by project name alone
4. **Security is minimal** - Anyone with the bot token can interact with any registered project

---

## 2. Current Architecture

### 2.1 Component Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     Telegram Bot Architecture                    │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────┐  │
│  │ TelegramBot  │───▶│TelegramState │───▶│  UserSession     │  │
│  │   (bot.rs)   │    │  (state.rs)  │    │  (session.rs)    │  │
│  └──────┬───────┘    └──────┬───────┘    └──────────────────┘  │
│         │                   │                                   │
│         │                   │ RwLock<HashMap<i64, UserSession>> │
│         │                   │                                   │
│  ┌──────▼───────┐    ┌──────▼───────┐    ┌──────────────────┐  │
│  │  Handlers    │    │ TmuxOrchest. │    │ AdapterRegistry  │  │
│  │(handlers.rs) │    │              │    │                  │  │
│  └──────────────┘    └──────────────┘    └──────────────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 File Structure

| File | Purpose | Lines |
|------|---------|-------|
| `lib.rs` | Module exports and documentation | 67 |
| `main.rs` | CLI binary entry point | 87 |
| `bot.rs` | TelegramBot struct, polling/webhook setup | 217 |
| `state.rs` | TelegramState, session management, tmux integration | 644 |
| `handlers.rs` | Command handlers (/start, /connect, /session, etc.) | 524 |
| `session.rs` | UserSession struct for tracking chat-to-project mapping | 135 |
| `error.rs` | Error types and conversions | 79 |
| `ngrok.rs` | Ngrok tunnel management for webhooks | 214 |

### 2.3 Data Flow

```
User Message → Telegram API → teloxide handler
                                    │
                                    ▼
                            handle_message()
                                    │
                  ┌─────────────────┼─────────────────┐
                  │                 │                 │
            has_session()?   send_message()    NotConnected
                  │                 │              error
                  │                 ▼
                  │          tmux.send_line()
                  │                 │
                  │                 ▼
                  │        poll_output_loop()
                  │                 │
                  │                 ▼
                  │        summarize_response()
                  │                 │
                  ▼                 ▼
            Error response    Send to Telegram
```

---

## 3. Session Mapping Analysis

### 3.1 State Structure (state.rs)

```rust
pub struct TelegramState {
    // Key data structure: maps Telegram chat_id to UserSession
    sessions: RwLock<HashMap<i64, UserSession>>,

    // External dependencies
    tmux: Option<TmuxOrchestrator>,
    adapters: AdapterRegistry,
    store: StateStore,

    // OpenRouter for summarization
    openrouter_key: Option<String>,
    openrouter_model: String,
}
```

### 3.2 UserSession Structure (session.rs)

```rust
pub struct UserSession {
    pub chat_id: ChatId,           // Telegram chat identifier
    pub project_path: String,      // Filesystem path
    pub project_name: String,      // Human-readable name
    pub tmux_session: String,      // tmux session name (commander-{name})

    // Response collection state
    pub response_buffer: Vec<String>,
    pub last_output_time: Option<Instant>,
    pub last_output: String,
    pub pending_query: Option<String>,
    pub is_waiting: bool,
}
```

### 3.3 Session Lifecycle

1. **Creation via /connect**
   - User sends `/connect <project_name>`
   - Handler calls `state.connect(chat_id, project_name)`
   - State loads projects from `StateStore`
   - State finds project by name or ID
   - If tmux session missing, creates one via adapter
   - Creates `UserSession` and inserts into HashMap

2. **Session attachment via /session**
   - User sends `/session <tmux_session_name>`
   - Handler calls `state.attach_session(chat_id, session_name)`
   - Checks if tmux session exists
   - Creates UserSession with inferred project info

3. **Message routing**
   - Regular messages trigger `handle_message()`
   - Checks `state.has_session(chat_id)`
   - If no session: returns `NotConnected` error
   - If session: forwards to tmux via `send_line()`

4. **Disconnection**
   - User sends `/disconnect`
   - Handler calls `state.disconnect(chat_id)`
   - Removes UserSession from HashMap
   - Does NOT terminate tmux session

---

## 4. "Session Not Found" Error Origins

### 4.1 Error Type 1: TelegramError::NotConnected

**Location:** `error.rs:36-38`, `state.rs:188`, `state.rs:222`

```rust
#[error("Not connected to a project. Use /connect <project> first.")]
NotConnected,
```

**Trigger conditions:**
- User sends message without having called `/connect` first
- User's `chat_id` not present in `sessions` HashMap
- Session was removed (disconnect or process restart)

**Relevant code (state.rs:186-188):**
```rust
let session = sessions
    .get_mut(&chat_id.0)
    .ok_or(TelegramError::NotConnected)?;
```

### 4.2 Error Type 2: TelegramError::SessionError

**Location:** `state.rs:374-376`

```rust
return Err(TelegramError::SessionError(
    format!("Session '{}' not found. Use /sessions to list available sessions.", session_name)
));
```

**Trigger conditions:**
- User attempts to attach to non-existent tmux session
- tmux `session_exists()` returns false

**Relevant code (state.rs:373-377):**
```rust
if !tmux.session_exists(session_name) {
    return Err(TelegramError::SessionError(
        format!("Session '{}' not found. Use /sessions to list available sessions.", session_name)
    ));
}
```

### 4.3 Error Type 3: ProjectNotFound

**Location:** `error.rs:42-43`, `state.rs:111`

```rust
#[error("Project not found: {0}")]
ProjectNotFound(String),
```

**Trigger conditions:**
- User calls `/connect <name>` with unknown project
- Project not in StateStore

---

## 5. Current Authentication/Authorization Mechanism

### 5.1 Summary: **NONE**

The current implementation has **no authentication layer**. Access is controlled entirely by:

1. **Bot token** - Anyone with the token can send commands
2. **Project name** - Knowing the name allows connection

### 5.2 Current Security Posture

| Aspect | Status | Risk |
|--------|--------|------|
| User authentication | None | High - any Telegram user can interact |
| Project access control | None | High - any user can connect to any project |
| Session isolation | Per-chat-id | Medium - different chats get different sessions |
| Command authorization | None | High - all commands available to all users |
| API key protection | Environment variables | Low - standard practice |

### 5.3 State Persistence

**Critical issue:** Sessions are stored in-memory only.

```rust
sessions: RwLock<HashMap<i64, UserSession>>,
```

On bot restart:
- All session mappings are lost
- Users must re-connect to projects
- No notification to users about disconnect

---

## 6. Implementation Recommendations for /telegram Pairing

### 6.1 Proposed Pairing Flow

```
┌─────────────────┐         ┌─────────────────┐         ┌─────────────────┐
│   CLI Session   │         │  Telegram Bot   │         │   Telegram App  │
└────────┬────────┘         └────────┬────────┘         └────────┬────────┘
         │                           │                           │
         │ /telegram                 │                           │
         │ ────────────────────────▶ │                           │
         │                           │                           │
         │ Generate 6-digit code     │                           │
         │ ◀──────────────────────── │                           │
         │                           │                           │
         │ Display: "Code: 123456"   │                           │
         │ "Valid for 5 minutes"     │                           │
         │                           │                           │
         │                           │ User sends: /pair 123456  │
         │                           │ ◀────────────────────────  │
         │                           │                           │
         │                           │ Validate code             │
         │                           │ Create chat-session map   │
         │                           │                           │
         │ Receive: "Paired!"        │ Send: "Connected to X"    │
         │ ◀──────────────────────── │ ─────────────────────────▶│
         │                           │                           │
```

### 6.2 Required Changes

#### 6.2.1 New State Fields (state.rs)

```rust
pub struct TelegramState {
    // Existing fields...

    // NEW: Pending pairing codes
    // Maps start_code -> (tmux_session, project_name, created_at)
    pending_pairings: RwLock<HashMap<String, PendingPairing>>,

    // NEW: Authorized pairings (persisted)
    // Maps chat_id -> List of authorized session names
    authorized_chats: RwLock<HashMap<i64, HashSet<String>>>,
}

struct PendingPairing {
    tmux_session: String,
    project_name: String,
    created_at: Instant,
    // Optional: specific chat_id if initiated from bot side
    for_chat_id: Option<i64>,
}
```

#### 6.2.2 New Handler: /pair (handlers.rs)

```rust
#[command(description = "Pair with a session using start code: /pair <code>")]
Pair(String),

pub async fn handle_pair(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    code: String,
) -> ResponseResult<()> {
    // 1. Validate code format (6 alphanumeric characters)
    // 2. Look up code in pending_pairings
    // 3. Check expiration (5 minutes)
    // 4. Create UserSession
    // 5. Add chat_id to authorized_chats
    // 6. Remove from pending_pairings
    // 7. Notify CLI session (via tmux output or IPC)
}
```

#### 6.2.3 New CLI Command: /telegram

Add to `commander-cli` a command that:
1. Generates a 6-character alphanumeric code
2. Registers it with the Telegram bot state
3. Displays the code and waits for pairing
4. Confirms when pairing succeeds

#### 6.2.4 IPC Mechanism Options

| Option | Pros | Cons |
|--------|------|------|
| File-based (JSON in state_dir) | Simple, works across processes | Polling required, slower |
| Unix socket | Fast, bidirectional | More complex, platform-specific |
| SQLite (StateStore) | Persistent, queryable | Existing infrastructure |
| Redis/pub-sub | Real-time, scalable | External dependency |

**Recommendation:** Use StateStore (SQLite) for persistence + file-based signaling for pairing events.

### 6.3 Security Considerations for Start Codes

#### 6.3.1 Code Generation

```rust
fn generate_pairing_code() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // No I,O,0,1 for clarity  pragma: allowlist secret
    let mut rng = rand::thread_rng();
    (0..6)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}
```

#### 6.3.2 Security Properties

| Property | Implementation |
|----------|----------------|
| **Entropy** | 6 chars from 32 charset = 32^6 = ~1 billion combinations |
| **Expiration** | 5 minute TTL, auto-cleanup |
| **Single-use** | Code removed after successful pairing |
| **Rate limiting** | Max 3 codes per session, max 5 attempts per chat_id |
| **Timing attacks** | Constant-time comparison for code validation |

#### 6.3.3 Attack Vectors and Mitigations

| Attack | Mitigation |
|--------|------------|
| Brute force | Rate limiting + 5 min expiration |
| Code interception | Short validity window, single use |
| Replay | Code deleted after use |
| Enumeration | Don't reveal if code exists vs expired |
| Social engineering | Codes are session-specific, not user-specific |

### 6.4 Persistence Recommendations

Add new table/file structure for authorized pairings:

```json
// ~/.local/share/commander/telegram_pairings.json
{
  "pairings": [
    {
      "chat_id": 123456789,
      "session_name": "commander-myproject",
      "project_name": "myproject",
      "authorized_at": "2026-02-01T12:00:00Z",
      "last_used": "2026-02-01T12:30:00Z"
    }
  ],
  "pending_codes": [
    {
      "code": "ABC123",
      "session_name": "commander-otherproject",
      "created_at": "2026-02-01T12:00:00Z",
      "expires_at": "2026-02-01T12:05:00Z"
    }
  ]
}
```

---

## 7. Action Items

### 7.1 Immediate (Bug Fixes)

- [ ] Clarify error messages for different "session not found" scenarios
- [ ] Add session state persistence (survives bot restart)
- [ ] Improve logging for debugging connection issues

### 7.2 Short-term (Pairing Implementation)

- [ ] Implement `PendingPairing` state structure
- [ ] Add `/pair` command handler
- [ ] Create `/telegram` CLI command in commander-cli
- [ ] Implement code generation with proper entropy
- [ ] Add expiration cleanup task

### 7.3 Medium-term (Security Hardening)

- [ ] Implement rate limiting on pairing attempts
- [ ] Add authorized_chats persistence
- [ ] Consider optional chat-id whitelisting
- [ ] Add audit logging for security events

---

## 8. References

### 8.1 Source Files Analyzed

- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/lib.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/main.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/session.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/error.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/ngrok.rs`

### 8.2 Dependencies

- `teloxide` - Telegram bot framework
- `commander-tmux` - Tmux session management
- `commander-persistence` - State storage
- `commander-adapters` - Tool adapter registry

---

*Research completed: 2026-02-01*
