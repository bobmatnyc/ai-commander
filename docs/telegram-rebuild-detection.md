# Telegram Bot Rebuild Detection and Auto-Reconnect

## Overview

This implementation adds intelligent session persistence and rebuild detection to the Telegram bot, allowing it to:

1. **Persist active sessions** to disk on connect/disconnect
2. **Detect rebuilds** vs simple restarts using binary hash tracking
3. **Auto-restore sessions** on startup (with validation)
4. **Notify users** about rebuild status and reconnection results

## Architecture

### Phase 1: Session Persistence

#### Files Modified/Created:
- `crates/commander-telegram/src/session.rs` - Added `PersistedSession` struct
- `crates/commander-telegram/src/state.rs` - Added persistence methods

#### Key Components:

**PersistedSession Struct:**
```rust
pub struct PersistedSession {
    pub chat_id: i64,
    pub project_path: String,
    pub project_name: String,
    pub tmux_session: String,
    pub thread_id: Option<i32>,
    pub worktree_info: Option<WorktreeInfo>,
    pub created_at: u64,
    pub last_activity: u64,
}
```

**Validation Logic:**
- Sessions must be < 24 hours old
- Associated tmux session must still exist
- Invalid sessions are logged and skipped

**Storage:**
- Path: `~/.ai-commander/state/telegram_sessions.json`
- Format: JSON with pretty-printing
- Auto-saved on connect/disconnect

#### Methods Added:

```rust
// TelegramState methods
pub async fn save_sessions(&self)
pub async fn load_sessions(&self) -> (usize, usize)

// PersistedSession methods
pub fn from_user_session(session: &UserSession) -> Self
pub fn age_seconds(&self) -> u64
pub fn is_valid(&self) -> bool
pub fn restore_to_user_session(&self) -> UserSession
```

### Phase 2: Rebuild Detection

#### Files Created:
- `crates/commander-telegram/src/version.rs` - Version tracking module

#### Key Components:

**BotVersion Struct:**
```rust
pub struct BotVersion {
    pub binary_hash: u64,
    pub last_start: u64,
    pub start_count: u64,
}
```

**Hash Computation:**
- Uses binary file size + modification time as proxy
- Fallback to compile-time version info
- Detects actual code changes vs simple restarts

**Storage:**
- Path: `~/.ai-commander/state/bot_version.json`
- Updated on every bot start
- Tracks start count for monitoring

#### Functions:

```rust
pub fn check_rebuild() -> (bool, bool, u64)
pub fn load_version() -> BotVersion
pub fn save_version(version: &BotVersion)
```

### Phase 3: Auto-Reconnect and Notifications

#### Files Modified:
- `crates/commander-telegram/src/bot.rs` - Added startup restoration logic

#### Startup Flow:

1. **Check rebuild status**
   ```rust
   let (is_rebuild, is_first_start, start_count) = check_rebuild();
   ```

2. **Restore sessions**
   ```rust
   let (restored_count, total_count) = state.load_sessions().await;
   ```

3. **Send notifications** (only on rebuild, not first start)
   ```rust
   if is_rebuild && !is_first_start {
       send_rebuild_notification(bot, state, restored_count, total_count).await;
   }
   ```

#### Notification Messages:

**All sessions restored:**
```
🔄 Bot rebuilt and restarted.
✅ Successfully restored 3 session(s).
```

**Partial restoration:**
```
🔄 Bot rebuilt and restarted.
✅ Restored 2 of 3 session(s).
⚠️ 1 session(s) could not be restored (expired or tmux session not found).
```

**No sessions:**
```
🔄 Bot rebuilt and restarted.
No active sessions to restore.
```

**No restoration:**
```
🔄 Bot rebuilt and restarted.
⚠️ Could not restore 2 session(s) (expired or tmux session not found).
```

## Testing

### Unit Tests

**Session Module (`session.rs`):**
- ✅ `test_new_session` - Session creation
- ✅ `test_response_collection` - Response buffering
- ✅ `test_progress_messages` - Progress tracking
- ✅ `test_incremental_summaries` - Summary generation

