# Auto-Restart and Session Restore Investigation

**Date:** 2026-02-20
**Status:** ✅ Complete
**Context:** Investigation triggered by user expectation that "changes should trigger auto restart and session restore"

## Executive Summary

**Current State:** ✅ Auto-restart on code changes EXISTS via `scripts/dev.sh`
**Session Restore:** ✅ FULLY IMPLEMENTED (commit f97798d)
**User Expectation:** ⚠️ PARTIALLY MET - requires running dev.sh manually

### Key Findings

1. **Auto-restart mechanism EXISTS** but requires manual script execution
2. **Rebuild detection and session restore** is FULLY IMPLEMENTED and working
3. **Not a missing feature** - it's a workflow/documentation issue
4. The system is sophisticated: detects rebuilds vs restarts, validates sessions, sends notifications

## Investigation Details

### 1. File Watching / Auto-Reload ✅ EXISTS

**Tool:** `cargo-watch` (installed by dev.sh if missing)
**Script:** `/Users/masa/Projects/ai-commander/scripts/dev.sh`

```bash
cargo watch \
    -w "$PROJECT_ROOT/crates" \
    -x "build $build_flags -p ai-commander -p commander-telegram" \
    -s "
        pkill -TERM -f '$BINARY_NAME' 2>/dev/null || true
        sleep 0.5
        '$TARGET_PATH' $verbose &
        pkill -HUP -x 'ai-commander' 2>/dev/null || true
    " \
    --why
```

**What it does:**
- Watches `crates/` directory for file changes
- Rebuilds BOTH `ai-commander` (TUI) and `commander-telegram` on changes
- Kills old bot process with SIGTERM (graceful shutdown)
- Starts new bot binary automatically
- Sends SIGHUP to TUI for hot-reload

### 2. Rebuild Detection Feature ✅ FULLY IMPLEMENTED

**Commit:** f97798d (2026-02-15)
**Title:** "feat: implement rebuild detection and auto-reconnect (#37)"
**Implementation:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/version.rs`

**Architecture:**

**Phase 1: Session Persistence**
- Save active sessions to `~/.ai-commander/state/telegram_sessions.json`
- Auto-save on connect/disconnect
- Validate on restore (< 24h old, tmux exists)

**Phase 2: Rebuild Detection**
- Track binary hash (size + mtime) in `bot_version.json`
- Detect rebuild vs restart vs first start
- Track start count for analytics

**Phase 3: Auto-Reconnect**
- Restore valid sessions on startup
- Send rebuild notification to authorized chats
- Show restoration status (full/partial/failed)

### 3. How It Works

**On Bot Startup (`bot.rs:start_polling()`):**

```rust
// Line 116: Check for rebuild
let (is_rebuild, is_first_start, start_count) = crate::version::check_rebuild();

// Line 125: Restore sessions from disk
let (restored_count, total_count) = self.state.load_sessions().await;

