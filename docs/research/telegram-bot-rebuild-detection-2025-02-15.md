# Telegram Bot Rebuild Detection and Auto-Reconnection

**Date:** 2026-02-15
**Status:** Investigation Complete - Implementation Plan Ready
**Type:** Feature Enhancement

## Executive Summary

This research investigates implementing rebuild detection and auto-reconnection for the Telegram bot component of ai-commander. The system currently restarts the bot when the TUI/REPL launches, but does not notify users or automatically reconnect to previous sessions after a rebuild.

**Key Findings:**
1. Bot lifecycle is managed through PID file (`~/.ai-commander/state/telegram.pid`)
2. Session state is stored in-memory (not persisted across restarts)
3. Authorized chats are persisted (`authorized_chats.json`)
4. No mechanism exists to detect rebuild vs normal start
5. No user notification system for bot restarts

**Recommended Solution:** Implement session state persistence with rebuild detection marker

---

## Current Architecture

### Bot Lifecycle Management

**Location:** `crates/ai-commander/src/lib.rs`

The bot lifecycle follows this pattern:

```
TUI/REPL Start
  ↓
restart_telegram_if_running()
  ↓
Kill existing process (via PID file)
  ↓
start_telegram_daemon()
  ↓
Spawn background process
  ↓
Write new PID to telegram.pid
```

**Key Functions:**
- `restart_telegram_if_running()` - Called on TUI/REPL startup (lines 165-200)
- `start_telegram_daemon()` - Spawns bot as detached process (lines 62-108)
- `is_telegram_running()` - Checks if PID from file is still active (lines 29-46)

### Session State Management

**Location:** `crates/commander-telegram/src/state.rs`

Current state structure:
```rust
pub struct TelegramState {
    sessions: RwLock<HashMap<i64, UserSession>>,  // In-memory only
    tmux: Option<TmuxOrchestrator>,
    adapters: AdapterRegistry,
    store: StateStore,
    authorized_chats: RwLock<HashSet<i64>>,       // Persisted
    group_configs: RwLock<HashMap<i64, GroupChatConfig>>, // Persisted
}
```

**Persisted Data:**
- `authorized_chats.json` - List of chat IDs authorized for the instance
- `group_configs.json` - Forum topic configurations
- `pairings.json` - Temporary pairing codes (5-minute TTL)
- `notifications.json` - Pending notifications queue

**NOT Persisted:**
- Active user sessions (chat_id → session mapping)
- Response collection buffers
- Pending queries and message IDs
- Connection state to tmux sessions

### User Session Structure

**Location:** `crates/commander-telegram/src/session.rs`

```rust
pub struct UserSession {
    chat_id: ChatId,
    project_path: String,
    project_name: String,
    tmux_session: String,
    response_buffer: Vec<String>,      // Lost on restart
    last_output_time: Option<Instant>, // Lost on restart
    last_output: String,               // Lost on restart
    pending_query: Option<String>,     // Lost on restart
    is_waiting: bool,                  // Lost on restart
    pending_message_id: Option<MessageId>, // Lost on restart
    thread_id: Option<ThreadId>,       // For forum topics
    worktree_info: Option<WorktreeInfo>,
}
```

**Impact of Restart:**
- Users lose active session connections
- Pending queries are abandoned
- Response collection is interrupted
- No notification of restart

---

## Problem Analysis

### 1. How to Detect Rebuild vs Normal Start

**Current State:**
- Bot has no concept of "rebuild" vs "first start"
- No persistent marker to track versions
- No way to distinguish intentional restart from crash recovery

**Detection Strategies:**

#### Option A: Version Marker File (Recommended)
```
~/.ai-commander/state/bot_version.json
{
  "version": "0.3.0",
  "binary_hash": "sha256:abc123...",
  "last_start": "2026-02-15T14:30:00Z",
  "start_count": 42
}
```

**Pros:**
- Simple to implement
- Can detect version changes
- Can track restart patterns
- No external dependencies

