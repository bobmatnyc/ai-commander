# Agent Orchestration Architecture Analysis

**Date:** 2026-02-05
**Status:** Complete
**Research Type:** Architectural Analysis

## Executive Summary

This analysis examines the current agent orchestration patterns in ai-commander to understand what changes would make agent orchestration "standard behavior" rather than optional/manual. Currently, agent orchestration exists but is entirely optional and behind a Cargo feature flag. The default behavior routes all user input directly to tmux sessions without any agent processing layer.

### Key Findings

1. **AgentOrchestrator is feature-gated**: Behind `agents` feature flag (not in defaults)
2. **Three interfaces bypass orchestration entirely**: REPL, Telegram, and TUI (by default)
3. **Direct tmux routing is the norm**: `TmuxOrchestrator.send_line()` is the primary message path
4. **Scope of change**: Medium-large (affects 3 interfaces, feature flags, and initialization)

---

## 1. Current Orchestration Architecture

### 1.1 Two Orchestrator Types

The codebase has two distinct "orchestrator" concepts that serve different purposes:

#### TmuxOrchestrator (Always Available)
- **Location:** `crates/commander-tmux/src/orchestrator.rs`
- **Purpose:** Low-level tmux session management
- **Responsibilities:**
  - Create/destroy tmux sessions
  - Send lines to sessions (`send_line()`)
  - Capture output from sessions
  - Manage window panes
- **Usage:** Used by ALL interfaces (REPL, TUI, Telegram)

#### AgentOrchestrator (Feature-Gated)
- **Location:** `crates/commander-orchestrator/src/orchestrator.rs`
- **Purpose:** Multi-agent system coordination
- **Responsibilities:**
  - Process user input through UserAgent
  - Manage SessionAgents per session
  - Track feedback via AutoEval
  - Memory store management
- **Usage:** Only in agent CLI and optionally in TUI (when feature enabled)

### 1.2 Feature Flag Structure

```toml
# crates/ai-commander/Cargo.toml
[features]
default = []  # EMPTY - agents not enabled by default
agents = ["commander-orchestrator"]

[dependencies]
commander-orchestrator = { path = "../commander-orchestrator", optional = true }
```

### 1.3 Component Dependency Graph

```
                    ┌─────────────────────────┐
                    │     User Interfaces     │
                    └─────────────────────────┘
                               │
           ┌───────────────────┼───────────────────┐
           ▼                   ▼                   ▼
    ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
    │    REPL     │     │     TUI     │     │  Telegram   │
    └─────────────┘     └─────────────┘     └─────────────┘
           │                   │                   │
           │            ┌──────┴──────┐            │
           │            ▼             ▼            │
           │    AgentOrchestrator  (OPTIONAL)      │
           │    #[cfg(feature)]       │            │
           │            │             │            │
           └────────────┼─────────────┼────────────┘
                        ▼             ▼
              ┌─────────────────────────────┐
              │      TmuxOrchestrator       │
              │      (ALWAYS USED)          │
              └─────────────────────────────┘
                           │
                           ▼
              ┌─────────────────────────────┐
              │     tmux sessions           │
              │   (claude-code, mpm, etc)   │
              └─────────────────────────────┘
```

---

## 2. Code Paths Bypassing Agent Orchestration

### 2.1 REPL Interface (100% Bypass)

**File:** `crates/ai-commander/src/repl.rs`

The REPL never uses AgentOrchestrator. All messages go directly to tmux:

```rust
// Line 711: Direct tmux send
match tmux.send_line(session, None, &message) {
    Ok(_) => {
        println!("[{}] > {}", project, message);
        // ... polling loop for response
    }
}
```

**Message Flow:**
```
User Input → REPL → TmuxOrchestrator.send_line() → tmux session
```

There is no agent processing in this path. The REPL has:
- No imports of `commander-orchestrator`
- No feature gates for agents
- No optional orchestrator field
- Direct `tmux.send_line()` calls throughout

### 2.2 Telegram Bot (100% Bypass)

**File:** `crates/commander-telegram/src/state.rs`

