# AI Commander Startup Bug Investigation

**Date**: 2026-02-23
**Investigator**: Claude Sonnet 4.5
**Context**: User reported "there was a bug in startup" without specific details

## Investigation Summary

Conducted systematic investigation across GUI, bot daemon, and recent code changes to identify potential startup issues.

## Findings

### 1. **Compilation Status: HEALTHY** ✅

All packages compile successfully:

```bash
# GUI compilation
cargo check -p commander-gui
✓ Finished in 41.55s
⚠️ Warning: field `store` is never read in GuiState (line 8)

# Telegram bot compilation
cargo check -p commander-telegram
✓ Finished in 26.14s
```

**Analysis**: No compilation errors. Warning is benign (unused field in struct).

### 2. **Frontend Build Status: HEALTHY** ✅

```bash
cd crates/commander-gui/ui && npm run build
✓ Built in 3.21s
⚠️ A11y warnings: Form label association, keyboard event listeners
```

**Analysis**: Vite build succeeds. Warnings are accessibility issues, not errors.

### 3. **Runtime Status: RUNNING** ✅

```bash
ps aux | grep commander-gui
✓ /Users/masa/Projects/ai-commander/target/debug/commander-gui (PID 2270)
✓ Vite dev server running (PID 84152)
✓ esbuild service running (PID 84153)
```

**Analysis**: GUI is currently running without crashes.

### 4. **Potential Issue Identified: Silent tmux Initialization Failure** ⚠️

**Location**: `crates/commander-gui/src/state.rs:25`

```rust
pub fn new() -> Result<Self> {
    let state_dir = commander_core::config::state_dir();
    let store = StateStore::new(state_dir);
    let tmux = TmuxOrchestrator::new().ok();  // ⚠️ Silently converts Err to None

    Ok(Self {
        store: Arc::new(store),
        tmux: tmux.map(Arc::new),  // tmux can be None
        current_session: Arc::new(RwLock::new(None)),
        bot_status: Arc::new(RwLock::new(DaemonStatus {
            running: false,
            pid: None,
        })),
    })
}
```

**Root Cause Analysis**:

1. **Silent Failure**: `TmuxOrchestrator::new().ok()` converts any `Err` to `None`
2. **No Error Visibility**: GUI starts successfully even if tmux initialization fails
3. **Delayed Impact**: All session-related commands fail with "Tmux not initialized" error

**Affected Commands** (all in `commands.rs`):
- `list_sessions` (line 21)
- `connect_session` (line 41)
- `stop_session` (line 59)
- `send_message` (line 87)

Each command checks for tmux:
```rust
let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;
```

**Why tmux could fail**:
- tmux not in PATH (unlikely, verified: `/opt/homebrew/bin/tmux 3.6a`)
- Permission issues running `which tmux`
- Race condition during startup
- Environment variable issues in GUI context

### 5. **Recent Code Changes: NO SMOKING GUN** ✅

Examined commits:
- `8266974` (option selection) - Moved option detection to `commander-core`, no GUI startup changes
- `811c76c` (hash detection) - Added hash-based change detection in polling loop, no startup impact
- `843d349` (session history) - Added slash command interpreter
- `474ee83` (session switching) - Fixed session switching

**Analysis**: Recent changes are in feature logic, not startup path.

### 6. **Background Task Outputs: EMPTY** ℹ️

```bash
ls /private/tmp/claude-501/-Users-masa-Projects-ai-commander/tasks/
✓ Directory exists
✓ Recent files present (hook_1554.output, etc.)
✓ Content: {"async": true, "asyncTimeout": 60000}
```

**Analysis**: No error logs in background tasks.

### 7. **Unrelated Error in Claude Logs** ❌

Found in `/Users/masa/Library/Logs/Claude/main.log`:
```
2026-02-23 19:52:37 [error] [detectedProjects] source failed:
Error: Failed to spawn /usr/bin/sqlite3 (via disclaimer):
/Applications/Claude.app/Contents/Helpers/disclaimer exited with code 14:
Error: in prepare, unable to open database file (14)
```

**Analysis**: This is a Claude Desktop database error, **NOT** related to AI Commander GUI startup.

## Reproduction Steps (Hypothetical)

If tmux initialization is the issue, user would experience:

1. Launch `cargo tauri dev` or run `commander-gui`
2. GUI opens successfully (no error dialog)
3. Navigate to Sessions page
4. Click "Refresh" or attempt to list sessions
5. **Error**: "Tmux not initialized" appears in UI
6. Cannot connect to any sessions
7. Cannot send messages to Claude

## Recommendations

### Immediate Actions

1. **Verify tmux availability in GUI context**:
   ```bash
   # Test if GUI can find tmux
   /Users/masa/Projects/ai-commander/target/debug/commander-gui &
   # Check logs for "tmux found" debug message
   ```

2. **Add startup error reporting**:
   ```rust
   // In state.rs:25, replace:
   let tmux = TmuxOrchestrator::new().ok();

   // With:
   let tmux = match TmuxOrchestrator::new() {
       Ok(t) => {
           eprintln!("[GUI] Tmux initialized successfully");
           Some(t)
       }
       Err(e) => {
           eprintln!("[GUI] WARNING: Tmux initialization failed: {}", e);
           None
       }
   };
   ```

3. **Test session functionality**:
   - Launch GUI
   - Try to list sessions
   - Check console for "Tmux not initialized" errors

### Long-Term Fixes

1. **Make tmux optional with clear messaging**:
   - Add UI indicator: "Tmux unavailable - session features disabled"
   - Show installation instructions if tmux missing

2. **Improve error visibility**:
   - Log tmux initialization status to file
   - Surface errors in GUI status bar

3. **Add health check endpoint**:
   ```rust
   #[tauri::command]
   pub async fn health_check(state: State<'_, GuiState>) -> Result<HealthStatus, String> {
       Ok(HealthStatus {
           tmux_available: state.tmux.is_some(),
           store_initialized: true,
           // ... other checks
       })
   }
   ```

## User Questions to Ask

To narrow down the issue:

1. **Did you see a specific error message?** (e.g., "Tmux not initialized", crash dialog, etc.)
2. **When did the bug occur?** (immediately on startup, or after attempting to use a feature?)
3. **Were you able to use the GUI at all?** (or did it fail to open?)
4. **Which features did you try to use?** (sessions list, bot controls, etc.)
5. **Check terminal output**: `cd crates/commander-gui && cargo tauri dev` - any errors printed?

## Conclusion

**Primary Hypothesis**: Silent tmux initialization failure during GUI startup, causing session-related features to fail with "Tmux not initialized" errors.

**Confidence**: Medium (plausible but not confirmed)

**Evidence**:
- ✅ Code pattern allows silent failure (`.ok()` conversion)
- ✅ All session commands check for tmux presence
- ❌ No direct evidence of tmux failure in logs
- ❌ No user-reported error message

**Next Steps**:
1. Confirm issue with user (get specific error message)
2. Add debug logging to tmux initialization
3. Test in GUI context vs. CLI context
4. Implement health check UI

---

**Attachments**:
- All code compiles successfully
- No runtime crashes detected
- GUI currently running (PID 2270)
- tmux verified installed: `/opt/homebrew/bin/tmux 3.6a`