**Cons:**
- Requires binary hash calculation on each start
- Version bumps require manual update

#### Option B: Compile-Time Build ID
```rust
const BUILD_ID: &str = env!("BUILD_ID");  // Set at compile time
```

**Pros:**
- Automatic change detection
- No runtime overhead

**Cons:**
- Requires build system changes
- May trigger on unrelated rebuilds

#### Option C: File Modification Time
Compare binary modification time to last known start time.

**Pros:**
- No additional files needed
- Simple logic

**Cons:**
- Can be fooled by `touch`
- Doesn't work for same-binary restarts

**Recommendation:** Option A (Version Marker File) provides the best balance of simplicity and reliability.

### 2. How to Persist Session State

**Current Sessions Data Model:**
```rust
sessions: RwLock<HashMap<i64, UserSession>>
```

**Persistence Requirements:**
- Must survive process restart
- Must be quick to load (< 100ms)
- Must handle concurrent access
- Must expire stale sessions

**Storage Options:**

#### Option A: JSON State File (Recommended)
```
~/.ai-commander/state/telegram_sessions.json
{
  "format_version": 1,
  "last_save": "2026-02-15T14:30:00Z",
  "sessions": [
    {
      "chat_id": 123456789,
      "project_path": "/Users/masa/Projects/myapp",
      "project_name": "myapp",
      "tmux_session": "commander-myapp",
      "thread_id": null,
      "connected_at": "2026-02-15T14:00:00Z",
      "last_activity": "2026-02-15T14:29:55Z",
      "worktree_info": null
    }
  ]
}
```

**Pros:**
- Human-readable for debugging
- Simple serialization (serde)
- Compatible with existing persistence pattern
- Easy migration path

**Cons:**
- Slightly slower than binary format
- Larger file size

#### Option B: SQLite Database
Use StateStore (already in crate) to store sessions.

**Pros:**
- Better for complex queries
- Atomic transactions
- Built-in indexing

**Cons:**
- Overkill for simple key-value storage
- Adds dependency complexity
- Migration overhead

**Recommendation:** Option A (JSON) matches the existing pattern (`authorized_chats.json`, `group_configs.json`) and is sufficient for the scale.

### 3. How to Notify Users

**Notification Requirements:**
- Must reach all authorized chats
- Must include rebuild reason (version change, manual restart, crash)
- Should not spam on every restart
- Must work even if user has no active session

**Notification Strategies:**

#### Option A: Broadcast on Startup (Recommended)
```rust
async fn notify_rebuild(&self, bot: &Bot) {
    let chat_ids = self.get_authorized_chat_ids().await;
    let message = "🔄 Bot restarted (new version available)";

    for chat_id in chat_ids {
        let _ = bot.send_message(ChatId(chat_id), message).await;
    }
}
```

**Pros:**
- Immediate notification
- Simple implementation
- Guaranteed delivery

**Cons:**
- Can't be disabled per-user
- Sends to all chats regardless of activity

#### Option B: On-Demand Notification
Store rebuild event, notify on next user interaction.

**Pros:**
- Less intrusive
- User-triggered

**Cons:**
- Delayed notification
- User may miss rebuild if inactive

**Recommendation:** Option A with rebuild detection (only notify if version changed).

### 4. How to Auto-Reconnect

**Reconnection Requirements:**
- Must restore chat_id → tmux session mapping
- Must validate tmux session still exists
- Must handle case where project was deleted
- Must restore forum topic routing

**Reconnection Flow:**

```
Bot Startup
  ↓
Load telegram_sessions.json
  ↓
For each saved session:
  ├─ Check if tmux session exists
  ├─ Check if project path still valid
  ├─ Restore session to in-memory state
  └─ Validate forum topic mapping (if applicable)
  ↓
Notify users of restored connections
```

**Edge Cases:**
1. **Tmux session doesn't exist** → Notify user, offer to recreate
2. **Project path deleted** → Remove session, notify user
3. **Forum topic deleted** → Clear topic mapping, notify user
4. **Session expired** (> 24 hours inactive) → Don't restore, clean up

