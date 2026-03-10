# Telegram Bot UI/Messaging Capabilities Analysis

**Date:** 2026-03-07
**Crate:** `crates/commander-telegram`
**Research scope:** UI capabilities, messaging patterns, pipeline architecture

---

## Key Files and Responsibilities

| File | Responsibility |
|------|---------------|
| `src/bot.rs` | Bot initialization, dispatcher setup, polling loops, response delivery |
| `src/handlers.rs` | Command handlers (/start, /help, /pair, /connect, /status, /list, /topic, etc.), message routing, callback handling |
| `src/state.rs` | Shared state (sessions, auth, group configs), output polling logic, summarization dispatch |
| `src/session.rs` | UserSession struct (response buffer, progress tracking, incremental summary thresholds) |
| `src/pairing.rs` | 6-character pairing code generation/consumption (5-min TTL, file-based) |
| `src/notifications.rs` | Cross-channel notification queue (poll every 2s, broadcast to all authorized chats) |
| `src/session_log.rs` | JSONL session logging for evals |
| `src/ipc_client.rs` | IPC to commander-daemon socket (tmux-only fallback when daemon absent) |
| `crates/commander-core/src/options.rs` | Option detection (A/B/C, 1/2/3, y/n patterns) for inline keyboard generation |
| `crates/commander-core/src/output_filter.rs` | `clean_response()` - strips UI noise (spinners, MCP tool output, progress indicators) |
| `crates/commander-core/src/summarizer.rs` | OpenRouter-based summarization with fallback truncation |

---

## 1. Current API Methods in Use

### Message Sending
- `bot.send_message(chat_id, text).parse_mode(ParseMode::Html)` - primary response delivery
- `bot.send_message(chat_id, text)` - plain text (no parse_mode) for error/status messages inconsistently
- `bot.edit_message_text(chat_id, msg_id, text)` - progress message updates (in-place editing)
- `bot.delete_message(chat_id, msg_id)` - cleanup of progress/summary messages before final response

### Typing Indicators
- `bot.send_chat_action(chat_id, ChatAction::Typing)` - sent every 500ms polling cycle for all waiting sessions
- For forum topics: `.message_thread_id(tid)` chained on `send_chat_action`

### Reply Threading
- `bot.send_message(...).reply_parameters(ReplyParameters::new(msg_id))` - final response replies to original user message

### Inline Keyboards
- `InlineKeyboardMarkup::new(buttons)` - generated when `OptionDetector` finds options in response
- `InlineKeyboardButton::callback(label, "option:{key}")` - each option is one row
- `bot.answer_callback_query(q_id)` - always acknowledged in callback handler

### Bot Info
- `bot.get_me()` - fetched to generate deep links (called per-notification, not cached)

### Chat Info
- `bot.get_chat(chat_id)` - called in `/groupmode` to check supergroup/forum status

---

## 2. Message Formatting

### Parse Mode
**HTML mode is used for final responses and notifications.** All `send_message` calls for AI responses use `.parse_mode(ParseMode::Html)`.

There is **no Markdown or MarkdownV2** usage anywhere in the codebase.

### HTML Entities in Use
- `<b>text</b>` - bold (project names, section headers)
- `<code>text</code>` - inline code (paths, commands)
- `<pre>text</pre>` - preformatted block (screen content in /status when summarization unavailable)
- `<a href="url">text</a>` - hyperlinks (deep links to sessions, "Open full session" links)

### HTML Escaping
A custom `html_escape()` function is used:
```rust
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
}
```
This is applied to user-controlled strings (project names, paths, queries) but **not consistently applied to all AI-generated content** before sending - AI responses go through `clean_response()`/summarization but not through html_escape before HTML-mode sending. This is a potential formatting breakage risk.

---

## 3. Typing Indicators / Chat Actions

The typing indicator is sent **every 500ms** in the output polling loop (`poll_output_loop`) for all sessions in `is_waiting` state. The indicator is sent proactively to indicate ongoing processing.

For **forum topics** (group mode), the indicator is sent with `.message_thread_id(tid)`.

On initial user message receipt (`handle_message`), a single `send_chat_action(Typing)` is sent before forwarding to tmux.

**There is no spinner or animated progress beyond the typing indicator and text updates.**

---

## 4. Long Response Handling

