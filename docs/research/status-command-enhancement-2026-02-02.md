# /status Command Enhancement Research

**Date:** 2026-02-02
**Researcher:** Claude Opus 4.5
**Task:** Investigate current `/status` implementation to understand how to enhance it

---

## Research Questions and Findings

### 1. Where is the `/status` command implemented?

**Location:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`

**Handler Function:** `handle_status` (lines 420-442)

```rust
pub async fn handle_status(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let status = if let Some((project_name, project_path)) = state.get_session_info(msg.chat.id).await
    {
        format!(
            "<b>Status: Connected</b>\n\n\
            📁 Project: {}\n\
            📍 Path: <code>{}</code>",
            project_name, project_path
        )
    } else {
        "<b>Status: Not connected</b>\n\nUse /connect &lt;project&gt; to connect to a project.".to_string()
    };

    bot.send_message(msg.chat.id, status)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    Ok(())
}
```

### 2. What does it currently display?

**When connected:**
- Status: Connected
- Project name
- Project path

**When not connected:**
- Status: Not connected
- Hint to use /connect

### 3. How can we access the required information?

#### A. The current adapter type (Claude Code, etc.)

**Available Data Sources:**

1. **Project config (stored in StateStore):**
   - Location: `crates/commander-persistence/` (StateStore)
   - Project has a `config` field with `"tool"` key containing adapter ID
   - Example: `project.config.get("tool")` returns `Some(Value::String("claude-code"))`

2. **AdapterRegistry:**
   - Location: `crates/commander-adapters/src/registry.rs`
   - Can resolve adapter ID to `Arc<dyn RuntimeAdapter>`
   - `AdapterInfo` provides: `id`, `name`, `description`, `command`, `default_args`

**Implementation Path:**
```rust
// In state.rs, from project config:
let tool_id = project.config.get("tool")
    .and_then(|v| v.as_str())
    .unwrap_or("claude-code");

// Get adapter info:
if let Some(adapter) = self.adapters.get(tool_id) {
    let info = adapter.info();
    // info.name = "Claude Code" or "MPM" etc.
}
```

#### B. Whether a command is being processed (waiting for response)

**Available in `UserSession` struct:**

```rust
// crates/commander-telegram/src/session.rs
pub struct UserSession {
    pub is_waiting: bool,              // Currently waiting for response
    pub pending_query: Option<String>, // The user's query text
    pub pending_message_id: Option<MessageId>,
    // ...
}
```

**Access Path:**
```rust
// In TelegramState, via get_session_info or similar:
let sessions = self.sessions.read().await;
if let Some(session) = sessions.get(&chat_id.0) {
    session.is_waiting       // true if processing
    session.pending_query    // Some("user question")
}
```

#### C. Current screen content from tmux

**Available via TmuxOrchestrator:**

```rust
// crates/commander-tmux/src/orchestrator.rs
pub fn capture_output(
    &self,
    session: &str,
    pane: Option<&str>,
    lines: Option<u32>,  // Number of lines to capture
) -> Result<String>
```

**Access Path:**
```rust
// In TelegramState, already used in poll_output():
let current_output = tmux
    .capture_output(&session.tmux_session, None, Some(200))
    .map_err(|e| TelegramError::TmuxError(e.to_string()))?;
```

**Note:** The raw output needs cleaning. Use existing `clean_raw_response()` or `is_ui_noise()` functions to filter Claude Code UI elements.

### 4. What session/state fields are available?

**UserSession (session.rs):**

| Field | Type | Description |
|-------|------|-------------|
| `chat_id` | `ChatId` | Telegram chat ID |
| `project_path` | `String` | Path to project directory |
| `project_name` | `String` | Project name |
| `tmux_session` | `String` | tmux session name (e.g., "commander-myproject") |
| `response_buffer` | `Vec<String>` | Collected response lines |
| `last_output_time` | `Option<Instant>` | When output last changed |
| `last_output` | `String` | Last captured tmux output |
| `pending_query` | `Option<String>` | User's current query |
| `is_waiting` | `bool` | Whether waiting for response |
| `pending_message_id` | `Option<MessageId>` | Message ID for reply threading |

**TelegramState (state.rs):**

| Field | Type | Description |
|-------|------|-------------|
| `sessions` | `RwLock<HashMap<i64, UserSession>>` | Active user sessions |
| `tmux` | `Option<TmuxOrchestrator>` | tmux integration |
| `adapters` | `AdapterRegistry` | Available adapters |
| `store` | `StateStore` | Project persistence |
| `openrouter_key` | `Option<String>` | For summarization |
| `openrouter_model` | `String` | Model for summarization |
| `authorized_chats` | `RwLock<HashSet<i64>>` | Authorized chat IDs |

---

## Recommended Implementation

### Enhanced Status Output

```
<b>Status: Connected</b>

