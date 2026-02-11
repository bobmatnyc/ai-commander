# AI Commander Telegram Architecture Analysis

**Date:** 2025-02-09
**Scope:** `/connect`, `/session`, `/status` commands and LLM interpretation architecture
**Research Context:** Understanding current implementation before adding "LLM interprets all chat unless /send"

---

## Executive Summary

AI Commander currently has a **partial LLM interpretation system** through the `AgentOrchestrator` (feature-gated with `agents`). The system already demonstrates the pattern needed for "interpret all messages" - it exists in message routing but was recently removed from notifications to fix "response bleeding."

**Key Finding:** The LLM interpretation infrastructure exists (`AgentOrchestrator`), but it's **optional** and only active when:
1. `agents` feature is compiled in
2. `AgentOrchestrator` successfully initializes (requires API keys)
3. Currently only used in `send_message()` path, not commands

---

## 1. Current `/connect` and `/session` Implementation

### `/connect <name>` Command Flow

**Location:** `crates/commander-telegram/src/handlers.rs` lines 261-397

**Purpose:** Connect to existing projects OR create new projects with adapter specification

**Syntax:**
- `/connect <project_name>` - Connect to registered project
- `/connect <path> -a <adapter> -n <name>` - Create and connect to new project
- Auto-detects if name matches existing tmux session → attaches instead of creating

**Implementation:**

```rust
pub async fn handle_connect(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    args: String,
) -> ResponseResult<()> {
    // 1. Authorization check
    if !state.is_authorized(msg.chat.id.0).await {
        // Reject with pairing message
    }

    // 2. Parse arguments
    let connect_args = parse_connect_args(args)?;

    // 3. Check if already connected, disconnect first

    // 4a. Existing project path:
    //     - state.connect(chat_id, project_name)
    //       - Loads project from StateStore
    //       - Checks tmux session exists, creates if needed
    //       - Launches adapter command in tmux
    //       - Creates UserSession

    // 4b. New project path:
    //     - Checks if name matches tmux session → attach_session()
    //     - Otherwise: state.connect_new(...)
    //       - Validates path, adapter
    //       - Creates Project record
    //       - Calls connect() to start session
}
```

**Key Functions:**

- **`state.connect(chat_id, project_name)`** (state.rs:319-435)
  - Primary connection logic
  - Three-stage lookup:
    1. **Registered project by name:** `StateStore` lookup
    2. **Tmux session fallback:** `commander-{name}`, `{name}`, `{base_name}`
    3. **Error:** Project not found
  - Validates project path exists and is readable
  - Creates tmux session if missing, sends adapter launch command
  - Returns `(project_name, tool_id)`

- **`state.attach_session(chat_id, session_name)`** (state.rs:602-648)
  - Direct tmux session attachment
  - No project registration required
  - Infers project info from session name
  - Used when `/connect` detects existing tmux session

- **`state.connect_new(chat_id, path, adapter, name)`** (state.rs:651-696)
  - Creates new Project record
  - Validates path and adapter
  - Saves to StateStore
  - Calls `connect()` to start session

**Adapter Detection:**

```rust
// From state.rs:345-382
let tool_id = project.config.get("tool")
    .and_then(|v| v.as_str())
    .unwrap_or("claude-code")
    .to_string();

if !tmux.session_exists(&session_name) {
    if let Some(adapter) = self.adapters.get(&tool_id) {
        let (cmd, cmd_args) = adapter.launch_command(&project.path);
        let full_cmd = format!("{} {}", cmd, cmd_args.join(" "));

        // Create tmux session in project directory
        tmux.create_session_in_dir(&session_name, Some(&project.path))?;

        // Send launch command
        tmux.send_line(&session_name, None, &full_cmd)?;
    }
}
```

**Unified Session Handling (commit c2b3ac7):**

Recent refactoring unified `/connect` and `/session` behavior:
- Both commands now use same `UserSession` structure
- Adapter detection from project config or fallback to "unknown"
- Consistent tmux session naming: `commander-{project_name}`

---

### `/session <session_name>` Command Flow