### When Summarization is Available (OPENROUTER_API_KEY set)
- `summarize_with_fallback(query, raw_response)` is called via OpenRouter API
- The summary replaces the full tmux output
- If the API call fails, falls back to truncation

### When Summarization is Unavailable
`fallback_truncate()` applies hard limits:
- `FALLBACK_MAX_LINES = 10` lines
- `FALLBACK_MAX_CHARS = 500` characters
- Appended suffix: `"...\n\n_({N} more lines)_"` or `"...\n\n_({N} more characters)_"`

The bot detects truncation in the final response by checking for `"more characters)_"` or `"more lines)_"` substrings and appends a deep link:
```
"...\n\n_({N} more lines)_\n\n👉 <a href=\"...\">Open full session</a>"
```

**There is no message splitting.** Responses are sent as a single message. If the summarized response itself exceeds Telegram's 4096-character limit, the `send_message` call will fail silently (logged as warning).

### Incremental Summaries
During long operations, the bot sends incremental summaries every 50 lines of output:
- At lines 50, 100, 150, etc., `summarize_incremental()` is called
- The summary is sent/updated as a separate message (tracked by `summary_messages` HashMap)
- This message is deleted when the final response is ready

### Progress Messages
Every 5 new lines of output, a progress message is sent/updated:
- Format: `"📥 Receiving...{N} lines captured"`
- Sent as an in-place edit (`edit_message_text`) if a progress message already exists
- Deleted when final response is ready

---

## 5. Inline Keyboards and Reply Keyboards

### Inline Keyboards (used)
The `OptionDetector` in `commander-core` scans AI responses for:
- Letter options: `A) text`, `A. text` (sequential: A, B, C...)
- Number options: `1) text`, `1. text` (sequential: 1, 2, 3...)
- Yes/No: `(y/n)`, `(Y/N)`, `(yes/no)`

When detected, an inline keyboard is added to the final response. Each option is a single-button row. Callback data format: `"option:{key}"` (e.g., `"option:A"`, `"option:y"`).

The callback handler (`handle_option_selection`) forwards the key as text to the connected tmux session.

### Connect Buttons (in /list)
The `/list` command generates inline buttons with `"connect:{session_name}"` callback data.

### Reply Keyboards
**Not used.** No `ReplyKeyboardMarkup` anywhere in the codebase.

---

## 6. Error and Status Message Display

### Authorization Errors (consistent pattern)
```
⛔ Not authorized. Use <code>/pair &lt;code&gt;</code> first.
```

### Operation Errors
Plain text with `❌` prefix, no parse_mode:
```
❌ Failed to connect to 'session': {error}
❌ Error: {error_message}
```

### Status Messages
HTML-formatted with emoji prefixes:
- `✅` - success
- `🔄` - in progress
- `💤` - idle
- `📊` - status header
- `🛑` - stopped
- `⚠️` - warning

### /status Command Output
Structured HTML with sections:
```
📊 <b>Status</b>

✅ Connection: Connected
📁 Project: {name}
📍 Path: <code>{path}</code>
🔧 Adapter: {adapter}

🔄 Activity: Processing...
📝 Query: "..."
```

When summarization is available: LLM interprets the tmux screen content.
When unavailable: raw `<pre>screen</pre>` block shown.

### Rebuild Notifications
Sent to all authorized chats on bot restart if not first start:
```
🔄 Bot rebuilt and restarted.
✅ Successfully restored {N} session(s).
```

---

## 7. Feature Detection / Configuration Code

### Summarization Availability
`is_summarization_available()` in `commander-core` checks for `OPENROUTER_API_KEY` (or equivalent). This controls:
- Whether AI summarization is used vs. truncation
- Whether `/status` shows LLM-interpreted screen vs. raw `<pre>` block
- Welcome message: `"✅ enabled"` or `"⚠️ disabled (set OPENROUTER_API_KEY)"`

### Adapter Type Detection
`adapter_display_name(tool_id)` maps adapter IDs to display strings:
```rust
"claude-code" | "cc" => "Claude Code"
"mpm"         => "Claude MPM"
"aider"       => "Aider"
"unknown"     => "this session"
```

### Agents Feature Flag
The `agents` Cargo feature (enabled by default) gates `commander-orchestrator` integration. Checked with `#[cfg(feature = "agents")]`.

