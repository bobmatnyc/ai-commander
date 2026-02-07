# Research: Timer Notification Parsing for Orchestration

**Date:** 2026-02-07
**Issue:** ISS-0022 / GitHub #35
**Status:** Research Complete
**Author:** Research Agent

## Executive Summary

Timer notifications are being passed through literally to agents instead of being parsed and interpreted for orchestration. This research documents the complete notification flow, identifies the gap, and proposes an implementation approach.

**Key Finding:** Notifications are generated and broadcast correctly, but there is no parsing/interpretation layer to extract structured data and trigger orchestration actions. The system treats notifications as opaque strings rather than actionable events.

---

## 1. Source of Timer Notifications

### 1.1 Generation Points

Timer notifications originate from **two monitoring loops** in the TUI:

**Fast Check (5-second interval)** - `/crates/ai-commander/src/tui/sessions.rs:16-103`
- `check_session_status()` monitors connected sessions
- Detects transitions from "working" to "ready" state
- Generates `[inbox]` notifications for individual sessions

**Slow Scan (5-minute interval)** - `/crates/ai-commander/src/tui/sessions.rs:105-209`
- `scan_all_sessions()` scans ALL tmux sessions
- Compares current state to last scan
- Generates `[clock]` notifications for newly waiting sessions
- Generates `[play]` notifications for resumed sessions

### 1.2 Notification Formats

```
[inbox] @{session_name} is ready
[inbox] @{session_name} is ready: {preview}
[clock] {N} new session(s) waiting for input:
   @{session_name}
   @{session_name} - {preview}
[play] @{session_name} resumed work
[timer] {N} new session(s) waiting for input:
   @{session_name} - {preview_with_ansi_codes}
```

The `[timer]` format is used specifically in the Telegram broadcast path (`/crates/commander-telegram/src/notifications.rs:199`).

### 1.3 Preview Content

The preview is extracted by `extract_ready_preview()` in `/crates/ai-commander/src/tui/helpers.rs:4-37`:
- Takes the last 50 lines of tmux output
- Filters out UI noise (box drawing, hints, separators)
- Returns the last meaningful line before the prompt

**Example preview content:**
```
masa@Masas-Studio:/Users/masa/Projects/izzie2 (main*?) [model_info][context_usage%]
```

This preview contains ANSI escape codes like `\033[90m` that are not stripped.

---

## 2. Message Flow Diagram

```
                                   +------------------+
                                   |  tmux sessions   |
                                   |  (Claude Code)   |
                                   +--------+---------+
                                            |
                                            | capture_output()
                                            v
+----------------------+           +-------------------+
|  check_session_status | ------> | is_claude_ready() |
|  (5 sec interval)     |          +--------+----------+
+----------------------+                    |
         |                                  | state change detected
         |                                  v
         |                        +-----------------+
         |                        | extract_ready_  |
         |                        | preview()       |
         |                        +--------+--------+
         |                                 |
         v                                 v
+----------------------+          +------------------+
| scan_all_sessions   |           | Notification     |
| (5 min interval)    |           | generated        |
+----------+-----------+          +--------+---------+
           |                               |
           +---------------+---------------+
                           |
             +-------------+-------------+
             |                           |
             v                           v
    +------------------+       +--------------------+
    | TUI Message      |       | commander_telegram |
    | (Message::system)|       | ::notify_*()       |
    +------------------+       +---------+----------+
             |                           |
             v                           v
    +------------------+       +--------------------+
    | TUI Display      |       | notifications.json |
    | (scroll, render) |       | (file queue)       |
    +------------------+       +---------+----------+
                                         |
                                         v
                               +--------------------+
                               | poll_notifications |
                               | _loop() (2 sec)    |
                               +---------+----------+
                                         |
                                         v
                               +--------------------+
                               | Telegram bot sends |
                               | raw message string |
                               +--------------------+
                                         |
                                         | (literal pass-through)
                                         v
                               +--------------------+
                               | User/Agent sees    |
                               | raw text + ANSI    |
                               +--------------------+
```

---

## 3. Current Flow: Timer to Display

### 3.1 TUI Path

1. **Detection:** `check_session_status()` or `scan_all_sessions()` detects state change
2. **Message Creation:** `Message::system(formatted_string)` creates message
3. **Storage:** Message pushed to `self.messages` vector
4. **Display:** Rendered in TUI message area with scroll

