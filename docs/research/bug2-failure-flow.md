# Bug #2: Connect Failure - Flow Diagram

## Current Failure Flow (Broken)

```
┌──────────────────────────────────────────────────────────────┐
│ 1. Application Startup                                        │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│ GuiState::new()                                               │
│   let tmux = TmuxOrchestrator::new().ok()                    │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │ Tmux running?   │
                    └─────────────────┘
                         │        │
                      YES│        │NO
                         │        │
         ┌───────────────┘        └──────────────┐
         │                                        │
         ▼                                        ▼
┌─────────────────────┐              ┌─────────────────────┐
│ state.tmux = Some(.)│              │ state.tmux = None   │
└─────────────────────┘              └─────────────────────┘
         │                                        │
         │                                        │
         └────────────────┬───────────────────────┘
                          │
                          ▼
         ┌────────────────────────────────────────┐
         │ GUI Running (tmux may be None!)        │
         └────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ 2. User Action: Click "Connect to ai-commander"              │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ Frontend: SessionList.svelte                                  │
│   await invoke('connect_session', { name: 'ai-commander' })  │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ Backend: commands.rs::connect_session()                       │
│   let tmux = state.tmux.as_ref().ok_or("Tmux not init")?    │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
                    ┌─────────────────┐
                    │ state.tmux?     │
                    └─────────────────┘
                         │        │
                      Some│       │None
                         │        │
         ┌───────────────┘        └──────────────┐
         │                                        │
         ▼                                        ▼
┌─────────────────────┐              ┌─────────────────────────┐
│ Check session exists│              │ Return Err("Tmux not    │
└─────────────────────┘              │ initialized")           │
         │                            └─────────────────────────┘
         │                                        │
         ▼                                        │
    ┌────────┐                                    │
    │Exists? │                                    │
    └────────┘                                    │
      │    │                                      │
    Yes│   │No                                    │
      │    │                                      │
      │    ▼                                      │
      │  ┌──────────────────────┐                │
      │  │ Return Err("Session  │                │
      │  │ 'name' not found")   │                │
      │  └──────────────────────┘                │
      │             │                             │
      │             └─────────────────┬───────────┘
      ▼                               │
┌─────────────────────┐               │
│ Set current_session │               │
│ Return Ok(())       │               │
└─────────────────────┘               │
         │                             │
         │                             │
         └────────────────┬────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ Frontend: catch (error)                                       │
│   console.error('Failed to connect:', error)                 │
│   // ⚠️  NO USER NOTIFICATION!                               │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ Result: User sees nothing                                     │
│   - No error dialog                                           │
│   - No toast notification                                     │
│   - Error only in browser console (hidden)                    │
└──────────────────────────────────────────────────────────────┘
```

---

## Fixed Flow (After Improvements)