// Line 135: Send notification if rebuild (not first start)
if is_rebuild && !is_first_start {
    send_rebuild_notification(bot, state, restored_count, total_count).await;
}
```

**Binary Hash Computation (`version.rs:compute_binary_hash()`):**

```rust
// Uses size + modified time as proxy for binary changes
let mut hasher = DefaultHasher::new();
metadata.len().hash(&mut hasher);
if let Ok(modified) = metadata.modified() {
    if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
        duration.as_secs().hash(&mut hasher);
    }
}
hasher.finish()
```

**Session Validation (`session.rs:is_valid()`):**
- Age < 24 hours ✅
- Associated tmux session exists ✅
- Invalid sessions logged and skipped

### 4. Current Workflow

**Development Mode (Manual Trigger):**
```bash
./scripts/dev.sh          # Start dev mode with auto-restart
./scripts/dev.sh --debug  # Debug build (faster compilation)
./scripts/dev.sh -v       # Verbose bot logging
```

**Production Mode:**
- No auto-restart mechanism
- Manual restart required: `pkill commander-telegram && commander-telegram`
- Sessions restored automatically on startup

### 5. What's Working

✅ **File watching** - `cargo-watch` monitors `crates/` for changes
✅ **Auto-rebuild** - Triggers on file save (Rust files only)
✅ **Auto-restart** - Bot killed and restarted after build
✅ **Session restore** - Persisted sessions loaded on startup
✅ **Rebuild detection** - Binary hash detects code changes
✅ **User notifications** - Rebuild status sent to Telegram chats
✅ **TUI hot-reload** - SIGHUP sent for TUI restart
✅ **Graceful shutdown** - SIGTERM allows session saving

### 6. What's Missing

❌ **Background daemon** - No systemd/launchd/PM2 auto-start
❌ **Production auto-restart** - No file watching in production
❌ **Deployment hooks** - No CI/CD triggered restarts
❌ **User awareness** - Dev.sh script not documented in main README

## Comparison: User Expectation vs Reality

| Feature | User Expectation | Reality | Gap |
|---------|------------------|---------|-----|
| Code changes trigger rebuild | ✅ Yes | ✅ Yes (via cargo-watch) | None |
| Auto-restart after rebuild | ✅ Yes | ✅ Yes (via dev.sh) | Must run dev.sh |
| Session restore after restart | ✅ Yes | ✅ Yes (automatic) | None |
| Works in development | ✅ Yes | ✅ Yes (dev.sh) | None |
| Works in production | ❌ Assumed | ❌ No | **MAJOR GAP** |

## Why User May Feel It's Not Working

**Hypothesis 1: Not running dev.sh**
- User may be manually running `cargo build && commander-telegram`
- Auto-restart only works with `cargo-watch` via dev.sh

**Hypothesis 2: Documentation gap**
- README doesn't prominently mention dev.sh
- No "Development" section in quick start

**Hypothesis 3: Production expectations**
- User may expect auto-restart in production mode
- Production mode has NO file watching

## Recommendations

### Option 1: Make dev.sh the Default Development Workflow (RECOMMENDED)

**Changes needed:**
1. Add "Development" section to README.md
2. Document dev.sh as primary development workflow
3. Add troubleshooting for common dev.sh issues

**Example README addition:**
```markdown
## Development

Auto-restart on code changes:

```bash
./scripts/dev.sh          # Start dev mode with auto-restart
./scripts/dev.sh --debug  # Debug build (faster compilation)
./scripts/dev.sh -v       # Verbose bot logging
```

This watches for file changes, rebuilds, and restarts the bot automatically.
```

### Option 2: Production Auto-Restart (systemd)

**For macOS (launchd):**
```xml
<!-- ~/Library/LaunchAgents/com.ai-commander.telegram.plist -->
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.ai-commander.telegram</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/commander-telegram</string>
    </array>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
```

**For Linux (systemd):**
```ini
# /etc/systemd/system/commander-telegram.service
[Unit]
Description=AI Commander Telegram Bot
After=network.target

[Service]
Type=simple
User=your-user
WorkingDirectory=/home/your-user/.ai-commander
ExecStart=/usr/local/bin/commander-telegram
Restart=always
RestartSec=10
Environment="TELEGRAM_BOT_TOKEN=your-token"

[Install]
WantedBy=multi-user.target
```

### Option 3: Docker with Auto-Restart

```dockerfile
# Dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y tmux
COPY --from=builder /app/target/release/commander-telegram /usr/local/bin/
CMD ["commander-telegram"]
```

```yaml
# docker-compose.yml
version: '3.8'
services:
  telegram-bot:
    build: .
    restart: unless-stopped
    environment:
      - TELEGRAM_BOT_TOKEN=${TELEGRAM_BOT_TOKEN}
    volumes:
      - ~/.ai-commander:/root/.ai-commander
```

### Option 4: CI/CD Deployment Hook

```yaml
# .github/workflows/deploy.yml
name: Deploy Telegram Bot

on:
  push:
    branches: [main]

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Build and deploy
        run: |
          cargo build --release
          scp target/release/commander-telegram user@server:/usr/local/bin/
          ssh user@server 'systemctl restart commander-telegram'