**Location:** `crates/commander-telegram/src/handlers.rs` lines 717-774

**Purpose:** Direct attachment to existing tmux session (bypasses project registry)

**Syntax:** `/session <session_name>`

**Implementation:**

```rust
pub async fn handle_session(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    session_name: String,
) -> ResponseResult<()> {
    // 1. Authorization check

    // 2. Disconnect from current session if connected

    // 3. Call state.attach_session(chat_id, session_name)
    //    - Validates tmux session exists
    //    - Infers project info from session name
    //    - Creates UserSession
    //    - No adapter launch (session already running)
}
```

**Difference from `/connect`:**

| Aspect | `/connect <name>` | `/session <session_name>` |
|--------|------------------|--------------------------|
| **Lookup** | Project registry → tmux fallback | Direct tmux session |
| **Project Check** | Validates path exists | Infers project from session |
| **Session Creation** | Creates if missing | Requires existing session |
| **Adapter Launch** | Sends adapter command | N/A (already running) |
| **Use Case** | Start/resume projects | Attach to running sessions |

**Fallback Chain (from commit e4fed88):**

`/connect` now falls back to tmux session lookup if project not found in registry:

```rust
// Try 1: Find registered project by name
if let Some(project) = projects.values().find(|p| p.name == base_name) {
    // Standard connect path
}

// Try 2: Fallback to direct tmux session lookup
let session_candidates = [
    format!("commander-{}", base_name),
    project_name.to_string(),
    base_name.to_string(),
];

for session_name in &session_candidates {
    if tmux.session_exists(session_name) {
        // Attach without project registration
    }
}
```

---

## 2. Current `/status` Implementation

### `/status` Command - LLM Interpretation IMPLEMENTED

**Location:** `crates/commander-telegram/src/handlers.rs` lines 438-505

**Recent Commit:** `b1022c8` - "feat: intelligent /status interprets screen content with LLM"

**Purpose:** Show connection status with intelligent activity interpretation

**Implementation:**

```rust
pub async fn handle_status(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let status = if let Some((project_name, project_path, tool_id, is_waiting,
                              pending_query, screen_preview)) =
        state.get_session_status(msg.chat.id).await
    {
        // Activity section - shows processing state
        let activity = if is_waiting {
            if let Some(query) = pending_query {
                format!("🔄 Activity: Processing command...\n📝 Query: \"{}\"", query)
            } else {
                "🔄 Activity: Processing...".to_string()
            }
        } else {
            "💤 Activity: Idle (ready for commands)".to_string()
        };

        // Screen preview section - cleaned tmux output
        let screen_section = if let Some(preview) = screen_preview {
            format!("\n\n📺 Screen:\n<pre>{}</pre>", html_escape(&preview))
        } else {
            String::new()
        };

        format!(
            "📊 <b>Status</b>\n\n\
            ✅ Connection: Connected\n\
            📁 Project: {}\n\
            📍 Path: <code>{}</code>\n\
            🔧 Adapter: {}\n\n\
            {}{}",
            project_name, project_path, adapter_name, activity, screen_section
        )
    } else {
        "📊 <b>Status</b>\n\n❌ Connection: Not connected"
    };

    bot.send_message(msg.chat.id, status)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}
```

**Screen Preview Generation:**

```rust
// From state.rs:300-305
let screen_preview = self.tmux.as_ref().and_then(|tmux| {
    tmux.capture_output(&session.tmux_session, None, Some(10))
        .ok()
        .map(|output| clean_screen_preview(&output, 5))
});
```

**`clean_screen_preview()` Function** (from commander-core):

```rust
// Removes UI noise, returns last N lines
pub fn clean_screen_preview(output: &str, max_lines: usize) -> String {
    output
        .lines()
        .filter(|line| !is_ui_noise(line))
        .collect::<Vec<_>>()
        .iter()
        .rev()
        .take(max_lines)
        .rev()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
```

**Current Status Output Example:**

