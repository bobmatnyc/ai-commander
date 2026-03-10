# Bug #2: Ready-to-Apply Fixes

**Date:** 2026-02-23
**Issue:** Connect failing due to poor Tmux error handling

This document contains **copy-paste ready code fixes** for immediate application.

---

## Fix 1: Improve Backend Error Messages (HIGH PRIORITY)

**File:** `crates/commander-gui/src/commands.rs`

**Location:** Line 40-49 (connect_session function)

### Current Code
```rust
#[tauri::command]
pub async fn connect_session(name: String, state: State<'_, GuiState>) -> Result<(), String> {
    let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;

    if !tmux.session_exists(&name) {
        return Err(format!("Session '{}' not found", name));
    }

    *state.current_session.write().unwrap() = Some(name.clone());
    Ok(())
}
```

### Replace With
```rust
#[tauri::command]
pub async fn connect_session(name: String, state: State<'_, GuiState>) -> Result<(), String> {
    let tmux = state.tmux.as_ref().ok_or(
        "Tmux is not available. Please ensure tmux is installed and running.\n\n\
         To start tmux: run 'tmux' in terminal\n\
         To install tmux:\n\
           - macOS: brew install tmux\n\
           - Ubuntu/Debian: sudo apt-get install tmux\n\
           - Fedora: sudo dnf install tmux"
    )?;

    if !tmux.session_exists(&name) {
        return Err(format!(
            "Session '{}' not found. It may have been stopped or deleted.\n\n\
             Use the session list to see available sessions.",
            name
        ));
    }

    *state.current_session.write().unwrap() = Some(name.clone());
    Ok(())
}
```

**Why this helps:**
- Users see actionable error messages
- Clear installation instructions
- Explains what went wrong and how to fix it

---

## Fix 2: Add Error Notifications (HIGH PRIORITY)

**File:** `crates/commander-gui/ui/src/lib/components/SessionList.svelte`

**Location:** Around line 25-50 (connect function)

### Current Code
```typescript
async function connect(name: string) {
  try {
    await invoke('connect_session', { name });
    const session = $sessions.find(s => s.name === name);
    if (session) {
      currentSession.set({ ...session, is_connected: true });

      // Add initial connection message only if this session has no messages yet
      const existingMessages = $sessionMessages.get(name);
      if (!existingMessages || existingMessages.length === 0) {
        addMessageToSession(name, {
          direction: 'system',
          content: `Connected to session: ${getDisplayName(name)}`,
          timestamp: new Date(),
        });
      }
    }
  } catch (error) {
    console.error('Failed to connect to session:', error);
    // TODO: Show error toast
  }
}
```

### Replace With

**Option A: Using Toast Notification (if toast library available)**
```typescript
async function connect(name: string) {
  try {
    await invoke('connect_session', { name });
    const session = $sessions.find(s => s.name === name);
    if (session) {
      currentSession.set({ ...session, is_connected: true });

      // Add initial connection message only if this session has no messages yet
      const existingMessages = $sessionMessages.get(name);
      if (!existingMessages || existingMessages.length === 0) {
        addMessageToSession(name, {
          direction: 'system',
          content: `Connected to session: ${getDisplayName(name)}`,
          timestamp: new Date(),
        });
      }
    }
  } catch (error) {
    console.error('Failed to connect to session:', error);

    // Show error notification
    const errorMessage = error instanceof Error ? error.message : String(error);
    toast.error('Connection Failed', {
      description: errorMessage,
      duration: 5000,
    });
  }
}
```

**Option B: Using Alert Dialog (simpler, no dependencies)**
```typescript
async function connect(name: string) {
  try {
    await invoke('connect_session', { name });
    const session = $sessions.find(s => s.name === name);
    if (session) {
      currentSession.set({ ...session, is_connected: true });

      // Add initial connection message only if this session has no messages yet
      const existingMessages = $sessionMessages.get(name);
      if (!existingMessages || existingMessages.length === 0) {
        addMessageToSession(name, {
          direction: 'system',
          content: `Connected to session: ${getDisplayName(name)}`,
          timestamp: new Date(),
        });
      }
    }
  } catch (error) {
    console.error('Failed to connect to session:', error);

    // Show error dialog
    const errorMessage = error instanceof Error ? error.message : String(error);
    alert(`Connection Failed\n\n${errorMessage}`);
  }
}
```