---

## Implementation Plan

### Phase 1: Persistence Infrastructure

**Files to Modify:**
- `crates/commander-telegram/src/state.rs`
- `crates/commander-telegram/src/session.rs`

**Changes:**

1. **Add Serializable Session Type**
```rust
// session.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub chat_id: i64,
    pub project_path: String,
    pub project_name: String,
    pub tmux_session: String,
    pub thread_id: Option<i32>,
    pub connected_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub worktree_info: Option<WorktreeInfo>,
}

impl From<&UserSession> for PersistedSession {
    fn from(session: &UserSession) -> Self {
        Self {
            chat_id: session.chat_id.0,
            project_path: session.project_path.clone(),
            project_name: session.project_name.clone(),
            tmux_session: session.tmux_session.clone(),
            thread_id: session.thread_id.map(|t| t.0.0),
            connected_at: Utc::now(), // Approximate
            last_activity: Utc::now(),
            worktree_info: session.worktree_info.clone(),
        }
    }
}
```

2. **Add Session Persistence Methods to TelegramState**
```rust
// state.rs
impl TelegramState {
    /// Save active sessions to disk.
    pub async fn save_sessions(&self) -> Result<()> {
        let sessions = self.sessions.read().await;
        let persisted: Vec<PersistedSession> = sessions
            .values()
            .map(|s| PersistedSession::from(s))
            .collect();

        let data = SessionsFile {
            format_version: 1,
            last_save: Utc::now(),
            sessions: persisted,
        };

        let path = runtime_state_dir().join("telegram_sessions.json");
        let json = serde_json::to_string_pretty(&data)?;
        std::fs::write(path, json)?;

        Ok(())
    }

    /// Load sessions from disk and restore active connections.
    pub async fn load_sessions(&self, bot: &Bot) -> Result<Vec<RestoredSession>> {
        let path = runtime_state_dir().join("telegram_sessions.json");
        if !path.exists() {
            return Ok(Vec::new());
        }

        let json = std::fs::read_to_string(&path)?;
        let data: SessionsFile = serde_json::from_str(&json)?;

        let mut restored = Vec::new();
        let mut sessions = self.sessions.write().await;

        for persisted in data.sessions {
            // Validate session can be restored
            if let Some(session) = self.try_restore_session(persisted).await {
                let chat_id = session.chat_id.0;
                sessions.insert(chat_id, session.clone());
                restored.push(RestoredSession {
                    chat_id,
                    project_name: session.project_name.clone(),
                });
            }
        }

        Ok(restored)
    }

    /// Try to restore a session, validating all preconditions.
    async fn try_restore_session(&self, persisted: PersistedSession) -> Option<UserSession> {
        // Check session age
        let age = Utc::now() - persisted.last_activity;
        if age.num_hours() > 24 {
            warn!(
                chat_id = persisted.chat_id,
                "Session too old, not restoring"
            );
            return None;
        }

        // Validate tmux session exists
        if let Some(tmux) = &self.tmux {
            if !tmux.session_exists(&persisted.tmux_session) {
                warn!(
                    session = %persisted.tmux_session,
                    "Tmux session no longer exists"
                );
                return None;
            }
        } else {
            return None;
        }

        // Validate project path
        let path = Path::new(&persisted.project_path);
        if !path.exists() {
            warn!(
                path = %persisted.project_path,
                "Project path no longer exists"
            );
            return None;
        }

        // Restore session
        Some(if let Some(thread_id) = persisted.thread_id {
            UserSession::with_thread_id(
                ChatId(persisted.chat_id),
                persisted.project_path,
                persisted.project_name,
                persisted.tmux_session,
                ThreadId(MessageId(thread_id)),
            )
        } else {
            UserSession::new(
                ChatId(persisted.chat_id),
                persisted.project_path,
                persisted.project_name,
                persisted.tmux_session,
            )
        })
    }
}
```

