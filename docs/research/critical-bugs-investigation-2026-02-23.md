# Critical Bugs Investigation - AI Commander

**Date:** 2026-02-23
**Investigator:** Research Agent (Claude Code)
**Context:** User reports two critical bugs: (1) Links missing session names, (2) Connect failing

## Executive Summary

**Bug #1 (Links Not Including Session Name): FALSE POSITIVE**
- Investigation shows the bug does NOT exist
- Links correctly include session names in URL parameter `connect_{display_name}`
- Recent commit 613c5fb only changed **link text**, not the URL itself
- User may have confused link text changes with URL parameter changes

**Bug #2 (Connect Failing): CONFIRMED - TMUX INITIALIZATION ISSUE**
- Root cause: `TmuxOrchestrator::new()` can fail silently
- When Tmux initialization fails, `state.tmux` is `None`
- `connect_session` command returns "Tmux not initialized" error
- This is a graceful failure, but provides poor UX with unclear error messages

## Detailed Investigation

### Bug #1: Links Not Including Session Name

#### User Report
> "Links not including session name"
> Referenced commit: 613c5fb

#### Investigation Process

1. **Examined commit 613c5fb:**
   - Title: "fix(telegram): add context-aware link text for notifications"
   - Purpose: Make link text context-aware (Resume/Continue/Open/Connect)

2. **Reviewed code changes:**

**BEFORE (commit 49b1ea5):**
```rust
let link = format!("https://t.me/{}?start=connect_{}", bot_username, display_name);
message.push_str(&format!("\n\n👉 <a href=\"{}\">Open {}</a>", link, display_name));
```

**AFTER (commit 613c5fb):**
```rust
let link = format!("https://t.me/{}?start=connect_{}", bot_username, display_name);

// Choose link text based on notification context
let link_text = if message.contains("resumed work") || message.contains("resumed") {
    format!("Resume {}", display_name)
} else if message.contains("paused") || message.contains("waiting") {
    format!("Continue {}", display_name)
} else if message.contains("ready") || message.contains("started") {
    format!("Open {}", display_name)
} else {
    format!("Connect to {}", display_name)
};

message.push_str(&format!("\n\n👉 <a href=\"{}\">{}</a>", link, link_text));
```

3. **Analysis:**
   - **Link URL generation:** Unchanged - still uses `display_name` correctly
   - **Link text:** Changed from static "Open {display_name}" to context-aware text
   - **URL parameter:** Still correctly includes `connect_{display_name}`

#### Verification

**Current code (bot.rs:523-543):**
```rust
let display_name = session.strip_prefix("commander-").unwrap_or(session);
// Generate deep link for connecting to this session
let bot_username = match bot.get_me().await {
    Ok(me) => me.username().to_string(),
    Err(_) => "commander".to_string(),
};
let link = format!("https://t.me/{}?start=connect_{}", bot_username, display_name);
```

The session name is correctly included in the URL as `connect_{display_name}`.

#### Conclusion

**Bug Status:** FALSE POSITIVE - Bug does not exist

**Explanation:**
- The user may have confused the **link text** change with the **URL parameter**
- Example generated link: `https://t.me/commander_bot?start=connect_ai-commander`
  - Session name is present: `ai-commander`
  - Only the display text changed: "Resume ai-commander" vs "Open ai-commander"

**Recommendation:** No code changes needed

---

### Bug #2: Connect Failing Altogether

#### User Report
> "Connect failing altogether"
> Context: Users clicking connect links or buttons experience failures

#### Investigation Process

1. **Reviewed connect_session command:**

**Current implementation (commands.rs:40-49):**
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

2. **Examined GuiState initialization:**

**State structure (state.rs:7-12):**
```rust
pub struct GuiState {
    pub store: Arc<StateStore>,
    pub tmux: Option<Arc<TmuxOrchestrator>>,  // ← Can be None!
    pub current_session: Arc<RwLock<Option<String>>>,
    pub bot_status: Arc<RwLock<DaemonStatus>>,
}
```

**Initialization (state.rs:21-36):**
```rust
impl GuiState {
    pub fn new() -> Result<Self> {
        let state_dir = commander_core::config::state_dir();
        let store = StateStore::new(state_dir);
        let tmux = TmuxOrchestrator::new().ok();  // ← Returns None if fails!

        Ok(Self {
            store: Arc::new(store),
            tmux: tmux.map(Arc::new),  // ← None propagated here
            current_session: Arc::new(RwLock::new(None)),
            bot_status: Arc::new(RwLock::new(DaemonStatus {
                running: false,
                pid: None,
            })),
        })
    }
}
```

3. **Identified failure modes:**

**Scenario A: Tmux Not Running**
```
User Action: Click "Connect" button
Result: Error "Tmux not initialized"
Cause: TmuxOrchestrator::new() fails if tmux binary not found or not running
```

**Scenario B: Session Does Not Exist**
```
User Action: Click deep link for non-existent session
Result: Error "Session 'name' not found"
Cause: Session was stopped or name mismatch
```

**Scenario C: Frontend Error Handling**

