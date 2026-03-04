# Resume Message Link Investigation

**Date**: 2026-02-22
**Issue**: "Resume" notification messages show wrong link text - they only show "connect" when they should show something else

---

## Problem Statement

User reported that "resume" messages in notifications don't show the right link - they only show "connect" when they should show something else (likely a resume/continue link).

## Investigation Findings

### Where "Resume" Messages Are Generated

**Location**: `crates/commander-telegram/src/notifications.rs:204-209`

```rust
/// Convenience function to broadcast a session resumed notification.
///
/// Uses conversational language.
pub fn notify_session_resumed(session_name: &str) -> Result<(), std::io::Error> {
    let display_name = session_name.strip_prefix("commander-").unwrap_or(session_name);
    let message = format!("Session \"{}\" resumed work", display_name);

    push_notification(message, Some(session_name.to_string()))
}
```

**Key Points**:
- Called from: `crates/ai-commander/src/tui/sessions.rs:224`
- Message text: `"Session \"{}\" resumed work"`
- Notification includes session name for deep linking

### Where Links Are Added to Notifications

**Location**: `crates/commander-telegram/src/bot.rs:520-531`

```rust
// Build notification message with deep link if session is specified
let mut message = notification.message.clone();
if let Some(session) = &notification.session {
    let display_name = session.strip_prefix("commander-").unwrap_or(session);
    // Generate deep link for connecting to this session
    let bot_username = match bot.get_me().await {
        Ok(me) => me.username().to_string(),
        Err(_) => "commander".to_string(),
    };
    let link = format!("https://t.me/{}?start=connect_{}", bot_username, display_name);
    message.push_str(&format!("\n\n👉 <a href=\"{}\">Open {}</a>", link, display_name));
}
```

**Key Points**:
- ALL notification messages with a session get the SAME link text format
- Link text: `"Open {display_name}"`
- Link URL: `https://t.me/{bot_username}?start=connect_{display_name}`
- Link format is HTML: `<a href="{link}">Open {display_name}</a>`

### Root Cause Analysis

**THE ISSUE**: Line 530 in `bot.rs` uses a **generic link text** for ALL notifications:

```rust
message.push_str(&format!("\n\n👉 <a href=\"{}\">Open {}</a>", link, display_name));
```

This applies to:
1. **Session ready** notifications: `"Session \"{}\" is ready"` → link says "Open {session}"
2. **Session resumed** notifications: `"Session \"{}\" resumed work"` → link says "Open {session}"
3. **Sessions waiting** notifications: Lists multiple sessions → each link says "Open {session}"

**Why This Is Confusing**:
- "Session resumed work" implies the session is already running
- "Open {session}" suggests you're opening something fresh/new
- User expects: "Continue {session}", "Resume {session}", or "Reconnect to {session}"

### Where Links Are NOT an Issue

The GUI (`crates/commander-gui/`) does NOT have this problem because:
1. GUI messages are plain text (no deep links)
2. GUI uses `/connect` command links, which are clear
3. GUI shows connection status visually with Activity indicator

**Example from GUI**:
```typescript
// SessionList.svelte line 37
content: `Connected to session: ${getDisplayName(name)}`,
```

### Other Deep Link Usage

**Truncated response links** (bot.rs:427-437):
```rust
// If truncated, append deep link to message text
if response.contains("more characters)_") || response.contains("more lines)_") {
    if let Some((name, _)) = state.get_session_info(chat_id).await {
        // Generate deep link for opening full session
        let bot_username = match bot.get_me().await {
            Ok(me) => me.username().to_string(),
            Err(_) => "commander".to_string(),
        };
        let link = format!("https://t.me/{}?start=connect_{}", bot_username, name);
        response.push_str(&format!("\n\n👉 <a href=\"{}\">Open full session</a>", link));
    }
}
```

**Key difference**: Uses context-aware text "Open full session" for truncated responses.

---

## Solution Options

### Option 1: Context-Aware Link Text (Recommended)

Change `bot.rs:520-531` to use context-aware link text based on notification type:

```rust
// Build notification message with deep link if session is specified
let mut message = notification.message.clone();
if let Some(session) = &notification.session {
    let display_name = session.strip_prefix("commander-").unwrap_or(session);

    // Generate deep link for connecting to this session
    let bot_username = match bot.get_me().await {
        Ok(me) => me.username().to_string(),
        Err(_) => "commander".to_string(),
    };
    let link = format!("https://t.me/{}?start=connect_{}", bot_username, display_name);

    // Choose link text based on notification message
    let link_text = if message.contains("resumed work") {
        format!("Continue {}", display_name)
    } else if message.contains("is ready") {
        format!("Open {}", display_name)
    } else if message.contains("waiting for your input") {
        format!("Connect to {}", display_name)
    } else {
        format!("Open {}", display_name)
    };

    message.push_str(&format!("\n\n👉 <a href=\"{}\">{}r</a>", link, link_text));
}
```

**Pros**:
- Minimal code change
- Preserves existing behavior for "ready" notifications
- Clear semantic distinction for "resumed" vs "ready"

**Cons**:
- String matching is fragile (if notification text changes)