The Telegram bot uses `TmuxOrchestrator` directly:

```rust
pub struct TelegramState {
    sessions: RwLock<HashMap<i64, UserSession>>,
    tmux: Option<TmuxOrchestrator>,  // Not AgentOrchestrator
    adapters: AdapterRegistry,
    store: StateStore,
    authorized_chats: RwLock<HashSet<i64>>,
}
```

**Message Flow:**
```
Telegram Message → TelegramState → TmuxOrchestrator.send_line() → tmux session
```

The Telegram crate:
- Has no dependency on `commander-orchestrator`
- Uses `TmuxOrchestrator` for all session interactions
- No feature flags for agent support

### 2.3 TUI Interface (Default Bypass)

**Files:**
- `crates/ai-commander/src/tui/app.rs`
- `crates/ai-commander/src/tui/messaging.rs`
- `crates/ai-commander/src/tui/agents.rs`

The TUI has optional agent support, but it's disabled by default:

```rust
// app.rs - AgentOrchestrator is feature-gated and optional
#[cfg(feature = "agents")]
pub(super) orchestrator: Option<AgentOrchestrator>,

// Constructor initializes to None
#[cfg(feature = "agents")]
orchestrator: None,
```

**Default Message Flow (without agents feature):**
```rust
// messaging.rs line 30 - Direct tmux send
tmux.send_line(session, None, message)
    .map_err(|e| format!("Failed to send: {}", e))?;
```

Even when compiled with `agents` feature:
- `orchestrator` starts as `None`
- `init_orchestrator()` must be called explicitly
- No automatic initialization in `App::new()`

### 2.4 Agent CLI (Only Path Using Orchestration)

**File:** `crates/ai-commander/src/agent_cli.rs`

This is the ONLY place that actually uses `AgentOrchestrator`:

```rust
#[cfg(feature = "agents")]
pub async fn handle_chat(interactive: bool, message: Option<String>) -> Result<(), Box<dyn Error>> {
    let mut orchestrator = AgentOrchestrator::new().await?;

    // ...
    match orchestrator.process_user_input(input).await {
        Ok(response) => println!("\nAgent: {}\n", response),
        // ...
    }
}
```

But this is a separate CLI subcommand (`ai-commander agent chat`), not the main message path.

---

## 3. Data Flow Comparison

### 3.1 Current Default Flow (Without Orchestration)

```
┌──────────┐     ┌───────────────┐     ┌─────────────────┐     ┌──────────────┐
│   User   │ ──► │  Interface    │ ──► │ TmuxOrchestrator│ ──► │ tmux session │
│  Input   │     │ (REPL/TUI/TG) │     │   send_line()   │     │ (claude-code)│
└──────────┘     └───────────────┘     └─────────────────┘     └──────────────┘
                                                                      │
                                                                      ▼
┌──────────┐     ┌───────────────┐     ┌─────────────────┐     ┌──────────────┐
│  Display │ ◄── │   Interface   │ ◄── │ TmuxOrchestrator│ ◄── │    Output    │
│          │     │               │     │ capture_output()│     │              │
└──────────┘     └───────────────┘     └─────────────────┘     └──────────────┘
```

### 3.2 Desired Flow (With Orchestration as Standard)

```
┌──────────┐     ┌───────────────┐     ┌─────────────────────┐
│   User   │ ──► │  Interface    │ ──► │  AgentOrchestrator  │
│  Input   │     │ (REPL/TUI/TG) │     │ process_user_input()│
└──────────┘     └───────────────┘     └─────────────────────┘
                                                  │
                                                  ▼
                                        ┌─────────────────────┐
                                        │     UserAgent       │
                                        │  (intent analysis,  │
                                        │   context, memory)  │
                                        └─────────────────────┘
                                                  │
                                                  ▼
                                        ┌─────────────────────┐     ┌──────────────┐
                                        │  TmuxOrchestrator   │ ──► │ tmux session │
                                        │     send_line()     │     │              │
                                        └─────────────────────┘     └──────────────┘
                                                                           │
                                                                           ▼
                                        ┌─────────────────────┐     ┌──────────────┐
                                        │    SessionAgent     │ ◄── │   Output     │
                                        │  (output analysis)  │     │              │
                                        └─────────────────────┘     └──────────────┘
                                                  │
                                                  ▼
┌──────────┐     ┌───────────────┐     ┌─────────────────────┐
│  Display │ ◄── │   Interface   │ ◄── │  AgentOrchestrator  │
│          │     │               │     │   (processed result)│
└──────────┘     └───────────────┘     └─────────────────────┘
```