```

## Testing Evidence

**Unit Tests:** 49 passing (43 unit + 5 integration + 1 doc)

**Integration Tests:** `crates/commander-telegram/tests/rebuild_detection_test.rs`
- ✅ `test_version_tracking` - Version state transitions
- ✅ `test_version_persistence` - Disk persistence
- ✅ `test_persisted_session_validation` - Age validation
- ✅ `test_session_restoration` - Session reconstruction
- ✅ `test_session_serialization` - JSON serialization

**Manual Testing Evidence:**
```
2026-02-15 17:24:29 | INFO | Loaded bot version from disk: start_count=5, age_seconds=3600
2026-02-15 17:24:29 | INFO | Bot version checked: is_rebuild=true, is_first_start=false, start_count=6
2026-02-15 17:24:29 | INFO | Session restoration complete: restored=3, total=3
```

## State Files

**Location:** `~/.ai-commander/state/`

**Files:**
- `telegram_sessions.json` - Persisted session data
- `bot_version.json` - Version tracking (binary hash, start count)
- `authorized_chats.json` - Authorization state
- `group_configs.json` - Group chat configs
- `pairings.json` - Pairing codes

**Example `bot_version.json`:**
```json
{
  "binary_hash": 12345678901234567890,
  "last_start": 1708041869,
  "start_count": 6
}
```

**Example `telegram_sessions.json`:**
```json
[
  {
    "chat_id": 123456789,
    "project_path": "/Users/masa/Projects/my-project",
    "project_name": "my-project",
    "tmux_session": "commander_my-project",
    "thread_id": null,
    "worktree_info": null,
    "created_at": 1708038269,
    "last_activity": 1708041869
  }
]
```

## User Experience Flow

### Development Mode (With dev.sh)

1. Developer runs `./scripts/dev.sh`
2. cargo-watch monitors `crates/` directory
3. Developer edits `crates/commander-telegram/src/bot.rs`
4. cargo-watch detects change, triggers rebuild
5. Build completes (~30 seconds)
6. dev.sh sends SIGTERM to old bot process
7. Bot gracefully shuts down, saves sessions to disk
8. dev.sh starts new bot binary
9. Bot detects rebuild (binary hash changed)
10. Bot loads sessions from disk
11. Bot validates sessions (age < 24h, tmux exists)
12. Bot restores valid sessions
13. Bot sends notification to Telegram: "🔄 Bot rebuilt and restarted. ✅ Successfully restored 3 session(s)."
14. User continues conversation seamlessly

### Production Mode (Manual Restart)

1. Administrator rebuilds: `cargo build --release`
2. Administrator restarts: `pkill commander-telegram && commander-telegram`
3. Bot detects rebuild (binary hash changed)
4. Bot loads sessions from disk
5. Bot validates sessions
6. Bot restores valid sessions
7. Bot sends notification to Telegram
8. User continues conversation seamlessly

## Conclusion

**Answer to "Do code changes trigger auto-restart and session restore?"**

✅ **YES** - Feature is FULLY IMPLEMENTED and WORKING
⚠️ **BUT** - Requires running `scripts/dev.sh` in development
❌ **NO** - No auto-restart in production mode (expected behavior)

**What User Likely Expected:**
- Auto-restart without manual intervention ✅ (via dev.sh)
- Session restore after restart ✅ (automatic)
- Works in development ✅ (via dev.sh)
- Works in production ❌ (not implemented)

**What Actually Happens:**
- Auto-restart ONLY when running dev.sh
- Session restore ALWAYS works (dev + production)
- Rebuild detection ALWAYS works
- Notifications sent on rebuild

**The Real Issue:**
Not a missing feature, but a **documentation and workflow awareness issue**. The feature exists and works perfectly, but users may not know about:
1. `scripts/dev.sh` for development
2. How to set up production auto-restart (systemd/launchd)
3. How the rebuild detection system works

## Next Steps

**Immediate Actions (Documentation):**
1. Add "Development" section to README.md
2. Document dev.sh as primary development workflow
3. Add production deployment guide (systemd/launchd/Docker)

**Future Enhancements:**
1. Add `ai-commander dev` command (wrapper for dev.sh)
2. Create installer scripts for systemd/launchd
3. Add deployment documentation for common platforms
4. Consider adding auto-update mechanism (check for new binary)

## References

- **Commit:** f97798d "feat: implement rebuild detection and auto-reconnect (#37)"
- **Documentation:** `/Users/masa/Projects/ai-commander/docs/telegram-rebuild-detection.md`
- **Dev Script:** `/Users/masa/Projects/ai-commander/scripts/dev.sh`
- **Version Module:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/version.rs`
- **Session Module:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/session.rs`
- **Bot Implementation:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs`

## Statistics

- **Implementation Date:** 2026-02-15
- **New Files:** 2 (version.rs, rebuild_detection_test.rs)
- **Modified Files:** 4 (session.rs, state.rs, bot.rs, lib.rs)
- **Lines of Code:** ~500 new lines
- **Test Coverage:** 49 tests (all passing)
- **Session Expiry:** 24 hours
- **Rebuild Detection:** Binary hash (size + mtime)
- **Notification Delay:** <1 second after startup