📁 Project: my-project
📍 Path: <code>/Users/masa/Projects/my-project</code>
🔧 Adapter: Claude Code

<b>Activity:</b>
⏳ Processing: "Explain authentication flow"

<b>Screen Preview:</b>
<code>[last 5 lines of tmux output, cleaned]</code>
```

Or when idle:

```
<b>Status: Connected</b>

📁 Project: my-project
📍 Path: <code>/Users/masa/Projects/my-project</code>
🔧 Adapter: Claude Code

<b>Activity:</b>
✅ Ready for input

<b>Screen Preview:</b>
<code>[last 5 lines of tmux output, cleaned]</code>
```

### Implementation Steps

1. **Extend `get_session_info` or create new method** to return:
   - `project_name`, `project_path`, `tmux_session`
   - `is_waiting`, `pending_query`
   - Adapter name (from project config + registry lookup)

2. **Add screen capture** to status:
   - Use `tmux.capture_output(session_name, None, Some(10))`
   - Clean with `clean_raw_response()` or similar
   - Truncate to last 5-10 lines

3. **Update `handle_status`** to format and display all info

### Code Changes Required

**File: `crates/commander-telegram/src/state.rs`**

Add new method or extend existing:
```rust
pub async fn get_extended_session_info(&self, chat_id: ChatId) -> Option<ExtendedSessionInfo> {
    let sessions = self.sessions.read().await;
    let session = sessions.get(&chat_id.0)?;

    // Get adapter name from project
    let adapter_name = self.get_adapter_name(&session.project_name);

    // Get screen content
    let screen_preview = self.get_screen_preview(&session.tmux_session);

    Some(ExtendedSessionInfo {
        project_name: session.project_name.clone(),
        project_path: session.project_path.clone(),
        adapter_name,
        is_waiting: session.is_waiting,
        pending_query: session.pending_query.clone(),
        screen_preview,
    })
}
```

**File: `crates/commander-telegram/src/handlers.rs`**

Update `handle_status`:
```rust
pub async fn handle_status(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    if let Some(info) = state.get_extended_session_info(msg.chat.id).await {
        let activity = if info.is_waiting {
            format!("⏳ Processing: \"{}\"",
                info.pending_query.unwrap_or_default().chars().take(50).collect::<String>())
        } else {
            "✅ Ready for input".to_string()
        };

        let status = format!(
            "<b>Status: Connected</b>\n\n\
            📁 Project: {}\n\
            📍 Path: <code>{}</code>\n\
            🔧 Adapter: {}\n\n\
            <b>Activity:</b>\n{}\n\n\
            <b>Screen Preview:</b>\n<code>{}</code>",
            info.project_name,
            info.project_path,
            info.adapter_name,
            activity,
            info.screen_preview.chars().take(500).collect::<String>()
        );

        bot.send_message(msg.chat.id, status)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
    } else {
        bot.send_message(
            msg.chat.id,
            "<b>Status: Not connected</b>\n\nUse /connect &lt;project&gt; to connect to a project."
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    }

    Ok(())
}
```

---

## Key Files to Modify

| File | Changes |
|------|---------|
| `crates/commander-telegram/src/state.rs` | Add `get_extended_session_info()`, helper methods for adapter name and screen preview |
| `crates/commander-telegram/src/handlers.rs` | Update `handle_status()` to use new data |

---

## Summary

The `/status` command enhancement is straightforward because:

1. **Adapter info** is already stored in project config (`"tool"` key) and can be looked up via `AdapterRegistry.get(id).info().name`

2. **Processing state** is tracked in `UserSession.is_waiting` and `UserSession.pending_query`

3. **Screen content** can be captured via `TmuxOrchestrator.capture_output()` and cleaned with existing utility functions

All required data is accessible; implementation requires adding a new info-gathering method and updating the handler formatting.