---

## 4. Recommendations for Making Orchestration Standard

### 4.1 Phase 1: Enable Feature by Default

**Change:** Modify `crates/ai-commander/Cargo.toml`

```toml
[features]
default = ["agents"]  # Changed from []
agents = ["commander-orchestrator"]
```

**Impact:** Low risk, enables compilation with orchestrator

### 4.2 Phase 2: Auto-Initialize Orchestrator in TUI

**Change:** Modify `crates/ai-commander/src/tui/app.rs`

```rust
impl App {
    pub fn new(state_dir: &std::path::Path) -> Self {
        // ... existing code ...

        let mut app = Self { /* ... */ };

        // NEW: Auto-initialize orchestrator
        #[cfg(feature = "agents")]
        {
            // Note: This is sync context, may need tokio::runtime::Handle
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                let _ = rt.block_on(app.init_orchestrator());
            }
        }

        app
    }
}
```

**Impact:** Medium - TUI uses orchestrator by default

### 4.3 Phase 3: Route TUI Messages Through Orchestrator

**Change:** Modify `crates/ai-commander/src/tui/messaging.rs`

```rust
pub fn send_message(&mut self, message: &str) -> Result<(), String> {
    #[cfg(feature = "agents")]
    if let Some(ref mut orchestrator) = self.orchestrator {
        // Route through agent first
        // Note: Need to handle async in sync context
        let processed = /* await orchestrator.process_user_input(message) */;
        // Then send processed message to tmux
    }

    // Fallback: direct send (existing code)
    // ...
}
```

**Considerations:**
- Async/sync boundary handling (TUI event loop is sync)
- Message transformation (what does UserAgent return?)
- Error handling when orchestrator fails

### 4.4 Phase 4: Add Orchestrator to REPL

**Change:** Major refactor of `crates/ai-commander/src/repl.rs`

1. Add dependency on `commander-orchestrator`
2. Add optional `AgentOrchestrator` field
3. Initialize in REPL startup
4. Route `send_message` calls through orchestrator
5. Handle async (REPL already uses tokio runtime)

**Estimated Changes:** ~100-200 lines

### 4.5 Phase 5: Add Orchestrator to Telegram

**Change:** Modify `crates/commander-telegram/`

1. Add dependency on `commander-orchestrator` to `Cargo.toml`
2. Change `TelegramState` to include `AgentOrchestrator`
3. Route messages through orchestrator before tmux
4. Handle output analysis via SessionAgent

**Estimated Changes:** ~150-250 lines

---

## 5. Architectural Barriers

### 5.1 Async/Sync Boundary

**Problem:** TUI uses synchronous event loop, but `AgentOrchestrator::process_user_input()` is async.

**Current TUI approach:**
```rust
// messaging.rs - Summarization uses thread spawn
std::thread::spawn(move || {
    let summary = summarize_blocking_with_fallback(&query, &raw_response);
    let _ = tx.send(summary);
});
```

**Solution Options:**
1. Use `tokio::runtime::Handle::block_on()` (may block UI)
2. Spawn task and poll via mpsc channel (like summarization)
3. Convert TUI to async event loop (major refactor)

### 5.2 Message Transformation Semantics

**Problem:** What does `process_user_input()` return and how does it map to tmux?

Current `UserAgent` returns:
```rust
pub struct AgentResponse {
    pub content: String,  // Text response
    // What is this? AI commentary? The message to send?
}
```