3. **Auto-Save on Session Changes**
```rust
// Add auto-save after connect/disconnect operations
impl TelegramState {
    pub async fn connect(&self, chat_id: ChatId, project_name: &str) -> Result<(String, String)> {
        // ... existing connection logic ...

        // Auto-save after connection
        if let Err(e) = self.save_sessions().await {
            warn!(error = %e, "Failed to save sessions");
        }

        Ok((project_name.to_string(), tool_id))
    }

    pub async fn disconnect(&self, chat_id: ChatId) -> Result<String> {
        // ... existing disconnection logic ...

        // Auto-save after disconnection
        if let Err(e) = self.save_sessions().await {
            warn!(error = %e, "Failed to save sessions");
        }

        Ok(project_name)
    }
}
```

### Phase 2: Rebuild Detection

**Files to Create:**
- `crates/commander-telegram/src/version.rs`

**Files to Modify:**
- `crates/commander-telegram/src/bot.rs`
- `crates/commander-telegram/src/main.rs`

**Changes:**

1. **Create Version Tracking Module**
```rust
// version.rs
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotVersion {
    pub version: String,
    pub last_start: DateTime<Utc>,
    pub start_count: u64,
}

impl BotVersion {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            last_start: Utc::now(),
            start_count: 1,
        }
    }

    pub fn load() -> Option<Self> {
        let path = version_file();
        if !path.exists() {
            return None;
        }

        let json = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&json).ok()
    }

    pub fn save(&self) -> Result<()> {
        let path = version_file();
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn increment_start_count(&mut self) {
        self.start_count += 1;
        self.last_start = Utc::now();
    }
}

fn version_file() -> PathBuf {
    commander_core::config::runtime_state_dir().join("bot_version.json")
}

pub fn detect_rebuild() -> bool {
    match BotVersion::load() {
        Some(prev) => {
            let current = BotVersion::current();
            prev.version != current.version
        }
        None => false, // First start
    }
}
```

2. **Integrate Rebuild Detection in Bot Startup**
```rust
// bot.rs
impl TelegramBot {
    pub async fn start_polling(&self) -> Result<()> {
        info!("Starting Telegram bot in polling mode...");

        // Detect if this is a rebuild
        let is_rebuild = version::detect_rebuild();

        // Load and update version
        let mut version = version::BotVersion::load()
            .unwrap_or_else(version::BotVersion::current);
        version.increment_start_count();
        version.save().ok();

        // Restore sessions from previous run
        let restored = self.state.load_sessions(&self.bot).await?;

        // Notify users if rebuild detected
        if is_rebuild && !restored.is_empty() {
            self.notify_rebuild(&restored).await;
        }

        // ... rest of existing startup logic ...
    }

    async fn notify_rebuild(&self, restored: &[RestoredSession]) {
        let bot = &self.bot;
        let version = env!("CARGO_PKG_VERSION");

        for session in restored {
            let message = format!(
                "🔄 Bot restarted (v{})\n\n\
                 ✅ Reconnected to: {}\n\n\
                 You can continue sending messages.",
                version,
                session.project_name
            );

            let _ = bot.send_message(ChatId(session.chat_id), message).await;
        }
    }
}
```

### Phase 3: TUI Indicator

**Files to Modify:**
- `crates/ai-commander/src/tui/ui.rs`
- `crates/ai-commander/src/tui/app.rs`

**Changes:**