### tmux Availability
`state.has_tmux()` - checked in welcome message and guards session operations.

### Daemon Availability
`DaemonClient::default_path().is_daemon_running()` checked at startup. If daemon socket exists, IPC is used; otherwise falls back to direct tmux orchestration.

---

## 8. Message/Response Pipeline

```
User sends message in Telegram
    |
    v
Teloxide dispatcher -> handle_message()
    |
    +-- Authorization check (is_authorized)
    +-- Thread ID check -> handle_topic_message() if forum topic
    +-- @alias prefix routing
    +-- Session check (has_session)
    |
    v
send_chat_action(Typing) [once]
    |
    v
state.send_message(chat_id, text, msg_id)
    -> Writes text to tmux session (or daemon IPC)
    -> Sets session.is_waiting = true
    -> Records session.pending_message_id, pending_query, send_time
    |
    v
[500ms polling loop in background task: poll_output_loop]
    |
    +-- For each waiting session:
    |   send_chat_action(Typing) [every poll cycle]
    |
    +-- state.poll_output(chat_id)
    |   -> Capture tmux output (find_new_lines)
    |   -> session.add_response_lines(new_lines)
    |   -> Check idle (1s threshold for claude-code, 2s for mpm)
    |   -> Check is_claude_ready() / is_mpm_ready() prompt detection
    |
    +-- PollResult::Progress(msg)     -> edit_message_text (every 5 lines)
    +-- PollResult::IncrementalSummary(s) -> send/edit summary message (every 50 lines)
    +-- PollResult::Summarizing       -> edit progress message with "Summarizing..."
    +-- PollResult::NoOutput          -> continue polling
    +-- PollResult::Complete(response, msg_id, thread_id):
        |
        +-- delete progress message
        +-- delete summary message
        +-- detect_options(response) -> optional inline keyboard
        +-- bot.send_message(chat_id, response)
              .parse_mode(Html)
              [.reply_parameters(original_msg_id)]
              [.message_thread_id(tid)]
              [.reply_markup(inline_keyboard)]
```

---

## 9. Authorization and Session Management

### Authorization
- Chat IDs stored in `~/.ai-commander/state/authorized_chats.json`
- Loaded at startup into `HashSet<i64>` in `TelegramState`
- Authorization granted via `/pair CODE` command (consumes one-time 6-char code)
- No per-command granularity - a chat is either authorized or not
- Authorization is for the entire Commander instance (not per-project)

### Session Management
- `HashMap<i64, UserSession>` keyed by chat_id (for DMs) or `(chat_id << 32 | thread_id)` for forum topics
- Persisted to `~/.ai-commander/state/telegram_sessions.json` (24h validity)
- Restored on bot restart via `state.load_sessions()`
- Sessions survive bot rebuilds/restarts

### Session Fields Relevant to UI
- `adapter_type`: "claude-code", "mpm", "unknown" (affects ready-state detection)
- `thread_id`: Forum topic (ThreadId) for group mode
- `pending_message_id`: Used for reply threading on final response
- `response_buffer`: Accumulates tmux output lines between polls
- `is_waiting` / `is_summarizing`: State machine flags

---

## 10. Configuration

### Environment Variables
| Variable | Usage |
|----------|-------|
| `TELEGRAM_BOT_TOKEN` | Required; bot authentication |
| `TELEGRAM_WEBHOOK_PORT` | Optional; webhook port (default 8443, currently unused - falls back to polling) |
| `OPENROUTER_API_KEY` | Optional; enables LLM summarization |

### HTTP Client Configuration
Custom reqwest client with explicit timeouts:
- Read timeout: 120s (for long-polling getUpdates)
- Connect timeout: 30s
- Pool idle timeout: 90s
- Max idle connections per host: 2

### Polling Intervals
- Output polling: 500ms (`POLL_INTERVAL_MS`)
- Notification polling: 2000ms (`NOTIFICATION_POLL_INTERVAL_MS`)

---

## 11. Current Gaps and Limitations

### Critical Gaps

1. **No message splitting for responses exceeding 4096 characters.** If a summarized response is still too long, the `send_message` API call fails silently (only a warn log). The user sees nothing.