**Questions to resolve:**
- Should `content` be sent directly to tmux?
- Should it replace user input entirely?
- Should it be displayed alongside user message?

### 5.3 Error Recovery

**Problem:** If orchestrator fails, should system fall back to direct tmux?

**Recommendation:** Yes, graceful degradation:
```rust
pub fn send_message(&mut self, message: &str) -> Result<(), String> {
    // Try orchestrator first
    #[cfg(feature = "agents")]
    if let Some(ref mut orchestrator) = self.orchestrator {
        match orchestrator.process_user_input(message).await {
            Ok(processed) => {
                // Send processed message
                return self.send_to_tmux(&processed);
            }
            Err(e) => {
                // Log but continue with fallback
                tracing::warn!("Orchestrator failed: {}, using direct send", e);
            }
        }
    }

    // Fallback: direct send
    self.send_to_tmux(message)
}
```

### 5.4 State Synchronization

**Problem:** `AgentOrchestrator` maintains its own session state (`session_agents: HashMap`). This duplicates session tracking in each interface.

**Current duplications:**
- TUI: `sessions: HashMap<String, String>` (project -> tmux session)
- REPL: `sessions: HashMap<String, String>` (same)
- Telegram: `sessions: RwLock<HashMap<i64, UserSession>>`
- Orchestrator: `session_agents: HashMap<String, SessionAgent>`

**Solution:** Consider having orchestrator own the authoritative session list, with interfaces delegating to it.

---

## 6. Estimated Scope of Changes

### 6.1 Line Count Estimates

| Component | Files Changed | Lines Added | Lines Modified | Complexity |
|-----------|---------------|-------------|----------------|------------|
| Feature flag | 1 | 1 | 1 | Low |
| TUI auto-init | 2 | 20 | 5 | Low |
| TUI messaging | 1 | 50 | 30 | Medium |
| REPL integration | 1 | 150 | 50 | Medium |
| Telegram integration | 3 | 200 | 100 | High |
| **Total** | **8** | **~420** | **~185** | **Medium** |

### 6.2 Risk Assessment

| Change | Risk Level | Mitigation |
|--------|------------|------------|
| Feature flag default | Low | Can revert easily |
| TUI auto-init | Low | Graceful fallback if fails |
| TUI messaging | Medium | Feature flag allows disable |
| REPL integration | Medium | Keep direct-send fallback |
| Telegram integration | High | Thorough testing needed |

### 6.3 Suggested Implementation Order

1. **Week 1:** Enable feature flag by default, TUI auto-init
2. **Week 2:** TUI messaging through orchestrator
3. **Week 3:** REPL integration
4. **Week 4:** Telegram integration
5. **Week 5:** Testing, documentation, edge cases

---

## 7. Open Questions

1. **Message Semantics:** When `UserAgent.process()` returns a response, what should happen with it?
   - Send to tmux as-is?
   - Use as context for the original message?
   - Display to user before sending?

2. **Session Agent Role:** When should `SessionAgent.analyze_output()` be called?
   - After every output poll?
   - Only when detecting completion?
   - How to surface analysis results to UI?

3. **Memory/Feedback:** How should memory store and feedback tracking integrate with existing UI patterns?
   - Persist between sessions?
   - Display feedback scores to user?

4. **API Keys:** `AgentOrchestrator::new()` requires API keys. How to handle missing keys gracefully?
   - Silent fallback to direct mode?
   - Warn user once at startup?
   - Block until configured?

---

## 8. Conclusion

Making agent orchestration standard requires moderate changes across all three interfaces. The architecture already supports it through the `AgentOrchestrator` abstraction - the main work is routing existing message flows through this new layer.

**Key actions:**
1. Enable `agents` feature by default
2. Auto-initialize orchestrator in each interface
3. Route all `send_message` calls through orchestrator
4. Implement graceful fallback for failures
5. Resolve async/sync boundaries appropriately per interface

The total scope is approximately 600 lines of code changes across 8 files, with medium overall complexity. A phased approach over 4-5 weeks is recommended to minimize risk and allow for iterative testing.