1. **Add Rebuild Status to App State**
```rust
// app.rs
pub struct App {
    // ... existing fields ...
    pub telegram_rebuild_status: TelegramRebuildStatus,
}

#[derive(Debug, Clone)]
pub enum TelegramRebuildStatus {
    NotRunning,
    Running { is_rebuild: bool, restored_count: usize },
}

impl App {
    pub fn new(state_dir: &std::path::Path) -> Self {
        // ... existing initialization ...

        // Check Telegram status after restart
        let telegram_rebuild_status = check_telegram_rebuild_status();

        Self {
            // ... existing fields ...
            telegram_rebuild_status,
        }
    }
}

fn check_telegram_rebuild_status() -> TelegramRebuildStatus {
    if !crate::is_telegram_running() {
        return TelegramRebuildStatus::NotRunning;
    }

    // Read rebuild marker if it exists
    let marker_path = commander_core::config::runtime_state_dir()
        .join("telegram_rebuild.marker");

    if marker_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&marker_path) {
            if let Ok(count) = content.trim().parse::<usize>() {
                // Clear marker after reading
                let _ = std::fs::remove_file(&marker_path);
                return TelegramRebuildStatus::Running {
                    is_rebuild: true,
                    restored_count: count,
                };
            }
        }
    }

    TelegramRebuildStatus::Running {
        is_rebuild: false,
        restored_count: 0,
    }
}
```

2. **Display Rebuild Status in UI**
```rust
// ui.rs
fn render_status_bar<B: Backend>(f: &mut Frame<B>, app: &App, area: Rect) {
    // ... existing status bar rendering ...

    // Add rebuild indicator
    let rebuild_indicator = match &app.telegram_rebuild_status {
        TelegramRebuildStatus::Running { is_rebuild: true, restored_count } => {
            format!(" 🔄 Bot rebuilt ({} sessions restored)", restored_count)
        }
        TelegramRebuildStatus::Running { is_rebuild: false, .. } => {
            " 📱 Bot running".to_string()
        }
        TelegramRebuildStatus::NotRunning => {
            " 📱 Bot offline".to_string()
        }
    };

    // Append to status bar
    // ... (integrate with existing status bar rendering)
}
```

3. **Write Rebuild Marker on Bot Startup**
```rust
// bot.rs (in notify_rebuild)
async fn notify_rebuild(&self, restored: &[RestoredSession]) {
    // ... existing notification logic ...

    // Write marker for TUI to detect
    let marker_path = runtime_state_dir().join("telegram_rebuild.marker");
    let _ = std::fs::write(&marker_path, restored.len().to_string());
}
```

---

## Testing Strategy

### Unit Tests

1. **Session Persistence**
```rust
#[tokio::test]
async fn test_save_and_load_sessions() {
    let temp_dir = tempfile::tempdir().unwrap();
    let state = TelegramState::new(temp_dir.path());

    // Create test session
    let session = UserSession::new(
        ChatId(12345),
        "/test/path".to_string(),
        "test-project".to_string(),
        "test-session".to_string(),
    );

    state.sessions.write().await.insert(12345, session);

    // Save
    state.save_sessions().await.unwrap();

    // Clear and reload
    state.sessions.write().await.clear();
    let restored = state.load_sessions(&bot).await.unwrap();

    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].chat_id, 12345);
}
```

2. **Rebuild Detection**
```rust
#[test]
fn test_rebuild_detection() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::env::set_var("COMMANDER_STATE_DIR", temp_dir.path());

    // First start - no rebuild
    assert!(!version::detect_rebuild());

    let mut version = version::BotVersion::current();
    version.save().unwrap();

    // Same version - no rebuild
    assert!(!version::detect_rebuild());

    // Change version
    version.version = "999.0.0".to_string();
    version.save().unwrap();

    // Different version - rebuild detected
    assert!(version::detect_rebuild());
}
```

3. **Session Validation**
```rust
#[tokio::test]
async fn test_expired_session_not_restored() {
    let persisted = PersistedSession {
        chat_id: 12345,
        last_activity: Utc::now() - chrono::Duration::hours(25),
        // ... other fields ...
    };

    let state = TelegramState::new(&temp_dir);
    let restored = state.try_restore_session(persisted).await;

    assert!(restored.is_none(), "Old session should not be restored");
}
```

### Integration Tests

1. **End-to-End Rebuild Flow**
```bash
# Terminal 1: Start bot
cargo run -p commander-telegram

# Terminal 2: Connect via Telegram
# Send /connect command

# Terminal 1: Stop bot (Ctrl+C)

# Terminal 1: Rebuild and restart
cargo build -p commander-telegram --release
cargo run -p commander-telegram

# Terminal 2: Verify notification received
# Verify session still works without re-pairing
```