2. **HTML escaping not applied to AI response content.** `clean_response()` strips UI noise but doesn't HTML-escape the content. If an AI response contains `<`, `>`, or `&`, the HTML parse_mode may fail or mangle the message. Currently the error is logged as a warning.

3. **`bot.get_me()` called per-notification.** Every notification dispatch calls `get_me()` to get the bot username for deep links. This is inefficient (one API call per notification per authorized chat).

4. **Typing indicator persists even during API calls.** The polling loop sends `Typing` every 500ms including during OpenRouter API calls for summarization, which can take several seconds. There's no distinction between "waiting for tmux output" and "calling OpenRouter API."

5. **No `disable_notification` parameter used.** All messages are sent with default notification behavior (sound/vibration). Long-running sessions that emit many notifications could be noisy for users.

### UI Limitations (Basic/Limited Feel)

6. **Progress messages are text-only** (`"📥 Receiving...{N} lines captured"`). No visual progress bar, no time estimate.

7. **No message editing for the final response** - progress message is deleted and a new message is sent. This causes a visual "flash" and makes it impossible to edit-in-place with a loading state.

8. **Option detection is purely syntactic** - relies on regex patterns (sequential A/B/C or 1/2/3). Does not handle options in other formats (e.g., bullet lists, table format).

9. **Inline keyboards are not persistent** - after the bot restarts, buttons on old messages have stale callback handling since the message context is lost. The callback handler does re-check state.

10. **No support for rich media** - no `sendPhoto`, `sendDocument`, `sendAudio`, or `sendVideo`. Everything is text. No way to send file diffs, images, or attachments.

11. **No `disable_web_page_preview`** on messages containing URLs. Deep links in notifications will generate large link previews.

12. **`/help` command sends plain text** (no parse_mode). The `Command::descriptions().to_string()` output is not HTML-formatted.

---

## 12. Best Insertion Points for Premium vs Standard Branching

The cleanest insertion points for feature detection logic:

### A. `state.rs` - `poll_output()` completion path (line ~1083-1090)
```rust
let response = if needs_summarization {
    summarize_with_fallback(&query, &raw_response).await
} else {
    clean_response(&raw_response)
};
```
**Insert here:** A `tier_features(chat_id)` check before/after summarization to gate premium formatting (e.g., richer summaries, longer limits).

### B. `bot.rs` - `poll_output_loop()` `Complete` branch (line ~413-469)
```rust
// Check for options in response
let detected_options = OptionDetector::detect_options(&response);

// Send the final response...
let mut req = bot.send_message(chat_id, &response)
    .parse_mode(teloxide::types::ParseMode::Html);
```
**Insert here:** Premium users could get richer formatting, message splitting, or different parse_mode.

### C. `handlers.rs` - `handle_message()` (line ~1316-1334)
Between authorization check and `state.send_message()`.
**Insert here:** Rate limiting, message length limits, or premium-only command features.

### D. `state.rs` - `TelegramState` struct (line ~248-270)
Add a `tier_registry: HashMap<i64, UserTier>` field alongside `authorized_chats`.
**Load from disk** similar to `authorized_chats` pattern for persistence.

### E. `handlers.rs` - `handle_start()` welcome message (line ~119-141)
The welcome message already checks `has_tmux()` and `has_summarization()`.
**Insert here:** Show tier-specific capabilities in the welcome message.

---

## Summary Table

| Capability | Current State |
|-----------|--------------|
| Parse mode | HTML only |
| Message splitting | Not implemented |
| Truncation (no API key) | 10 lines / 500 chars |
| Typing indicators | Every 500ms during processing |
| Progress updates | Text edits every 5 lines |
| Incremental summaries | Every 50 lines (LLM) |
| Inline keyboards | Yes - auto-detected from A/B/C, 1/2/3, y/n patterns |
| Reply keyboards | Not used |
| Reply threading | Yes - final response replies to original message |
| Forum topic support | Yes - group mode with topic-to-session mapping |
| Rich media (photos/files) | Not implemented |
| Deep links | Yes - t.me/bot?start=connect_{session} |
| Authorization | Chat-level, file-persisted, pairing code based |
| Session persistence | JSON file, 24h TTL, restored on restart |
| Notification broadcast | Yes - 2s polling, all authorized chats |
| Disable notifications | Not implemented |
| Web preview suppression | Not implemented |