**Option C: Using Svelte Store for Notification State (recommended)**
```typescript
// At top of file, add:
import { writable } from 'svelte/store';

// Create error store (if not exists)
export const errorNotification = writable<{message: string, title: string} | null>(null);

// In connect function:
async function connect(name: string) {
  try {
    await invoke('connect_session', { name });
    const session = $sessions.find(s => s.name === name);
    if (session) {
      currentSession.set({ ...session, is_connected: true });

      // Add initial connection message only if this session has no messages yet
      const existingMessages = $sessionMessages.get(name);
      if (!existingMessages || existingMessages.length === 0) {
        addMessageToSession(name, {
          direction: 'system',
          content: `Connected to session: ${getDisplayName(name)}`,
          timestamp: new Date(),
        });
      }
    }
  } catch (error) {
    console.error('Failed to connect to session:', error);

    // Set error notification
    const errorMessage = error instanceof Error ? error.message : String(error);
    errorNotification.set({
      title: 'Connection Failed',
      message: errorMessage
    });

    // Clear after 5 seconds
    setTimeout(() => errorNotification.set(null), 5000);
  }
}
```

Then add error notification component to template:
```svelte
{#if $errorNotification}
  <div class="error-notification">
    <h3>{$errorNotification.title}</h3>
    <p>{$errorNotification.message}</p>
    <button on:click={() => errorNotification.set(null)}>×</button>
  </div>
{/if}

<style>
  .error-notification {
    position: fixed;
    top: 20px;
    right: 20px;
    background: #fee;
    border: 2px solid #fcc;
    border-radius: 8px;
    padding: 16px;
    max-width: 400px;
    box-shadow: 0 4px 6px rgba(0,0,0,0.1);
    z-index: 1000;
    animation: slideIn 0.3s ease-out;
  }

  .error-notification h3 {
    margin: 0 0 8px 0;
    color: #c00;
    font-size: 16px;
    font-weight: 600;
  }

  .error-notification p {
    margin: 0;
    color: #600;
    font-size: 14px;
    white-space: pre-wrap;
  }

  .error-notification button {
    position: absolute;
    top: 8px;
    right: 8px;
    background: none;
    border: none;
    font-size: 24px;
    cursor: pointer;
    color: #c00;
    line-height: 1;
  }

  @keyframes slideIn {
    from {
      transform: translateX(100%);
      opacity: 0;
    }
    to {
      transform: translateX(0);
      opacity: 1;
    }
  }
</style>
```

**Choose option based on your project:**
- **Option A**: If you have a toast library (e.g., svelte-sonner, svelte-french-toast)
- **Option B**: Quick fix, no dependencies, but not the best UX
- **Option C**: Best UX, native Svelte, no dependencies, but requires template changes

---

## Fix 3: Add Tmux Health Check (MEDIUM PRIORITY)

**File:** `crates/commander-gui/src/commands.rs`

**Location:** After `disconnect_session` function (around line 55)

### Add New Command
```rust
#[tauri::command]
pub async fn check_tmux_status(state: State<'_, GuiState>) -> Result<bool, String> {
    Ok(state.tmux.is_some())
}
```

### Frontend Usage

**File:** `crates/commander-gui/ui/src/App.svelte` or main component

**Add on mount:**
```typescript
import { onMount } from 'svelte';
import { invoke } from '@tauri-apps/api/tauri';

let tmuxAvailable = true;

onMount(async () => {
  try {
    tmuxAvailable = await invoke('check_tmux_status');

    if (!tmuxAvailable) {
      // Show warning banner
      console.warn('Tmux is not available');
      // You can set a store or show UI element here
    }
  } catch (error) {
    console.error('Failed to check tmux status:', error);
  }
});
```

**Add warning banner to template:**
```svelte
{#if !tmuxAvailable}
  <div class="warning-banner">
    ⚠️ Tmux is not available. Session connections will fail.
    <a href="#" on:click|preventDefault={showTmuxHelp}>How to fix</a>
  </div>
{/if}

<style>
  .warning-banner {
    background: #ffc;
    border: 2px solid #fc0;
    padding: 12px;
    text-align: center;
    color: #630;
    font-weight: 500;
  }

  .warning-banner a {
    color: #00c;
    text-decoration: underline;
    margin-left: 8px;
  }
</style>
```

---

## Fix 4: Tmux Auto-Recovery (LOW PRIORITY)

**File:** `crates/commander-gui/src/state.rs`

**Location:** `GuiState::new()` function (around line 21-36)