2. **Stress Test: Multiple Restarts**
```bash
for i in {1..10}; do
    cargo run -p commander-telegram &
    sleep 2
    kill %1
    wait
done

# Verify:
# - No duplicate notifications
# - Session persistence works across all restarts
# - No state corruption
```

---

## Migration Plan

### Version 0.3.1 (Initial Implementation)

**Files Added:**
- `crates/commander-telegram/src/version.rs`
- `docs/research/telegram-bot-rebuild-detection-2025-02-15.md` (this file)

**Files Modified:**
- `crates/commander-telegram/src/state.rs` (persistence methods)
- `crates/commander-telegram/src/session.rs` (serializable types)
- `crates/commander-telegram/src/bot.rs` (rebuild detection)
- `crates/ai-commander/src/tui/app.rs` (status tracking)
- `crates/ai-commander/src/tui/ui.rs` (rebuild indicator)

**Migration Steps:**
1. Sessions from 0.3.0 are NOT migrated (require re-pairing)
2. New session persistence kicks in after first 0.3.1 connection
3. Rebuild detection only works starting from 0.3.1 → 0.3.2+

**User Communication:**
```
⚠️ Breaking Change in v0.3.1:
After upgrading, you'll need to re-pair your Telegram bot using `/telegram`
and `/pair <code>` once. After that, sessions will persist across restarts.
```

### Version 0.3.2+ (Enhancements)

**Potential Improvements:**
1. **Session expiry configuration** - Allow users to set custom timeout
2. **Session health checks** - Ping tmux sessions periodically
3. **Graceful session transfer** - Migrate sessions without restart
4. **Multi-instance support** - Handle multiple bots per state directory

---

## Edge Cases & Error Handling

### 1. Corrupt Session File

**Scenario:** `telegram_sessions.json` contains invalid JSON

**Handling:**
```rust
pub async fn load_sessions(&self, bot: &Bot) -> Result<Vec<RestoredSession>> {
    let path = runtime_state_dir().join("telegram_sessions.json");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => {
            error!(error = %e, "Failed to read sessions file");
            return Ok(Vec::new());
        }
    };

    let data: SessionsFile = match serde_json::from_str(&json) {
        Ok(d) => d,
        Err(e) => {
            error!(error = %e, "Corrupt sessions file, backing up and resetting");
            let backup_path = path.with_extension("json.backup");
            let _ = std::fs::copy(&path, &backup_path);
            let _ = std::fs::remove_file(&path);
            return Ok(Vec::new());
        }
    };

    // ... rest of load logic ...
}
```

### 2. Tmux Session Doesn't Exist

**Scenario:** User deleted tmux session manually

**Handling:** Session restoration fails gracefully, user notified on next interaction

### 3. Concurrent Bot Instances

**Scenario:** User accidentally starts two bots

**Current Behavior:** PID file overwritten, first bot loses tracking

**Fix (Future):**
- Add PID validation before starting
- Check if process with PID in file is actually commander-telegram
- Refuse to start if another instance running

### 4. Disk Full During Save

**Scenario:** No space to write `telegram_sessions.json`

**Handling:**
```rust
pub async fn save_sessions(&self) -> Result<()> {
    let path = runtime_state_dir().join("telegram_sessions.json");
    let temp_path = path.with_extension("json.tmp");

    // Write to temporary file first
    let json = serde_json::to_string_pretty(&data)?;
    std::fs::write(&temp_path, json)?;

    // Atomic rename (on same filesystem)
    std::fs::rename(&temp_path, &path)?;

    Ok(())
}
```

---

## Performance Considerations

### Session Load Time

**Estimated Load Time:**
- 1 session: < 1ms
- 10 sessions: < 5ms
- 100 sessions: < 20ms (unlikely to have this many)

**Optimization:** Load happens once at startup, minimal impact.