**Location:** `/crates/ai-commander/src/tui/sessions.rs`

### 3.2 Telegram Path

1. **Detection:** Same as TUI (runs in TUI process)
2. **Broadcast Call:** `commander_telegram::notify_session_ready()` or `notify_sessions_waiting()`
3. **Queue Storage:** `push_notification()` writes to `~/.ai-commander/state/notifications.json`
4. **Polling:** Telegram bot's `poll_notifications_loop()` reads every 2 seconds
5. **Delivery:** `bot.send_message()` sends raw string to all authorized chats

**Key Files:**
- `/crates/commander-telegram/src/notifications.rs` - Notification queue management
- `/crates/commander-telegram/src/bot.rs:269-313` - Polling loop

### 3.3 The Gap: No Parsing Layer

Currently, notifications flow through as opaque strings. There is **no parsing or interpretation** at any point:

- TUI displays raw strings
- Telegram sends raw strings
- Agent receives raw strings (if connected to a session)
- No structured data extraction
- No action triggers based on notification content

---

## 4. Where Should Parsing/Interpretation Happen?

### 4.1 Recommended Architecture

```
+--------------------+
| Notification       |
| Generation         |
+--------+-----------+
         |
         v
+--------------------+
| ParsedNotification | <-- NEW: Structured data type
| {                  |
|   type: NotifyType |
|   sessions: [      |
|     { name, path,  |
|       branch,      |
|       model,       |
|       context% }   |
|   ]                |
| }                  |
+--------+-----------+
         |
    +----+----+
    |         |
    v         v
+-------+  +----------+
| Store |  | Action   | <-- NEW: Orchestration trigger
| (JSON)|  | Handler  |
+-------+  +----------+
```

### 4.2 Proposed Parsing Location

**Option A: At Generation Time (Recommended)**
- Parse in `notify_sessions_waiting()` and `notify_session_ready()`
- Store structured `ParsedNotification` in JSON queue
- Display can format from structured data
- Agents receive structured data

**Option B: At Consumption Time**
- Parse when reading from notification queue
- Each consumer (TUI, Telegram, Agent) parses independently
- More flexible but duplicates logic

**Option C: Dual Path**
- Keep human-readable string for display
- Add structured data alongside for orchestration

### 4.3 Recommendation

**Option C (Dual Path)** provides best compatibility:

```rust
pub struct Notification {
    pub id: String,
    pub message: String,              // Human-readable (existing)
    pub session: Option<String>,      // Existing
    pub created_at: u64,              // Existing
    pub read_by: HashSet<String>,     // Existing
    // NEW: Structured data
    pub parsed: Option<ParsedNotification>,
}

pub struct ParsedNotification {
    pub notification_type: NotificationType,
    pub sessions: Vec<SessionStatus>,
}

pub enum NotificationType {
    SessionReady,      // [inbox]
    SessionsWaiting,   // [clock], [timer]
    SessionResumed,    // [play]
}

pub struct SessionStatus {
    pub name: String,              // e.g., "izzie-33"
    pub full_name: String,         // e.g., "commander-izzie-33"
    pub user_host: Option<String>, // e.g., "masa@Masas-Studio"
    pub path: Option<String>,      // e.g., "/Users/masa/Projects/izzie2"
    pub branch: Option<String>,    // e.g., "main"
    pub git_status: Option<String>,// e.g., "*?"
    pub model: Option<String>,     // e.g., "us.anthropic.claude-opus-4-5-20251101-v1:0"
    pub context_usage: Option<u8>, // e.g., 70
}
```

---

## 5. Data Extractable from Notifications

### 5.1 Session Identifier

**Source:** `@{session_name}` pattern
**Regex:** `@([a-zA-Z0-9_-]+)`
**Example:** `@izzie-33` -> `izzie-33`

### 5.2 User/Host Info

**Source:** Preview line
**Pattern:** `{user}@{host}:{path}`
**Regex:** `([^@]+)@([^:]+):([^\s]+)`
**Example:** `masa@Masas-Studio:/Users/masa/Projects/izzie2` ->
- user: `masa`
- host: `Masas-Studio`
- path: `/Users/masa/Projects/izzie2`

### 5.3 Git Branch and Status