**Version Module (`version.rs`):**
- ✅ `test_bot_version_creation` - Version initialization
- ✅ `test_bot_version_update` - Update logic
- ✅ `test_hash_string` - Hash computation

### Integration Tests

**Rebuild Detection (`tests/rebuild_detection_test.rs`):**
- ✅ `test_version_tracking` - Version state transitions
- ✅ `test_version_persistence` - Disk persistence
- ✅ `test_persisted_session_validation` - Age validation
- ✅ `test_session_restoration` - Session reconstruction
- ✅ `test_session_serialization` - JSON serialization

**Test Results:**
```
running 49 tests (43 unit + 5 integration + 1 doc)
test result: ok. 49 passed; 0 failed
```

## User Experience

### On Rebuild (Code Change)

1. **Bot detects rebuild** via binary hash change
2. **Loads persisted sessions** from disk
3. **Validates each session**:
   - Age < 24 hours ✅
   - tmux session exists ✅
4. **Restores valid sessions** automatically
5. **Sends notification** to all authorized chats

### On Simple Restart (No Code Change)

1. **Bot detects restart** (same binary hash)
2. **Restores sessions** silently
3. **No notification sent** (to avoid noise)

### Session Expiry

Sessions older than 24 hours are **automatically expired** because:
- Long-lived sessions may have stale state
- tmux sessions may have been manually killed
- Project paths may have changed
- Users likely don't expect restoration after a day

## File Structure

```
~/.ai-commander/state/
├── telegram_sessions.json   # Persisted session data
├── bot_version.json          # Version tracking
├── authorized_chats.json     # Authorization state
├── group_configs.json        # Group chat configs
└── pairings.json            # Pairing codes
```

## Benefits

### For Users
- 🔄 **Seamless reconnection** after bot updates
- 🔔 **Clear status notifications** on rebuild
- ⚡ **No manual reconnection** needed
- 🛡️ **Safe expiry** prevents stale sessions

### For Developers
- 🐛 **Easier debugging** with version tracking
- 📊 **Start count metrics** for monitoring
- 🧪 **Comprehensive test coverage** (49 tests)
- 📝 **Clear separation** of rebuild vs restart

## Future Enhancements

Possible improvements for future iterations:

1. **Configurable expiry** - Allow users to set custom session TTL
2. **Session migration** - Handle project path changes
3. **Partial state restoration** - Restore response buffers, pending queries
4. **Metrics collection** - Track restoration success rates
5. **Admin notifications** - Separate channel for bot health monitoring

## Implementation Statistics

- **New files**: 2 (version.rs, rebuild_detection_test.rs)
- **Modified files**: 4 (session.rs, state.rs, bot.rs, lib.rs)
- **New structs**: 2 (PersistedSession, BotVersion)
- **New methods**: 8 (save/load sessions, version tracking, etc.)
- **New tests**: 5 integration tests
- **Total tests**: 49 (all passing)
- **Lines of code**: ~500 new lines

## Configuration

No additional configuration required! The feature works out of the box:

- ✅ Uses existing state directory (`~/.ai-commander/state/`)
- ✅ Uses existing authorization system
- ✅ Uses existing tmux session names
- ✅ Compatible with both 1:1 and group chats
- ✅ Compatible with worktree sessions

## Compatibility

Works with all existing features:
- ✅ Standard project connections
- ✅ Git worktree sessions (`/connect-tree`)
- ✅ Forum group topics
- ✅ Multiple adapters (claude-code, mpm)
- ✅ Agent orchestrator integration
- ✅ Notification system

## Error Handling

The implementation is robust against:

- 📁 Missing state files → Creates new
- 🔧 Corrupted JSON → Logs error, creates new
- ⏰ Expired sessions → Skips restoration
- 💀 Dead tmux sessions → Skips restoration
- 🚫 Permission errors → Logs error, continues

All errors are **logged but not fatal** - the bot continues to function normally.