### Auto-Save Frequency

**Current Plan:** Save on connect/disconnect only

**Alternative:** Periodic auto-save (every 5 minutes)

**Trade-off:**
- Periodic: Better durability, more I/O
- On-change: Less I/O, slight risk of data loss

**Recommendation:** Start with on-change, add periodic in 0.4.0 if needed.

### Memory Usage

**Additional Memory per Session:**
- Before: ~1KB (in-memory UserSession)
- After: ~1KB + ~500 bytes (serialized on disk)

**Total Impact:** Negligible (< 100KB even with 100 sessions)

---

## Security Considerations

### 1. Session File Permissions

**Risk:** Unauthorized read of `telegram_sessions.json` reveals project paths

**Mitigation:**
```rust
use std::os::unix::fs::PermissionsExt;

pub async fn save_sessions(&self) -> Result<()> {
    // ... save logic ...

    #[cfg(unix)]
    {
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o600); // Only owner can read/write
        std::fs::set_permissions(&path, perms)?;
    }

    Ok(())
}
```

### 2. Session Hijacking

**Risk:** Attacker with access to state directory could modify sessions

**Mitigation:**
- Already mitigated: Bot requires pairing code (5-minute TTL)
- Authorized chats list is separate protection layer
- Session restoration validates tmux session exists

### 3. PID File Race Condition

**Risk:** Two processes write PID file simultaneously

**Mitigation:**
- Use atomic write (write to .tmp, rename)
- Add file locking (future enhancement)

---

## Documentation Updates Needed

### User-Facing Documentation

1. **README.md**
```markdown
### Telegram Bot Persistence (v0.3.1+)

The Telegram bot now persists your session connections across restarts:

- **Auto-reconnection**: Sessions restore automatically after bot restart
- **Rebuild notifications**: Get notified when bot updates
- **TUI indicator**: See rebuild status in the terminal UI

**Note:** Sessions expire after 24 hours of inactivity.
```

2. **Bot Help Command**
```
/help response should include:

Sessions persist across bot restarts. If you see a "Bot restarted"
message, your connection is automatically restored - just continue
sending messages.
```

### Developer Documentation

1. **Architecture Diagram**
```
crates/commander-telegram/
├── src/
│   ├── bot.rs          # Rebuild detection, notify_rebuild()
│   ├── state.rs        # save_sessions(), load_sessions()
│   ├── session.rs      # PersistedSession struct
│   └── version.rs      # BotVersion, detect_rebuild()
└── README.md           # Updated architecture section
```

2. **State File Format Documentation**
```markdown
## State Files

### telegram_sessions.json
Stores active Telegram bot sessions for restoration after restart.

**Format:**
```json
{
  "format_version": 1,
  "last_save": "2026-02-15T14:30:00Z",
  "sessions": [...]
}
```

**Location:** `~/.ai-commander/state/telegram_sessions.json`
**Permissions:** 0600 (owner read/write only)
**Expiry:** Sessions older than 24 hours are not restored
```

---

## Open Questions

### Q1: Should we notify on every restart or only rebuilds?

**Options:**
- A) Only notify on version change (rebuild)
- B) Notify on every restart (including crashes)
- C) Make it configurable

**Recommendation:** A (only rebuilds) to avoid notification spam

**Rationale:**
- Rebuilds are rare (once per release)
- Crashes should be avoided, not normalized
- Users care more about "new version" than "bot restarted"

### Q2: What should happen if session restore fails?

**Options:**
- A) Silent failure, user re-pairs manually
- B) Send error message to user
- C) Retry restoration in background

**Recommendation:** B (send error message)

**Message Example:**
```
⚠️ Could not restore your session to "myproject"

Reason: Tmux session no longer exists

To reconnect, use:
/connect

Need a new pairing code?
Run `/telegram` in the TUI, then use `/pair <code>` here.
```

### Q3: Should we support session migration between bot versions?

**Context:** If UserSession structure changes in future versions