```
┌──────────────────────────────────────────────────────────────┐
│ 1. Application Startup                                        │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│ GuiState::new()                                               │
│   let tmux = TmuxOrchestrator::new().ok()                    │
│                                                               │
│   if tmux.is_none() {                                        │
│     // Try starting tmux server                              │
│     Command::new("tmux").arg("start-server").status();       │
│     tmux = TmuxOrchestrator::new().ok();                     │
│   }                                                           │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │ Tmux available? │
                    └─────────────────┘
                         │        │
                      YES│        │NO
                         │        │
         ┌───────────────┘        └──────────────┐
         │                                        │
         ▼                                        ▼
┌─────────────────────┐              ┌─────────────────────────┐
│ state.tmux = Some(.)│              │ state.tmux = None       │
│                     │              │                         │
│ ✅ Ready to connect │              │ ⚠️  Show startup warning│
└─────────────────────┘              └─────────────────────────┘
         │                                        │
         │                                        ▼
         │                            ┌─────────────────────────┐
         │                            │ Frontend: Show banner   │
         │                            │ "Tmux unavailable.      │
         │                            │  Please install/start"  │
         │                            └─────────────────────────┘
         │                                        │
         └────────────────┬───────────────────────┘
                          │
                          ▼
         ┌────────────────────────────────────────┐
         │ GUI Running                            │
         └────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ 2. User Action: Click "Connect to ai-commander"              │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ Frontend: SessionList.svelte                                  │
│   await invoke('connect_session', { name: 'ai-commander' })  │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ Backend: commands.rs::connect_session()                       │
│   let tmux = state.tmux.as_ref().ok_or(                      │
│     "Tmux is not available. Please ensure tmux is            │
│      installed and running."                                  │
│   )?                                                          │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
                    ┌─────────────────┐
                    │ state.tmux?     │
                    └─────────────────┘
                         │        │
                      Some│       │None
                         │        │
         ┌───────────────┘        └──────────────┐
         │                                        │
         ▼                                        ▼
┌─────────────────────┐              ┌─────────────────────────┐
│ Check session exists│              │ Return Err("Tmux is not │
└─────────────────────┘              │ available. Please...")   │
         │                            └─────────────────────────┘
         │                                        │
         ▼                                        │
    ┌────────┐                                    │
    │Exists? │                                    │
    └────────┘                                    │
      │    │                                      │
    Yes│   │No                                    │
      │    │                                      │
      │    ▼                                      │
      │  ┌──────────────────────┐                │
      │  │ Return Err("Session  │                │
      │  │ 'name' not found. It │                │
      │  │ may have stopped.")  │                │
      │  └──────────────────────┘                │
      │             │                             │
      │             └─────────────────┬───────────┘
      ▼                               │
┌─────────────────────┐               │
│ Set current_session │               │
│ Return Ok(())       │               │
└─────────────────────┘               │
         │                             │
         │                             │
         └────────────────┬────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ Frontend: catch (error)                                       │
│   console.error('Failed to connect:', error)                 │
│                                                               │
│   // ✅ SHOW USER NOTIFICATION                               │
│   notifications.error({                                       │
│     title: 'Connection Failed',                              │
│     message: error.toString(),                               │
│     timeout: 5000                                            │
│   })                                                          │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│ Result: User sees clear error                                 │
│   ✅ Error toast/notification visible                         │
│   ✅ Helpful error message                                    │
│   ✅ User understands what went wrong                         │
│   ✅ User knows how to fix (install/start tmux)              │
└──────────────────────────────────────────────────────────────┘
```

---

## Key Differences

| Aspect | Current (Broken) | Fixed |
|--------|-----------------|-------|
| **Tmux startup** | Silent failure → None | Try auto-start, show warning if still None |
| **Error messages** | "Tmux not initialized" | "Tmux is not available. Please ensure..." |
| **User feedback** | Console only (hidden) | Toast notification (visible) |
| **Health check** | None | Check on startup, show banner |
| **Recovery** | Manual only | Auto-recovery attempted |

---

## Error Message Comparison

### Current (Cryptic)
```
Error: Tmux not initialized
```

### Improved (Helpful)
```
Connection Failed

Tmux is not available. Please ensure tmux is installed and running.

To start tmux: run 'tmux' in terminal
To install tmux: brew install tmux (macOS) or apt-get install tmux (Linux)
```

---

## Testing Scenarios

### Scenario 1: Tmux Not Running
```bash
# Before fix:
killall tmux
# GUI starts → User clicks connect → Silent failure in console

# After fix:
killall tmux
# GUI starts → Banner: "Tmux unavailable"
# User clicks connect → Visible error notification
```

### Scenario 2: Session Doesn't Exist
```bash
# Before fix:
# User clicks connect to "deleted-session"
# → Error in console only

# After fix:
# User clicks connect to "deleted-session"
# → Toast: "Session 'deleted-session' not found. It may have been stopped."
```

### Scenario 3: Tmux Auto-Recovery
```bash
# After fix only:
killall tmux
# GUI starts → Attempts "tmux start-server"
# → If successful: state.tmux = Some(...), no warning
# → If failed: state.tmux = None, show warning banner
```

---

## Implementation Checklist

- [ ] Backend: Improve error message in `connect_session()`
- [ ] Backend: Add Tmux auto-recovery in `GuiState::new()`
- [ ] Backend: Add `check_tmux_status()` command
- [ ] Frontend: Add error notification in `SessionList.svelte`
- [ ] Frontend: Add health check on startup
- [ ] Frontend: Show warning banner if Tmux unavailable
- [ ] Test: Verify with `killall tmux` scenario
- [ ] Test: Verify error visibility in GUI
- [ ] Docs: Update troubleshooting guide

---

**Related Files:**
- `crates/commander-gui/src/state.rs` (Tmux initialization)
- `crates/commander-gui/src/commands.rs` (connect_session)
- `crates/commander-gui/ui/src/lib/components/SessionList.svelte` (error handling)