**Source:** Preview line in parentheses
**Pattern:** `({branch}{status})`
**Regex:** `\(([a-zA-Z0-9_/-]+)([*?!+-]*)\)`
**Example:** `(main*?)` ->
- branch: `main`
- status: `*?` (modified + untracked)

### 5.4 Model Information

**Source:** Preview line in brackets with ANSI codes
**Pattern:** `[{model}|{name}|{usage}%]`
**Pre-process:** Strip ANSI codes `\033\[[0-9;]*m`
**Regex:** `\[([^|]+)\|([^|]+)\|([0-9]+)%\]`
**Example:** `[us.anthropic.claude-opus-4-5-20251101-v1:0|Claude MPM|70%]` ->
- model: `us.anthropic.claude-opus-4-5-20251101-v1:0`
- name: `Claude MPM`
- context_usage: `70`

### 5.5 ANSI Escape Code Handling

**Codes Present:**
- `\033[90m` - Gray text (dim)
- `\033[0m` - Reset

**Stripping Regex:** `\x1B\[[0-9;]*[a-zA-Z]`

---

## 6. Implementation Approach

### 6.1 Phase 1: Add Parsing Module

Create `/crates/commander-core/src/notification_parser.rs`:

```rust
use regex::Regex;
use once_cell::sync::Lazy;

static ANSI_REGEX: Lazy<Regex> = Lazy::new(||
    Regex::new(r"\x1B\[[0-9;]*[a-zA-Z]").unwrap()
);
static SESSION_REGEX: Lazy<Regex> = Lazy::new(||
    Regex::new(r"@([a-zA-Z0-9_-]+)").unwrap()
);
static PATH_REGEX: Lazy<Regex> = Lazy::new(||
    Regex::new(r"([^@\s]+)@([^:]+):([^\s(]+)").unwrap()
);
static BRANCH_REGEX: Lazy<Regex> = Lazy::new(||
    Regex::new(r"\(([a-zA-Z0-9_/-]+)([*?!+-]*)\)").unwrap()
);
static MODEL_REGEX: Lazy<Regex> = Lazy::new(||
    Regex::new(r"\[([^|\]]+)\|([^|\]]+)\|([0-9]+)%\]").unwrap()
);

pub fn strip_ansi(s: &str) -> String {
    ANSI_REGEX.replace_all(s, "").to_string()
}

pub fn parse_notification(message: &str) -> ParsedNotification {
    let clean = strip_ansi(message);
    // ... parsing logic
}
```

### 6.2 Phase 2: Update Notification Struct

Modify `/crates/commander-telegram/src/notifications.rs`:

```rust
pub struct Notification {
    // ... existing fields
    #[serde(default)]
    pub parsed: Option<ParsedNotification>,
}

pub fn notify_sessions_waiting(sessions: &[(String, String)]) -> Result<()> {
    // Build human-readable message
    let message = format!("[timer] {} new session(s)...", sessions.len());

    // Build parsed data
    let parsed = ParsedNotification {
        notification_type: NotificationType::SessionsWaiting,
        sessions: sessions.iter().map(|(name, preview)| {
            parse_session_preview(name, preview)
        }).collect(),
    };

    push_notification_with_parsed(message, None, Some(parsed))
}
```

### 6.3 Phase 3: Add Orchestration Handler

Create orchestration action interface:

```rust
pub trait OrchestrationHandler {
    fn on_sessions_waiting(&mut self, sessions: &[SessionStatus]);
    fn on_session_ready(&mut self, session: &SessionStatus);
    fn on_session_resumed(&mut self, session: &SessionStatus);
}
```

### 6.4 Phase 4: Integrate with Agent

In the agent's message handling:

```rust
fn process_notification(&mut self, notification: &Notification) {
    if let Some(parsed) = &notification.parsed {
        match parsed.notification_type {
            NotificationType::SessionsWaiting => {
                // Auto-switch to highest priority waiting session
                // Or present choice to user
            }
            NotificationType::SessionReady => {
                // Mark session as available
            }
            NotificationType::SessionResumed => {
                // Update session state
            }
        }
    }
}
```

---

## 7. Files to Modify

| File | Changes |
|------|---------|
| `commander-core/src/lib.rs` | Add `notification_parser` module export |
| `commander-core/src/notification_parser.rs` | **NEW**: Parsing logic |
| `commander-telegram/src/notifications.rs` | Add `parsed` field, update generation |
| `commander-telegram/src/lib.rs` | Export new types |
| `ai-commander/src/tui/helpers.rs` | Reuse parsing for `extract_ready_preview` |
| `commander-agent/src/session_agent/mod.rs` | Add orchestration handler |

