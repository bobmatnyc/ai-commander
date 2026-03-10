# Bug Investigation Summary

**Date:** 2026-02-23
**Quick Reference for: Critical Bugs in AI Commander**

---

## TL;DR

| Bug | Status | Severity | Action Required |
|-----|--------|----------|-----------------|
| **#1: Links missing session names** | ❌ FALSE POSITIVE | None | No action |
| **#2: Connect failing** | ✅ CONFIRMED | HIGH | Fix immediately |

---

## Bug #1: Links Not Including Session Name ❌

### Status: FALSE POSITIVE

**What user reported:**
> "Links are missing session names" after commit 613c5fb

**What actually happened:**
- Commit 613c5fb changed **link text** only (Open → Resume/Continue/Open/Connect)
- Link URLs **correctly include session names**: `connect_{display_name}`
- No bug exists in the URL generation

**Evidence:**
```rust
// Current code (bot.rs:529)
let link = format!("https://t.me/{}?start=connect_{}", bot_username, display_name);
                                                    // ↑ Session name here
```

**Example link:**
```
https://t.me/commander_bot?start=connect_ai-commander
                                        ↑ session name present
```

**Action:** None required - bug does not exist

---

## Bug #2: Connect Failing Altogether ✅

### Status: CONFIRMED - HIGH PRIORITY

**What user reported:**
> "Connection is completely broken"

**Root cause:**
```rust
// state.rs:25
let tmux = TmuxOrchestrator::new().ok();  // ← Can return None!

// commands.rs:41
let tmux = state.tmux.as_ref().ok_or("Tmux not initialized")?;
                                     // ↑ Cryptic error message
```

**Failure scenario:**
1. Tmux binary not in PATH, or tmux server not running
2. `TmuxOrchestrator::new()` fails silently → `state.tmux = None`
3. User clicks "Connect" → Backend returns "Tmux not initialized"
4. Frontend logs error to console only → **User sees nothing**

**Impact:**
- Users cannot connect to sessions
- No visible error indication
- Poor UX during failure

---

## Immediate Fixes Required

### 1. Show Errors to Users (Frontend)

**File:** `crates/commander-gui/ui/src/lib/components/SessionList.svelte`

**Change:**
```typescript
async function connect(name: string) {
  try {
    await invoke('connect_session', { name });
    // ... success handling
  } catch (error) {
    // ADD THIS: Show error to user
    notifications.error({
      title: 'Connection Failed',
      message: error.toString(),
      timeout: 5000
    });
  }
}
```

### 2. Better Error Messages (Backend)

**File:** `crates/commander-gui/src/commands.rs`

**Change:**
```rust
pub async fn connect_session(name: String, state: State<'_, GuiState>) -> Result<(), String> {
    let tmux = state.tmux.as_ref().ok_or(
        "Tmux is not available. Please ensure tmux is installed and running."
        // ↑ More helpful message
    )?;

    if !tmux.session_exists(&name) {
        return Err(format!(
            "Session '{}' not found. It may have been stopped or deleted.",
            name
        ));
    }

    *state.current_session.write().unwrap() = Some(name.clone());
    Ok(())
}
```

---

## Optional Improvements

### Add Health Check

```rust
#[tauri::command]
pub async fn check_tmux_status(state: State<'_, GuiState>) -> Result<bool, String> {
    Ok(state.tmux.is_some())
}
```

Call on frontend startup to warn users if Tmux unavailable.

### Auto-Start Tmux

```rust
impl GuiState {
    pub fn new() -> Result<Self> {
        let mut tmux = TmuxOrchestrator::new().ok();

        // Try starting tmux server if not running
        if tmux.is_none() {
            let _ = std::process::Command::new("tmux")
                .arg("start-server")
                .status();
            tmux = TmuxOrchestrator::new().ok();
        }

        // ... rest of initialization
    }
}
```

---

## Testing Plan

### Verify Bug #1 (False Positive)
```bash
# Check notification links in bot.rs
rg "format.*connect_" crates/commander-telegram/src/bot.rs

# Expected: Should find line with display_name in URL parameter
```

### Reproduce Bug #2
```bash
# 1. Stop tmux
killall tmux

# 2. Start GUI
cd crates/commander-gui && cargo run

# 3. Try to connect to a session
# Expected: "Tmux not initialized" error (currently only in console)
```

### Verify Fix for Bug #2
1. Apply error message improvements
2. Apply frontend notification changes
3. Reproduce bug scenario
4. **Expected:** User sees error notification with helpful message

---

## Timeline

- **2026-02-22:** Commit 613c5fb changed link text (Bug #1 false positive originated)
- **2026-02-21:** Commit 474ee83 fixed session switching (unrelated to bugs)
- **2026-02-23:** Investigation confirms Bug #1 is false positive, Bug #2 is real

---

## Priority

**Bug #2 (Connect Failure):**
- Severity: HIGH
- User Impact: Cannot use core functionality
- Fix Complexity: Low (2 file changes)
- Estimated Time: 30 minutes

**Bug #1 (Link Names):**
- Severity: None
- User Impact: None (false positive)
- Fix Complexity: N/A
- Action: Document findings for user

---

**Next Steps:**
1. Apply Bug #2 fixes (error messages + notifications)
2. Test with tmux stopped scenario
3. Add health check for better UX
4. Consider auto-recovery mechanism

**Full investigation:** See `docs/research/critical-bugs-investigation-2026-02-23.md`