**Options:**
- A) No migration, users re-pair after breaking changes
- B) Version-aware migration (format_version field)
- C) Best-effort migration with fallback to re-pairing

**Recommendation:** B (version-aware migration)

**Implementation:**
```rust
fn migrate_sessions(data: SessionsFile) -> SessionsFile {
    match data.format_version {
        1 => data, // Current format
        2 => migrate_v1_to_v2(data),
        _ => {
            warn!("Unknown session format version, resetting");
            SessionsFile::new()
        }
    }
}
```

---

## Success Criteria

### Functional Requirements ✓

- [ ] Bot detects rebuilds (version change)
- [ ] Sessions persist to JSON file
- [ ] Sessions restore on bot restart
- [ ] Users notified of rebuild
- [ ] TUI shows rebuild status
- [ ] Old sessions (>24h) not restored
- [ ] Invalid sessions fail gracefully

### Non-Functional Requirements ✓

- [ ] Session load time < 50ms
- [ ] Auto-save overhead < 10ms
- [ ] Memory usage increase < 1MB
- [ ] No notification spam (1 per rebuild)
- [ ] File permissions secure (0600)
- [ ] Backward compatible (v0.3.0 → v0.3.1)

### User Experience ✓

- [ ] Zero manual intervention after rebuild
- [ ] Clear notification of what happened
- [ ] No loss of connection state
- [ ] Obvious TUI indicator
- [ ] Helpful error messages

---

## Timeline Estimate

### Phase 1: Persistence (2-3 hours)
- Session serialization types
- save_sessions() / load_sessions()
- Auto-save on connect/disconnect
- Unit tests

### Phase 2: Rebuild Detection (1-2 hours)
- version.rs module
- detect_rebuild() logic
- notify_rebuild() implementation
- Integration with bot startup

### Phase 3: TUI Indicator (1 hour)
- Rebuild status tracking
- UI rendering changes
- Marker file coordination

### Phase 4: Testing & Polish (2-3 hours)
- Integration tests
- Error handling
- Documentation
- Code review

**Total Estimate:** 6-9 hours

---

## Next Steps

1. **Create GitHub Issue**
   - Title: "Telegram Bot: Implement rebuild detection and auto-reconnection"
   - Labels: `enhancement`, `telegram`, `ux`
   - Assign to current sprint

2. **Create Feature Branch**
   ```bash
   git checkout -b feature/telegram-rebuild-detection
   ```

3. **Implement Phase 1** (Session Persistence)
   - Follow implementation plan above
   - Create unit tests
   - Submit PR for phase 1 review

4. **Iterate on Phases 2-3** based on feedback

5. **Update Documentation** after all phases complete

---

## Appendix: Alternative Approaches Considered

### A1: Use StateStore Instead of JSON

**Rejected Because:**
- StateStore is SQLite-based, overkill for simple persistence
- JSON matches existing patterns (authorized_chats.json, etc.)
- Simpler debugging with human-readable format

### A2: Store Sessions in Tmux Itself

**Idea:** Use tmux environment variables to store session metadata

**Rejected Because:**
- Couples bot lifecycle to tmux lifecycle
- Doesn't help with rebuild detection
- Harder to query and manage

### A3: Persistent Bot Process (No Restarts)

**Idea:** Keep bot running, hot-reload code

**Rejected Because:**
- Complex to implement (requires dynamic loading)
- Doesn't solve the problem (still need rebuild detection)
- Adds architectural complexity

### A4: Message Queue for Notifications

**Idea:** Use Redis/RabbitMQ for bot-to-user notifications

**Rejected Because:**
- Adds external dependency
- Overkill for simple broadcast
- Doesn't improve on direct Telegram API

---

## References

- [Teloxide Documentation](https://docs.rs/teloxide/)
- [Telegram Bot API - Chat Management](https://core.telegram.org/bots/api#chat)
- [Serde JSON](https://docs.rs/serde_json/)
- [Rust Async Best Practices](https://rust-lang.github.io/async-book/)

---

**End of Research Document**