### Option 2: Add Notification Type Field

Add a `notification_type` field to `Notification` struct:

```rust
pub struct Notification {
    pub id: u64,
    pub message: String,
    pub session: Option<String>,
    pub notification_type: NotificationType,  // NEW
    pub timestamp: DateTime<Utc>,
}

pub enum NotificationType {
    SessionReady,
    SessionResumed,
    SessionsWaiting,
    Custom,
}
```

Then use it in link generation:

```rust
let link_text = match notification.notification_type {
    NotificationType::SessionResumed => format!("Continue {}", display_name),
    NotificationType::SessionReady => format!("Open {}", display_name),
    NotificationType::SessionsWaiting => format!("Connect to {}", display_name),
    NotificationType::Custom => format!("Open {}", display_name),
};
```

**Pros**:
- Robust, not dependent on string content
- Easier to extend with new notification types
- Type-safe

**Cons**:
- Requires schema migration (notification store)
- More intrusive change

### Option 3: Different Notification Functions with Link Text

Add an optional `link_text` parameter to `push_notification`:

```rust
pub fn push_notification(
    message: String,
    session: Option<String>,
    link_text: Option<String>,  // NEW
) -> Result<(), std::io::Error> {
    // Store link_text in notification
}

pub fn notify_session_resumed(session_name: &str) -> Result<(), std::io::Error> {
    let display_name = session_name.strip_prefix("commander-").unwrap_or(session_name);
    let message = format!("Session \"{}\" resumed work", display_name);

    push_notification(
        message,
        Some(session_name.to_string()),
        Some(format!("Continue {}", display_name))  // NEW
    )
}
```

**Pros**:
- Each notification explicitly controls its link text
- No string matching needed
- Backward compatible (link_text = None uses default)

**Cons**:
- Requires notification store schema change
- More parameters to track

---

## Recommendation

**Implement Option 1 (Context-Aware Link Text)** as the quickest fix:

1. **File to modify**: `crates/commander-telegram/src/bot.rs`
2. **Lines to change**: 520-531
3. **Change type**: Replace generic link text with context-aware selection

**Why Option 1**:
- Solves the user's immediate complaint
- No schema changes required
- Can be implemented and tested in <30 minutes
- Low risk of breaking existing functionality

**Future improvement**:
- If more notification types are added, consider Option 2 for type safety
- Option 3 is a good middle ground if link text needs per-call customization

---

## Verification Steps

After implementing fix:

1. **Trigger resume notification**:
   ```bash
   # In AI Commander TUI
   # 1. Create a session
   # 2. Let it go idle
   # 3. Send it a message to resume work
   ```

2. **Check Telegram notification**:
   - Message should say: `Session "session-name" resumed work`
   - Link should say: `Continue session-name` (NOT "Open session-name")
   - Clicking link should execute `/start connect_session-name`

3. **Verify other notifications still work**:
   - "Session ready" → should say "Open {session}"
   - "Sessions waiting" → should say "Connect to {session}"

---

## Additional Context

### GUI Behavior (For Comparison)

The GUI does NOT have this issue because:

1. **Message rendering** (`ChatView.svelte:168`):
   ```svelte
   <div class="content">{message.content}</div>
   ```
   - Plain text rendering, no HTML parsing
   - No deep links (uses `/connect` command syntax)

2. **Connection messages** (`SessionList.svelte:36-39`):
   ```typescript
   addMessageToSession(name, {
       direction: 'system',
       content: `Connected to session: ${getDisplayName(name)}`,
       timestamp: new Date(),
   });
   ```
   - Clear, unambiguous language
   - No links to click (connection is implicit)

3. **Session actions** (`ChatView.svelte:134-158`):
   - Status, Stop, Disconnect buttons
   - No ambiguity about "connect" vs "resume"

### URL Format

Deep link format: `https://t.me/{bot_username}?start=connect_{session_name}`

**How it works**:
1. User clicks link in Telegram
2. Opens bot chat with `/start connect_{session_name}` payload
3. Bot parses payload and executes connect command
4. User is connected to session

**Note**: The URL always uses `connect_` regardless of whether it's a fresh connection or resume. The confusion is ONLY in the link text, not the URL itself.

---

## Files Referenced

| File | Lines | Purpose |
|------|-------|---------|
| `crates/commander-telegram/src/notifications.rs` | 204-209 | Generates resume notification message |
| `crates/commander-telegram/src/bot.rs` | 520-531 | Adds deep link to notifications |
| `crates/commander-telegram/src/bot.rs` | 427-437 | Adds deep link to truncated responses |
| `crates/ai-commander/src/tui/sessions.rs` | 224 | Calls `notify_session_resumed` |
| `crates/commander-gui/ui/src/lib/components/ChatView.svelte` | 168 | GUI message rendering (no HTML) |
| `crates/commander-gui/ui/src/lib/components/SessionList.svelte` | 36-39 | GUI connection messages |

---

## Next Steps

1. Implement Option 1 (context-aware link text)
2. Test with real resume notifications
3. Verify other notification types still work correctly
4. Consider Option 2 if more notification types are added in future