### Current Code
```rust
impl GuiState {
    pub fn new() -> Result<Self> {
        // Use commander_core::config to get the state directory
        let state_dir = commander_core::config::state_dir();
        let store = StateStore::new(state_dir);
        let tmux = TmuxOrchestrator::new().ok();

        Ok(Self {
            store: Arc::new(store),
            tmux: tmux.map(Arc::new),
            current_session: Arc::new(RwLock::new(None)),
            bot_status: Arc::new(RwLock::new(DaemonStatus {
                running: false,
                pid: None,
            })),
        })
    }
}
```

### Replace With
```rust
impl GuiState {
    pub fn new() -> Result<Self> {
        // Use commander_core::config to get the state directory
        let state_dir = commander_core::config::state_dir();
        let store = StateStore::new(state_dir);

        // Try to initialize Tmux
        let mut tmux = TmuxOrchestrator::new().ok();

        // If Tmux initialization failed, try starting tmux server
        if tmux.is_none() {
            tracing::info!("Tmux not available, attempting to start server");

            // Try to start tmux server
            match std::process::Command::new("tmux")
                .arg("start-server")
                .status()
            {
                Ok(status) if status.success() => {
                    tracing::info!("Started tmux server successfully");
                    // Retry initialization
                    tmux = TmuxOrchestrator::new().ok();

                    if tmux.is_some() {
                        tracing::info!("Tmux initialized successfully after server start");
                    } else {
                        tracing::warn!("Tmux server started but initialization still failed");
                    }
                }
                Ok(status) => {
                    tracing::warn!("Failed to start tmux server: exit code {}", status);
                }
                Err(e) => {
                    tracing::warn!("Failed to execute tmux command: {}", e);
                }
            }
        }

        Ok(Self {
            store: Arc::new(store),
            tmux: tmux.map(Arc::new),
            current_session: Arc::new(RwLock::new(None)),
            bot_status: Arc::new(RwLock::new(DaemonStatus {
                running: false,
                pid: None,
            })),
        })
    }
}
```

**Note:** This requires `tracing` to be imported at the top of the file:
```rust
use tracing::{info, warn};
```

---

## Testing After Applying Fixes

### Test 1: Verify Error Message Improvement
```bash
# Stop tmux
killall tmux

# Start GUI
cd crates/commander-gui && cargo run

# Try to connect to any session
# Expected: Should see helpful error message with install instructions
```

### Test 2: Verify Error Notification
```bash
# Stop tmux
killall tmux

# Start GUI
# Click "Connect" button
# Expected: Should see visible error notification (not just console)
```

### Test 3: Verify Health Check
```bash
# Stop tmux
killall tmux

# Start GUI
# Expected: Should see warning banner about tmux unavailability
```

### Test 4: Verify Auto-Recovery
```bash
# Stop tmux
killall tmux

# Start GUI
# Expected: GUI should attempt to start tmux server automatically
# Check logs for: "Started tmux server successfully"
```

---

## Priority of Fixes

| Fix | Priority | Impact | Effort | Apply Order |
|-----|----------|--------|--------|-------------|
| Fix 1: Error Messages | HIGH | High | 5 min | 1st |
| Fix 2: Error Notifications | HIGH | High | 10-15 min | 2nd |
| Fix 3: Health Check | MEDIUM | Medium | 10 min | 3rd |
| Fix 4: Auto-Recovery | LOW | Low | 15 min | 4th |

**Recommended:** Apply Fix 1 and Fix 2 immediately (critical for UX). Apply Fix 3 and 4 when time permits.

---

## Build and Register Commands

### After adding `check_tmux_status` command

**File:** `crates/commander-gui/src/main.rs`

**Find the `invoke_handler` section and add:**
```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    commands::check_tmux_status,  // ← Add this line
])
```

### Rebuild
```bash
cd crates/commander-gui
cargo build
```

---

## Verification Checklist

After applying fixes:

- [ ] Backend: Error messages improved in `commands.rs`
- [ ] Frontend: Error notification shows in UI
- [ ] Backend: `check_tmux_status` command added and registered
- [ ] Frontend: Health check runs on startup
- [ ] Frontend: Warning banner shows when tmux unavailable
- [ ] Backend: Auto-recovery attempts tmux server start
- [ ] Test: Verify with `killall tmux` scenario
- [ ] Test: Error notification visible in GUI (not console only)
- [ ] Test: Warning banner appears on startup without tmux
- [ ] Test: Auto-recovery logs show tmux start attempt

---

**Estimated Total Time:** 30-45 minutes for all fixes

**Critical Fixes Only (Fix 1 + Fix 2):** 15-20 minutes