Reviewed SessionList.svelte:25-50 for error handling:
```typescript
async function connect(name: string) {
  try {
    await invoke('connect_session', { name });
    const session = $sessions.find(s => s.name === name);
    if (session) {
      currentSession.set({ ...session, is_connected: true });

      // Add initial connection message
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

**Issue found:** Error is logged to console but not shown to user!

#### Root Cause Analysis

**Primary Issue:** Silent Tmux initialization failure
- `TmuxOrchestrator::new()` can fail if:
  - Tmux binary not in PATH
  - Tmux socket permissions issue
  - Tmux server not running
- Failure is silently converted to `None` via `.ok()`
- User receives cryptic "Tmux not initialized" error

**Secondary Issue:** Poor error visibility
- Errors caught in frontend but only logged to console
- No user-facing error notification
- User has no indication of what went wrong

#### Verification

**Build status:**
```bash
cargo build --package commander-gui
# Result: Compiles successfully with 1 warning (unused field `store`)
```

**Runtime behavior (hypothetical):**
1. User clicks "Connect to ai-commander"
2. Frontend calls `invoke('connect_session', { name: 'commander-ai-commander' })`
3. Backend checks `state.tmux.as_ref()`
4. If `None`, returns `Err("Tmux not initialized")`
5. Frontend catches error, logs to console
6. User sees no feedback

#### Conclusion

**Bug Status:** CONFIRMED

**Root Causes:**
1. **Backend:** Silent Tmux initialization failure with poor error messages
2. **Frontend:** No user-facing error notifications

**Impact:**
- Users cannot connect to sessions when Tmux fails to initialize
- No clear indication of what went wrong
- Poor user experience during failure scenarios

---

## Recommendations

### Bug #1: No Action Required
- Bug does not exist - URLs correctly include session names
- Consider adding test to verify URL format includes session name
- Update user documentation if confusion persists

### Bug #2: Immediate Actions Required

#### High Priority: Improve Error Visibility

**Frontend Changes (SessionList.svelte):**
```typescript
async function connect(name: string) {
  try {
    await invoke('connect_session', { name });
    // ... success handling
  } catch (error) {
    // Show error toast/notification to user
    notifications.error({
      title: 'Connection Failed',
      message: error.toString(),
      timeout: 5000
    });
  }
}
```

**Backend Changes (commands.rs):**
```rust
pub async fn connect_session(name: String, state: State<'_, GuiState>) -> Result<(), String> {
    // More helpful error message
    let tmux = state.tmux.as_ref().ok_or(
        "Tmux is not available. Please ensure tmux is installed and running."
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

#### Medium Priority: Tmux Health Check

Add a health check command:
```rust
#[tauri::command]
pub async fn check_tmux_status(state: State<'_, GuiState>) -> Result<bool, String> {
    Ok(state.tmux.is_some())
}
```

Frontend can check on startup and show warning if Tmux unavailable.

#### Low Priority: Auto-Recovery

Consider auto-starting Tmux or providing guidance when Tmux not running:
```rust
impl GuiState {
    pub fn new() -> Result<Self> {
        // ... existing code
        let mut tmux = TmuxOrchestrator::new().ok();

        // Attempt to start tmux if not running
        if tmux.is_none() {
            if let Ok(_) = std::process::Command::new("tmux").arg("start-server").status() {
                tmux = TmuxOrchestrator::new().ok();
            }
        }

        // ... rest of initialization
    }
}
```

---

## Testing Recommendations

### Bug #1 (False Positive)
**Verification test:**
```rust
#[test]
fn test_notification_link_includes_session_name() {
    let bot_username = "commander_bot";
    let display_name = "ai-commander";
    let link = format!("https://t.me/{}?start=connect_{}", bot_username, display_name);

    assert!(link.contains("connect_ai-commander"));
}
```

### Bug #2 (Connect Failure)

**Unit test for error handling:**
```rust
#[tokio::test]
async fn test_connect_session_fails_when_tmux_not_initialized() {
    let state = GuiState {
        store: Arc::new(StateStore::new(PathBuf::from("/tmp/test"))),
        tmux: None,  // Simulate uninitialized Tmux
        current_session: Arc::new(RwLock::new(None)),
        bot_status: Arc::new(RwLock::new(DaemonStatus {
            running: false,
            pid: None,
        })),
    };

    let result = connect_session("test-session".to_string(), State::from(&state)).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Tmux"));
}
```

**Integration test for error display:**
```typescript
// SessionList.test.ts
test('displays error when connect fails', async () => {
  // Mock invoke to fail
  vi.mocked(invoke).mockRejectedValue(new Error('Tmux not initialized'));

  render(SessionList);

  const connectButton = screen.getByText('Connect');
  await fireEvent.click(connectButton);

  // Should show error notification
  await waitFor(() => {
    expect(screen.getByText(/Connection Failed/i)).toBeInTheDocument();
  });
});
```

---

## Timeline of Changes

### Commit 613c5fb (2026-02-22)
- **Change:** Added context-aware link text
- **Impact on Bug #1:** None - URLs unchanged
- **Impact on Bug #2:** None - unrelated

### Commit 474ee83 (2026-02-21)
- **Change:** Fixed session switching, added create session
- **Impact on Bug #1:** None
- **Impact on Bug #2:** None - `connect_session` unchanged

### Historical Context
- `connect_session` command existed since initial Tauri implementation
- Tmux initialization issue has been present since GuiState creation
- Frontend error handling has always been console-only

---

## Conclusion

**Bug #1 is a false positive** - session names are correctly included in deep links. The recent change only affected link display text, not the URL parameter.

**Bug #2 is a real issue** caused by poor error handling when Tmux fails to initialize. The fix requires:
1. Better error messages in backend
2. User-facing error notifications in frontend
3. Optional: Health checks and auto-recovery mechanisms

**Priority:** Bug #2 should be addressed immediately to improve user experience during failure scenarios.