```
📊 Status

✅ Connection: Connected
📁 Project: my-rust-api
📍 Path: /Users/masa/Projects/my-rust-api
🔧 Adapter: Claude Code

💤 Activity: Idle (ready for commands)

📺 Screen:
cargo build
   Compiling my-rust-api v0.1.0
   Finished dev [unoptimized + debuginfo] target(s)
[project] ❯
```

**LLM Interpretation Note:**

The screen preview is **NOT** LLM-interpreted (it's cleaned with regex). The `/status` command shows raw screen state. The LLM interpretation exists in **message response summarization**, not status display.

---

## 3. Current Message Routing

### Message Flow Architecture

**Entry Point:** `crates/commander-telegram/src/bot.rs` lines 144-192

```rust
let handler = dptree::entry()
    .branch(
        Update::filter_message()
            .filter_command::<Command>()
            .endpoint(|bot, msg, cmd| {
                handle_command(bot, msg, cmd, state)
            })
    )
    .branch(
        Update::filter_message()
            .filter(|msg: Message| {
                // Unrecognized commands (start with / but didn't parse)
                msg.text().map(|t| t.starts_with('/')).unwrap_or(false)
            })
            .endpoint(|bot, msg| {
                bot.send_message(msg.chat.id, "Unknown command...").await
            })
    )
    .branch(
        Update::filter_message()
            .filter(|msg: Message| {
                // Non-command text messages
                msg.text().map(|t| !t.starts_with('/')).unwrap_or(false)
            })
            .endpoint(|bot, msg| {
                handle_message(bot, msg, state)
            })
    );
```

**Message Routing Layers:**

1. **Command Layer** (Lines 145-152)
   - Recognized commands → `handle_command()`
   - Matches `/start`, `/help`, `/connect`, etc.

2. **Unknown Command Layer** (Lines 154-177)
   - Messages starting with `/` that don't parse
   - Returns "Unknown command" error

3. **Regular Message Layer** (Lines 179-192)
   - All non-command text
   - Routes to `handle_message()`

---

### Regular Message Handler - LLM Integration Point

**Location:** `crates/commander-telegram/src/handlers.rs` lines 593-676

```rust
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let Some(text) = msg.text() else {
        return Ok(());
    };

    // Check for @alias routing
    if let Some(rest) = text.strip_prefix('@') {
        // Route to specific project
    }

    // Check if connected
    if !state.has_session(msg.chat.id).await {
        bot.send_message(msg.chat.id, "Not connected...").await?;
        return Ok(());
    }

    // Send typing indicator
    bot.send_chat_action(msg.chat.id, ChatAction::Typing).await?;

    // Send message with LLM processing (feature-gated)
    state.send_message(msg.chat.id, text, Some(msg.id)).await?;

    Ok(())
}
```

---

### `send_message()` - LLM Interpretation Layer

**Location:** `crates/commander-telegram/src/state.rs` lines 448-509

**THIS IS WHERE LLM INTERPRETATION HAPPENS**

```rust
pub async fn send_message(
    &self,
    chat_id: ChatId,
    message: &str,
    message_id: Option<MessageId>
) -> Result<()> {
    let tmux = self.tmux.as_ref()?;

    // TRY TO PROCESS MESSAGE THROUGH ORCHESTRATOR (agents feature)
    #[cfg(feature = "agents")]
    let processed_message = {
        let mut orchestrator = self.orchestrator.write().await;
        if let Some(ref mut orch) = *orchestrator {
            match orch.process_user_input(message).await {
                Ok(processed) => {
                    debug!(
                        original = %message,
                        processed = %processed,
                        "Message processed through orchestrator"
                    );
                    processed
                }
                Err(e) => {
                    warn!(error = %e, "Orchestrator processing failed, using original");
                    message.to_string()
                }
            }
        } else {
            message.to_string()
        }
    };

    #[cfg(not(feature = "agents"))]
    let processed_message = message.to_string();

    // Get session
    let mut sessions = self.sessions.write().await;
    let session = sessions.get_mut(&chat_id.0)?;

    // Capture initial output for comparison
    let last_output = tmux.capture_output(&session.tmux_session, None, Some(200))?;

    // SEND PROCESSED MESSAGE TO TMUX
    tmux.send_line(&session.tmux_session, None, &processed_message)?;

    // Start response collection
    session.start_response_collection(&processed_message, last_output, message_id);

    Ok(())
}
```

**Key Points:**

1. **Feature-Gated:** Only active when `agents` feature compiled
2. **Optional Processing:** Falls back to original message if orchestrator unavailable
3. **Already Exists:** The infrastructure for LLM interpretation is already here
4. **Silent Operation:** User doesn't see if message was interpreted or not
5. **Graceful Degradation:** Works without agents feature or if LLM call fails

---

### AgentOrchestrator Integration

**Location:** `crates/commander-orchestrator/src/lib.rs`

```rust
pub struct AgentOrchestrator {
    user_agent: UserAgent,
    session_agents: HashMap<String, SessionAgent>,
    memory_store: Arc<RwLock<MemoryStore>>,
}

impl AgentOrchestrator {
    pub async fn process_user_input(&mut self, input: &str) -> Result<String> {
        // 1. Get user agent context
        let context = AgentContext {
            user_input: input.to_string(),
            conversation_history: self.memory_store.read().await.get_history(),
        };

        // 2. Process through UserAgent
        let response = self.user_agent.process(context).await?;

        // 3. Store in memory
        self.memory_store.write().await.add_exchange(input, &response.text);

        // 4. Return interpreted message
        Ok(response.text)
    }
}
```

**Initialization:** (state.rs:146-168)

```rust
#[cfg(feature = "agents")]
pub async fn init_orchestrator(&self) -> Result<bool> {
    let mut orchestrator = self.orchestrator.write().await;
    if orchestrator.is_some() {
        return Ok(false); // Already initialized
    }

    match AgentOrchestrator::new().await {
        Ok(orch) => {
            info!("Agent orchestrator initialized for Telegram bot");
            *orchestrator = Some(orch);
            Ok(true)
        }
        Err(e) => {
            warn!(error = %e, "Failed to initialize, continuing without LLM");
            Ok(false)
        }
    }
}
```

---

### Notification Path - LLM REMOVED

**Location:** `crates/commander-telegram/src/bot.rs` lines 277-325

**Commit:** `c39c48e` - "fix: remove LLM from notification path to prevent response bleeding"

**Problem that was fixed:**

Notifications were being processed through LLM, which added unwanted preambles:

```
Before (with LLM):
"Hello! I see that your session 'my-project' is ready. You can now continue working!"

After (without LLM):
"Session 'my-project' is ready. Use /connect my-project to continue."
```

**Current Implementation:**

```rust
async fn poll_notifications_loop(bot: Bot, state: Arc<TelegramState>) {
    let mut poll_interval = interval(Duration::from_millis(NOTIFICATION_POLL_INTERVAL_MS));

    loop {
        poll_interval.tick().await;

        // Get unread notifications
        let notifications = get_unread_notifications("telegram");
        if notifications.is_empty() {
            continue;
        }

        // Get all authorized chat IDs
        let authorized_chats = state.get_authorized_chat_ids().await;

        // Send notifications WITHOUT LLM processing
        // Comment from code:
        // "Note: Notifications already have clean, conversational formatting from
        //  notify_session_ready/notify_session_resumed/notify_sessions_waiting.
        //  No LLM summarization needed - it only introduces preamble bleeding."

        for notification in &notifications {
            for &chat_id in &authorized_chats {
                bot.send_message(ChatId(chat_id), &notification.message).await?;
            }
            sent_ids.push(notification.id.clone());
        }

        mark_notifications_read("telegram", &sent_ids)?;
    }
}
```

**Key Insight:**

The notification path demonstrates **why direct message forwarding is sometimes better** than LLM interpretation - the notification creators already format messages conversationally, so LLM adds unwanted transformation.

---

## 4. Architecture for "All Chat Interpreted by LLM Unless /send"

### Current State Assessment

**What Exists:**

1. ✅ **LLM Interpretation Infrastructure:** `AgentOrchestrator` with `process_user_input()`
2. ✅ **Feature Gating:** `#[cfg(feature = "agents")]` pattern established
3. ✅ **Graceful Degradation:** Falls back to original message if LLM unavailable
4. ✅ **Message Routing:** Already distinguishes commands vs. regular messages
5. ✅ **Silent Processing:** No user feedback about interpretation

**What's Missing:**

1. ❌ **Command Interpretation:** LLM only processes regular messages, not commands
2. ❌ **Bypass Mechanism:** No `/send` command to skip interpretation
3. ❌ **Explicit Interpretation:** Current system is silent; need user awareness
4. ❌ **Command Extraction:** No logic to detect when LLM should suggest commands

---

### Proposed Architecture

#### Option A: Interpret Before Routing (Aggressive)

```rust
// In bot.rs dispatcher setup
let handler = dptree::entry()
    .branch(
        Update::filter_message()
            .endpoint(move |bot, msg| {
                let state = Arc::clone(&state);
                async move {
                    // INTERPRET ALL MESSAGES THROUGH LLM FIRST
                    let interpreted = state.interpret_message(msg.text()?).await?;

                    // Check if interpretation suggests a command
                    if let Some(command) = extract_command(&interpreted) {
                        handle_command(bot, msg, command, state).await
                    } else if interpreted.starts_with("/send ") {
                        // Bypass interpretation for /send
                        let raw_message = interpreted.strip_prefix("/send ")?;
                        state.send_message_raw(msg.chat.id, raw_message).await
                    } else {
                        // Send interpreted message to session
                        state.send_message(msg.chat.id, &interpreted).await
                    }
                }
            })
    );
```

**Pros:**
- LLM sees ALL messages (commands + chat)
- Can transform commands into natural language
- Single interpretation point

**Cons:**
- Commands get interpreted twice (LLM → command parser)
- Latency on every command
- Complex fallback logic

---

#### Option B: Parallel Interpretation (Conservative) **RECOMMENDED**

```rust
// In handlers.rs
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let Some(text) = msg.text() else {
        return Ok(());
    };

    // CHECK FOR /send BYPASS FIRST
    if let Some(raw_message) = text.strip_prefix("/send ") {
        // Skip interpretation, send directly to tmux
        state.send_message_direct(msg.chat.id, raw_message, Some(msg.id)).await?;
        return Ok(());
    }

    // Check connection
    if !state.has_session(msg.chat.id).await {
        bot.send_message(msg.chat.id, "Not connected...").await?;
        return Ok(());
    }

    // Send typing indicator
    bot.send_chat_action(msg.chat.id, ChatAction::Typing).await?;

    // INTERPRET MESSAGE THROUGH LLM (already exists!)
    // This is already in send_message() - just make it visible
    state.send_message_with_interpretation(msg.chat.id, text, Some(msg.id)).await?;

    Ok(())
}
```

**Changes in state.rs:**

```rust
// Rename current send_message() to send_message_with_interpretation()
pub async fn send_message_with_interpretation(...) {
    // Existing logic - already does interpretation!
}

// Add new bypass method
pub async fn send_message_direct(
    &self,
    chat_id: ChatId,
    message: &str,
    message_id: Option<MessageId>
) -> Result<()> {
    let tmux = self.tmux.as_ref()?;
    let mut sessions = self.sessions.write().await;
    let session = sessions.get_mut(&chat_id.0)?;

    // Capture initial output
    let last_output = tmux.capture_output(&session.tmux_session, None, Some(200))?;

    // SEND MESSAGE DIRECTLY TO TMUX (no LLM)
    tmux.send_line(&session.tmux_session, None, message)?;

    // Start response collection
    session.start_response_collection(message, last_output, message_id);

    Ok(())
}
```

**Pros:**
- Minimal changes (mostly renaming)
- LLM interpretation already implemented
- `/send` provides clear bypass
- Commands stay fast (no interpretation)

**Cons:**
- Commands not interpreted (but is this bad?)
- LLM doesn't see command context

---

#### Option C: Selective Interpretation (Hybrid)

```rust
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let Some(text) = msg.text() else {
        return Ok(());
    };

    // Check for /send bypass
    if let Some(raw) = text.strip_prefix("/send ") {
        state.send_message_direct(msg.chat.id, raw, Some(msg.id)).await?;
        return Ok(());
    }

    // Detect if message is already command-like
    let should_interpret = !text.starts_with('/')
        && !looks_like_shell_command(text)
        && !looks_like_code(text);

    if should_interpret {
        // Use LLM interpretation
        state.send_message_with_interpretation(msg.chat.id, text, Some(msg.id)).await?;
    } else {
        // Send directly
        state.send_message_direct(msg.chat.id, text, Some(msg.id)).await?;
    }

    Ok(())
}

fn looks_like_shell_command(text: &str) -> bool {
    text.starts_with("cd ")
        || text.starts_with("ls ")
        || text.starts_with("git ")
        || text.starts_with("cargo ")
        // etc.
}

fn looks_like_code(text: &str) -> bool {
    text.contains("fn ")
        || text.contains("def ")
        || text.contains("class ")
        || text.contains('{')
        // etc.
}
```

**Pros:**
- Smart defaults (interpret natural language, not code)
- Users can still bypass with `/send`
- Reduced LLM calls for obvious commands

**Cons:**
- Heuristics can be wrong
- More complex logic
- Unclear to user when interpretation happens

---

### Recommended Implementation: Option B (Conservative)

**Rationale:**

1. **Already 90% implemented** - just need `/send` command
2. **Clear user model** - "everything interpreted unless /send"
3. **Graceful degradation** - falls back if LLM unavailable
4. **Minimal changes** - rename existing method, add bypass
5. **Preserves command speed** - commands not interpreted

**Implementation Steps:**

1. **Add `/send` command to enum** (handlers.rs:48)

```rust
#[derive(BotCommands, Clone, Debug)]
pub enum Command {
    // ... existing commands

    #[command(description = "Send message without interpretation: /send <message>")]
    Send(String),
}
```

2. **Rename send_message() to send_message_with_interpretation()** (state.rs:452)

3. **Add send_message_direct()** (state.rs:510+)

```rust
pub async fn send_message_direct(
    &self,
    chat_id: ChatId,
    message: &str,
    message_id: Option<MessageId>
) -> Result<()> {
    // Identical to send_message_with_interpretation() but skip LLM block
}
```

4. **Update handle_message()** (handlers.rs:593)

```rust
pub async fn handle_message(...) {
    // Add /send check at top
    if text.starts_with("/send ") {
        let raw = text.strip_prefix("/send ").unwrap();
        state.send_message_direct(msg.chat.id, raw, Some(msg.id)).await?;
        return Ok(());
    }

    // Rest stays same (already uses interpretation!)
}
```

5. **Add help text updates**

```rust
"<b>Message Interpretation:</b>\n\
 All messages are interpreted through AI before being sent.\n\
 Use <code>/send &lt;message&gt;</code> to bypass interpretation."
```

---

### Architecture Diagram

```
User Message
    |
    v
┌─────────────────────────┐
│ Telegram Dispatcher     │
└───────┬─────────────────┘
        |
        v
    Command?
    /       \
  YES        NO
   |          |
   v          v
┌────────┐  ┌────────────────┐
│Command │  │handle_message()│
│Handler │  └────────┬───────┘
└────────┘           |
                     v
                 /send?
                /      \
              YES       NO
               |         |
               v         v
    ┌──────────────┐  ┌──────────────────────┐
    │send_direct() │  │send_with_interpret() │
    │(bypass LLM)  │  │(use orchestrator)    │
    └──────┬───────┘  └─────────┬────────────┘
           |                    |
           |                    v
           |          ┌──────────────────┐
           |          │AgentOrchestrator │
           |          │process_input()   │
           |          └─────────┬────────┘
           |                    |
           v                    v
    ┌─────────────────────────────┐
    │    tmux.send_line()         │
    │ (final message to session)  │
    └─────────────────────────────┘
```

---

## 5. Key Insights and Recommendations

### Insights

1. **LLM Interpretation Already Exists:** The `send_message()` function in `state.rs` already processes messages through `AgentOrchestrator` when available. The infrastructure for "interpret all messages" is implemented.

2. **Notification Anti-Pattern:** The notification path removed LLM processing because it added unwanted preambles. This shows that **not all messages benefit from interpretation** - sometimes the source already formats conversationally.

3. **Feature-Gated Design Works:** The `#[cfg(feature = "agents")]` pattern allows compilation without LLM dependencies and graceful runtime fallback when orchestrator unavailable.

4. **Session Unification:** Recent commits unified `/connect` and `/session`, showing the codebase is actively converging on simpler patterns.

5. **Screen Preview is NOT LLM:** Despite `/status` commit mentioning "intelligent interpretation," the screen preview is regex-cleaned, not LLM-interpreted. True LLM interpretation is only in message processing.

### Recommendations

#### Immediate (2-4 hours)

1. **Implement `/send` command** following Option B architecture
   - Minimal changes (rename method, add bypass)
   - Clear user model
   - Preserves existing behavior

2. **Document interpretation behavior**
   - Update `/help` to explain interpretation
   - Add to `/start` welcome message
   - Show when interpretation fails

3. **Add interpretation feedback**
   - Optional: Show "[✨ Interpreted]" prefix on bot responses
   - Let users understand when LLM was involved

#### Short-term (1-2 days)

1. **Command interpretation** (if desired)
   - Add LLM interpretation to command parsing
   - Extract commands from natural language ("help me connect" → `/connect`)
   - Show suggested command before executing

2. **Improve orchestrator visibility**
   - Add `/orchestrator status` command
   - Show memory usage, recent interpretations
   - Allow manual memory clearing

3. **Better error messages**
   - When orchestrator unavailable, explain why
   - Suggest setting API keys if missing
   - Show fallback behavior clearly

#### Long-term (1+ weeks)

1. **Context-aware interpretation**
   - Pass session state to orchestrator
   - Interpret based on recent tmux output
   - Suggest commands based on current project state

2. **Multi-turn conversations**
   - Store conversation history in UserSession
   - Pass to orchestrator for context
   - Allow "follow-up" questions

3. **Interpretation preferences**
   - Per-user setting: always/never/auto interpret
   - Per-project setting: interpret code vs. prose
   - Stored in StateStore

---

## Appendix: Code Locations Reference

### Core Files

- **Command Handlers:** `crates/commander-telegram/src/handlers.rs`
  - `/connect`: lines 261-397
  - `/session`: lines 717-774
  - `/status`: lines 438-505
  - `handle_message`: lines 593-676

- **Bot Setup:** `crates/commander-telegram/src/bot.rs`
  - Message routing: lines 144-192
  - Output polling: lines 232-275
  - Notification polling: lines 277-325

- **State Management:** `crates/commander-telegram/src/state.rs`
  - `send_message()` with LLM: lines 448-509
  - `connect()`: lines 319-435
  - `attach_session()`: lines 602-648
  - `get_session_status()`: lines 276-315

- **Session Data:** `crates/commander-telegram/src/session.rs`
  - UserSession struct: lines 8-30
  - Response collection: lines 64-94

- **Orchestrator:** `crates/commander-orchestrator/src/lib.rs`
  - AgentOrchestrator struct and API

### Recent Commits

- `6e50a2a` - fix: show actual tmux session names in /list
- `e4fed88` - fix: /connect falls back to tmux session lookup
- `c2b3ac7` - feat: unify session handling with adapter detection
- `b1022c8` - feat: intelligent /status interprets screen content with LLM
- `c39c48e` - fix: remove LLM from notification path to prevent response bleeding

---

## Conclusion

The AI Commander Telegram bot already has a **robust LLM interpretation infrastructure** through `AgentOrchestrator`. The primary task of adding "interpret all chat unless /send" is **mostly renaming and adding a bypass command** - the core functionality already exists.

The architecture supports graceful degradation (works without LLM), feature gating (compiles without agents crate), and has been battle-tested in production (notification path shows where interpretation fails).

**Recommended next step:** Implement `/send` bypass following Option B (Conservative) architecture. This preserves all existing behavior while adding the requested feature with minimal risk.