---

## 8. Testing Strategy

### 8.1 Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        let input = "text \x1B[90mgrayed\x1B[0m normal";
        assert_eq!(strip_ansi(input), "text grayed normal");
    }

    #[test]
    fn test_parse_session_name() {
        let notification = "[timer] 1 new session(s) waiting:\n   @izzie-33 - preview";
        let parsed = parse_notification(notification);
        assert_eq!(parsed.sessions[0].name, "izzie-33");
    }

    #[test]
    fn test_parse_full_preview() {
        let preview = "masa@Masas-Studio:/Users/masa/Projects/izzie2 (main*?) \x1B[90m[us.anthropic.claude-opus-4-5-20251101-v1:0|Claude MPM|70%]\x1B[0m";
        let session = parse_session_preview("izzie-33", preview);

        assert_eq!(session.user_host, Some("masa@Masas-Studio".into()));
        assert_eq!(session.path, Some("/Users/masa/Projects/izzie2".into()));
        assert_eq!(session.branch, Some("main".into()));
        assert_eq!(session.git_status, Some("*?".into()));
        assert_eq!(session.model, Some("us.anthropic.claude-opus-4-5-20251101-v1:0".into()));
        assert_eq!(session.context_usage, Some(70));
    }
}
```

### 8.2 Integration Tests

1. Generate notification with known format
2. Verify parsed data matches expected
3. Verify backward compatibility (old notifications without `parsed` field)
4. Test orchestration handler receives correct data

---

## 9. Migration Considerations

### 9.1 Backward Compatibility

- `parsed` field is `Option<>` so old notifications work
- Old code reading notifications ignores new field (serde skip_unknown)
- Display continues using `message` string

### 9.2 Forward Compatibility

- New code checks for `parsed` field first
- Falls back to regex parsing of `message` if `parsed` missing

---

## 10. Related Issues

- **ANSI Code Stripping:** Should be consistent across all paths
- **Preview Extraction:** `extract_ready_preview()` duplicates some parsing logic
- **Notification Queue:** Consider adding TTL-based cleanup for `parsed` data

---

## 11. Conclusion

The timer notification system is functioning correctly for generation and delivery, but lacks a parsing/interpretation layer. The recommended approach is:

1. Add `notification_parser` module to `commander-core`
2. Extend `Notification` struct with optional `parsed` field
3. Update notification generators to populate parsed data
4. Add orchestration handler interface for agents

This enables agents to:
- Understand session status semantically
- Extract project path, branch, context usage
- Take automated actions based on notification type
- Display clean, formatted information without ANSI codes

**Estimated Implementation Time:** 4-6 hours

---

## Appendix A: Full Regex Pattern Reference

```
# Strip ANSI escape codes
\x1B\[[0-9;]*[a-zA-Z]

# Extract session name from @mention
@([a-zA-Z0-9_-]+)

# Parse user@host:path
([^@\s]+)@([^:]+):([^\s(]+)

# Parse branch and git status
\(([a-zA-Z0-9_/-]+)([*?!+-]*)\)

# Parse model info block
\[([^|\]]+)\|([^|\]]+)\|([0-9]+)%\]

# Notification type detection
^\[inbox\]  -> SessionReady
^\[clock\]  -> SessionsWaiting (TUI)
^\[timer\]  -> SessionsWaiting (Telegram)
^\[play\]   -> SessionResumed
```

## Appendix B: Example Notification Payloads

### Input (Raw)
```
[timer] 1 new session(s) waiting for input:
   @izzie-33 - masa@Masas-Studio:/Users/masa/Projects/izzie2 (main*?) \033[90m[us.anthropic.claude-opus-4-5-20251101-v1:0|Claude MPM|70%]\033[0m
```

### Output (Parsed)
```json
{
  "notification_type": "SessionsWaiting",
  "sessions": [
    {
      "name": "izzie-33",
      "full_name": "commander-izzie-33",
      "user_host": "masa@Masas-Studio",
      "path": "/Users/masa/Projects/izzie2",
      "branch": "main",
      "git_status": "*?",
      "model": "us.anthropic.claude-opus-4-5-20251101-v1:0",
      "context_usage": 70
    }
  ]
}
```
