# Adapter and Processing Detection Analysis

**Date:** 2026-03-04
**Scope:** Claude Code / MPM adapter distinction, processing detection, terminal state patterns, and UX gaps in the Telegram bot
**Investigator:** Research Agent

---

## Executive Summary

The codebase has a **two-tier adapter architecture** that is partially disconnected. The formal adapter abstraction (`commander-adapters`) correctly distinguishes Claude Code from MPM and provides adapter-specific launch commands. However, the Telegram bot's polling and idle-detection logic (`commander-telegram`) uses a single hardcoded `is_claude_ready()` function from `commander-core` for **all** session types, regardless of whether the session is running Claude Code or MPM.

The good news: the Telegram bot **already sends `ChatAction::Typing`** every 500ms while waiting. The gap is not total silence — it is that the existing indicators (typing dots, progress counts) may be insufficient for users waiting on long-running MPM orchestration tasks, and MPM sessions may be incorrectly declared "ready" or may never be declared ready if MPM's idle prompt differs from Claude Code's `❯` character.

---

## 1. Session Types: How MPM vs Claude Code Sessions Are Distinguished

### Storage Layer

Adapter type is stored as a string in the project's persistent config, not in the in-memory `UserSession`.

**File:** `crates/commander-persistence` (via `commander_models::Project`)

The project config map stores the adapter:
```rust
// crates/commander-telegram/src/state.rs:568
project.config.insert("tool".to_string(), serde_json::json!(tool_id));
```

When creating a new project via `/connect <path> -a <adapter>`:
```rust
// crates/commander-telegram/src/state.rs:1546-1549
let tool_id = self.adapters.resolve(adapter)
    .ok_or_else(|| TelegramError::SessionError(
        format!("Unknown adapter: {}. Use: cc (claude-code), mpm", adapter)
    ))?
    .to_string();
```

When connecting to an existing project:
```rust
// crates/commander-telegram/src/state.rs:614-620
let tool_id = project
    .config
    .get("tool")
    .and_then(|v| v.as_str())
    .unwrap_or("claude-code")
    .to_string();
```

### Critical Gap: Adapter Type Is Not Stored in UserSession

`UserSession` (`crates/commander-telegram/src/session.rs`) has **no `adapter` field**. The `tool_id` is used during the `connect()` call to launch the tmux session (if not already running), then **discarded**. Once connected, all sessions are treated identically:

```rust
// crates/commander-telegram/src/session.rs:10-44
pub struct UserSession {
    pub chat_id: ChatId,
    pub project_path: String,
    pub project_name: String,
    pub tmux_session: String,
    pub response_buffer: Vec<String>,
    pub last_output_time: Option<Instant>,
    pub last_output: String,
    pub pending_query: Option<String>,
    pub is_waiting: bool,
    pub pending_message_id: Option<MessageId>,
    // ... no adapter_type field
    pub daemon_session_id: Option<String>,
    pub send_time: Option<std::time::Instant>,
}
```

The `tool_id` is returned from `connect()` and used only for display purposes in the handler response message — it plays no role in subsequent output polling.

### Unregistered Session Fallback

When connecting to an unregistered tmux session (no project record), the tool_id defaults to `"unknown"`:
```rust
// crates/commander-telegram/src/state.rs:723
return Ok((display_name, "unknown".to_string()));
```

---

## 2. Startup Commands: What Launches Each Session Type

### Architecture

The `AdapterRegistry` in `commander-adapters` holds the two adapters:

```rust
// crates/commander-adapters/src/registry.rs
// Aliases: "cc" -> "claude-code", "mpm" -> "mpm", "sh"|"shell" -> "shell"
```

### Claude Code Adapter

**File:** `crates/commander-adapters/src/claude_code.rs`

```rust
// launch_command implementation
pub fn launch_command(&self, project_path: &str) -> (String, Vec<String>) {
    (
        "claude".to_string(),
        vec![
            "--dangerously-skip-permissions".to_string(),
            "--project".to_string(),
            project_path.to_string(),
        ],
    )
}
```

Full command executed: `claude --dangerously-skip-permissions --project <path>`

### MPM Adapter

**File:** `crates/commander-adapters/src/mpm.rs`

```rust
// launch_command implementation
pub fn launch_command(&self, project_path: &str) -> (String, Vec<String>) {
    (
        "claude-mpm".to_string(),
        vec![
            "--project".to_string(),
            project_path.to_string(),
        ],
    )
}
```

Full command executed: `claude-mpm --project <path>`

### How the Launch Happens

**File:** `crates/commander-telegram/src/state.rs:622-650`

```rust
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

Session name format: `commander-{project_name}` (e.g., `commander-my-project`).

---

## 3. Processing Detection: The "Done" Logic

### Where Detection Happens

**File:** `crates/commander-telegram/src/state.rs`

Two parallel functions both implement identical detection logic:

- `poll_output()` at **line 1211** — for direct chats
- `poll_topic_output()` at **line 1000** — for forum group topics

### The Completion Gate: Dual Condition (Lines 1261-1263 and 1055-1057)

```rust
// crates/commander-telegram/src/state.rs:1261-1263 (poll_output)
let is_idle = session.is_idle(1500);          // time-based: 1500ms silence
let has_prompt = is_claude_ready(&current_output); // pattern-based: prompt visible

if is_idle && has_prompt && !session.response_buffer.is_empty() {
    // declare completion
}
```

Both conditions must be simultaneously true:

1. **`session.is_idle(1500)`** — time-based: `last_output_time.elapsed() > 1500ms`
   - **File:** `crates/commander-telegram/src/session.rs:235-238`
   - The session tracks `last_output_time` as an `Instant` updated on every new line received
   - If no new output lines arrive for 1.5 seconds, `is_idle()` returns `true`

2. **`is_claude_ready(&current_output)`** — pattern-based: Claude Code prompt detected
   - **File:** `crates/commander-core/src/output_filter.rs:175-236`
   - **Applied to ALL session types regardless of adapter**

### The `is_claude_ready()` Function in Full

**File:** `crates/commander-core/src/output_filter.rs:175-237`

```rust
pub fn is_claude_ready(output: &str) -> bool {
    let lines: Vec<&str> = output.lines().rev()
        .filter(|l| !l.trim().is_empty())
        .take(10)
        .collect();

    // Pattern 1: Lone ❯ prompt character (last 3 non-empty lines)
    for line in &lines[..lines.len().min(3)] {
        let trimmed = line.trim();
        if trimmed == "❯" || trimmed == "❯ "
            || trimmed.ends_with(" ❯") || trimmed.ends_with(" ❯ ") {
            return true;
        }
    }

    // Pattern 2: Input box separator lines (last 5 lines)
    let has_separator = lines.iter().take(5).any(|l| {
        let trimmed = l.trim();
        trimmed.starts_with("───")  // U+2500 triple
            || trimmed.starts_with("╭─")
            || trimmed.starts_with("╰─")
    });

    // Pattern 3: "bypass permissions" hint text (last 5 lines)
    let has_bypass_hint = lines.iter().take(5).any(|l| l.contains("bypass permissions"));

    // Pattern 4: ❯ within separator context (last 5 lines)
    if has_separator {
        for (i, line) in lines.iter().enumerate() {
            if line.contains("❯") && i < 5 {
                return true;
            }
        }
    }

    // Pattern 5: Ready indicators (last 3 lines)
    let has_ready_indicator = lines.iter().take(3).any(|l| {
        let trimmed = l.trim();
        trimmed == "│ ❯"
            || trimmed.starts_with("│ ❯")
            || trimmed == ">"
            || trimmed.ends_with("> ")
            || trimmed.contains("[ready]")
    });

    has_ready_indicator || has_bypass_hint
}
```

---

## 4. Terminal State Patterns: What Each Tool Shows

### Claude Code Terminal Patterns

**While processing (tool is thinking/generating):**

Detected by `is_ui_noise()` in `crates/commander-core/src/output_filter.rs:58-165`:

| Pattern | Example | Detection |
|---------|---------|-----------|
| Spinner characters (13 chars) | `✳ Working...`, `⏺ Reading file` | First char in `SPINNER_CHARS` array |
| Thinking indicators | `(thinking)`, `thinking...`, `thinking…`, `spelunking` | Case-insensitive string match |
| Box drawing status bar | Lines starting with `╮`, `╭`, `│`, `├`, `└`, etc. | Unicode box drawing char prefix |
| Claude branding | Lines with `▐▛`, `▜▌`, `▝▜`, `▛▘` | Block element sequences |
| Model stat line | `[claude|MPM|69%]` | Contains `|` and `%]` |
| MCP tool invocations | `(MCP)(owner: ...)` | Pattern match |

**When idle/ready (waiting for user input):**

Detected by `is_claude_ready()` in `crates/commander-core/src/output_filter.rs:175-237`:

| Pattern | Example | Confidence |
|---------|---------|-----------|
| Lone `❯` prompt | `❯` on its own line | Very high |
| Box separator + `❯` | `───────` then `│ ❯` | High |
| `bypass permissions` hint | `Bypass with: /bypass permissions` | High |
| `│ ❯` combined | `│ ❯ ` | High |
| Generic `>` | `>` alone | Medium (also matches shell) |

### MPM Terminal Patterns (Formal Adapter Definition)

**File:** `crates/commander-adapters/src/patterns.rs` (mpm module)

The formal MPM adapter defines these patterns — but these are NOT used by `commander-telegram`:

**Idle patterns (unused by telegram bot):**
- `(?i)PM ready` — confidence 0.95
- `(?i)awaiting instructions` — confidence 0.95
- `^>\s*$` — generic prompt, confidence 0.9
- `\[IDLE\]` — explicit marker, confidence 1.0

**Working patterns (unused by telegram bot):**
- `(?i)delegating|assigning` — confidence 0.9
- `(?i)coordinating|orchestrating` — confidence 0.85
- `(?i)processing|working` — confidence 0.8

**The critical implication:** MPM's actual terminal output when idle shows `PM ready` or `awaiting instructions`. The current `is_claude_ready()` function looks for Claude Code's `❯` character. If MPM does not display `❯` when idle, **MPM sessions will never be detected as complete** — the bot will poll forever (or until the 1.5s idle timeout fires but `is_claude_ready` returns false, so the condition never triggers).

### Filter Behavior During Collection

**File:** `crates/commander-core/src/output_filter.rs:49-165` (`is_ui_noise`)

The `find_new_lines()` function calls `is_ui_noise()` on every line to filter terminal chrome before storing to `response_buffer`. This means spinner characters, box drawing, and thinking indicators are already stripped before counting. The response buffer contains only substantive content lines.

---

## 5. Current UX: What the User Sees While Waiting

The Telegram bot provides several feedback mechanisms. The flow after a user sends a message:

```
User sends message
    |
    v
state.send_message() called
    -> tmux.send_line() sends text to the tmux session
    -> session.start_response_collection() sets is_waiting=true
    |
    v
poll_output_loop() runs every 500ms (POLL_INTERVAL_MS=500)
    |
    +-> [For all is_waiting sessions]
    |       bot.send_chat_action(chat_id, ChatAction::Typing)  <-- typing indicator
    |
    +-> poll_output() called
            |
            +-- New lines? every 5 lines -> Progress("📥 Receiving...N lines captured")
            |     [sent/edited as message]
            |
            +-- every 50 lines -> IncrementalSummary(text)
            |     [sent/edited as message]
            |
            +-- idle + prompt visible -> Summarizing
            |     [sent "🤖 Summarizing output..." message]
            |     [then Complete on next poll after summarization]
            |
            +-- Complete -> final response sent
                  [deletes progress/summary message, sends final]
```

**File:** `crates/commander-telegram/src/bot.rs`
- `POLL_INTERVAL_MS = 500`
- Typing indicator sent every 500ms via `bot.send_chat_action(chat_id, ChatAction::Typing)`

### What the user sees in practice:

1. **0ms**: Typing dots appear in the Telegram chat header (standard Telegram `ChatAction::Typing`)
2. **500ms intervals**: Typing dots refreshed (each `ChatAction::Typing` lasts ~5 seconds on Telegram, so it auto-renews)
3. **After ~5 new lines**: "📥 Receiving...5 lines captured" message appears in chat
4. **After ~10 lines**: "📥 Receiving...10 lines captured" (edited in-place)
5. **After ~50 lines**: Incremental summary sent
6. **When idle + prompt**: "🤖 Summarizing output..." appears, then final response

### Current Gaps

1. **Before first 5 lines**: Only typing dots are visible. If Claude Code processes silently without emitting output early, the user sees only dots for an extended period with no status change.

2. **MPM adapter deadlock risk**: If MPM does not show the Claude Code `❯` prompt when idle, the `has_prompt` check in `is_idle && has_prompt` will never return `true`. The session will be stuck: tmux output stops changing (idle timer fires), but `is_claude_ready()` returns `false` because MPM's ready indicator is different. The response is never delivered.

3. **No "thinking" state message**: While Claude Code shows spinner chars in the terminal, the bot strips these as `is_ui_noise()`. There is no translation of "Claude is thinking" into a distinctive Telegram message — only the generic Telegram typing dots.

4. **No start confirmation**: After sending a message, there is no immediate acknowledgment message like "Sent to Claude Code. Waiting for response..." — the user only gets the typing indicator.

---

## 6. Recommended Approach for Spinner / Processing Indicator

### What Already Works (Do Not Break)

- `ChatAction::Typing` is already sent every 500ms — this is the Telegram standard and works well
- Progress messages ("📥 Receiving...N lines captured") work well for verbose sessions
- Incremental summaries handle very long responses

### Recommended: Add an Immediate Acknowledgment Message

The highest-value change is sending an acknowledgment message immediately when a user message is received, then editing it through the processing states. This gives users immediate feedback that the message was received and is being processed.

**Where to insert:**

**File:** `crates/commander-telegram/src/state.rs`

In `send_message()` (line ~1104) or in the message handler after the `send_message()` call, send an initial status message and store its `MessageId` in `UserSession`. The existing `pending_message_id` field is already there for reply threading but could be extended.

Alternatively, a dedicated `status_message_id: Option<MessageId>` field on `UserSession` could track this "thinking" message:

```rust
// In UserSession (conceptual, not implemented)
pub status_message_id: Option<MessageId>,
```

Then in `poll_output_loop()`:
- At first poll after `is_waiting` becomes true: edit status to "Processing..."
- On `Progress(...)`: edit status message with line count
- On `Summarizing`: edit to "Summarizing..."
- On `Complete`: delete status message, send final response

### Recommended: Adapter-Aware Idle Detection

**Where to insert:**

Add `adapter_type: String` to `UserSession`:

```rust
// crates/commander-telegram/src/session.rs
pub struct UserSession {
    // ... existing fields
    pub adapter_type: String,  // "claude-code", "mpm", "unknown"
}
```

Populate it during `connect()`:

```rust
// crates/commander-telegram/src/state.rs:653-659
let mut session = UserSession::new(
    chat_id,
    project.path.clone(),
    project.name.clone(),
    session_name,
);
session.adapter_type = tool_id.clone();  // ADD THIS
```

Then in `poll_output()`, use adapter-aware detection:

```rust
// Instead of: let has_prompt = is_claude_ready(&current_output);
let has_prompt = match session.adapter_type.as_str() {
    "mpm" => is_mpm_ready(&current_output),
    _ => is_claude_ready(&current_output),
};
```

Where `is_mpm_ready()` would be a new function in `commander-core/src/output_filter.rs` checking for `PM ready`, `awaiting instructions`, the `>` prompt, or a time-based fallback.

---

## 7. Unified Detection: Shared vs Different

### What Is Shared (Already Unified)

| Concern | Implementation | File |
|---------|---------------|------|
| Polling interval | 500ms for all sessions | `bot.rs:POLL_INTERVAL_MS` |
| Typing indicator | `ChatAction::Typing` for all `is_waiting` sessions | `bot.rs:poll_output_loop` |
| Line filtering | `is_ui_noise()` applied to all output | `output_filter.rs:is_ui_noise` |
| Idle timer | 1500ms threshold for all sessions | `state.rs:session.is_idle(1500)` |
| Progress updates | Every 5 lines for all sessions | `session.rs:should_emit_progress` |
| Incremental summaries | Every 50 lines for all sessions | `session.rs:should_emit_incremental_summary` |
| tmux capture | `capture_output(session, None, Some(200))` | `state.rs:poll_output` |

### What Is Different (Diverges by Adapter)

| Concern | Claude Code | MPM | Current Code |
|---------|-------------|-----|-------------|
| Idle prompt pattern | `❯` character, `───` separator, `bypass permissions` | `PM ready`, `awaiting instructions`, `>` | Only Claude Code patterns in use |
| Launch command | `claude --dangerously-skip-permissions --project <path>` | `claude-mpm --project <path>` | Correctly diverged in adapters crate |
| Working patterns | Spinner chars (✳⏺●), `thinking...`, `(thinking)` | `delegating`, `coordinating`, `processing` | Spinner detection is generic; formal MPM patterns unused |
| Adapter registry | `"claude-code"`, aliases: `"cc"` | `"mpm"` | Correctly diverged in adapters crate |

### What Needs to Change for Unification

1. **Add `adapter_type` to `UserSession`** (minimal change, high impact)
   - File: `crates/commander-telegram/src/session.rs`
   - Populate in: `crates/commander-telegram/src/state.rs:connect()` and `connect_topic()`

2. **Add `is_mpm_ready()` to `commander-core`**
   - File: `crates/commander-core/src/output_filter.rs`
   - Check for: `PM ready`, `awaiting instructions`, `[IDLE]`, generic `>` prompt
   - Export from: `crates/commander-core/src/lib.rs`

3. **Make `poll_output()` and `poll_topic_output()` adapter-aware**
   - File: `crates/commander-telegram/src/state.rs:1261-1263` and `1055-1057`
   - Replace `is_claude_ready(&current_output)` with adapter-dispatched call

4. **Optional: Use `RuntimeAdapter.analyze_output()` from `commander-adapters`**
   - The `commander-adapters` crate already has `ClaudeCodeAdapter.analyze_output()` and `MpmAdapter.analyze_output()` returning `OutputAnalysis { state: RuntimeState, confidence: f32 }`
   - `TelegramState` already holds `adapters: AdapterRegistry` (line 256) but only uses it for launch command dispatch
   - Extend usage to include output analysis: `self.adapters.get(&session.adapter_type)?.analyze_output(&current_output)`
   - This would give the most correct and extensible behavior — but requires wiring the adapter into the poll loop

---

## Key File Reference Map

| File | Role |
|------|------|
| `crates/commander-adapters/src/traits.rs` | `RuntimeAdapter` trait, `RuntimeState` enum |
| `crates/commander-adapters/src/claude_code.rs` | `ClaudeCodeAdapter`, `launch_command()` |
| `crates/commander-adapters/src/mpm.rs` | `MpmAdapter`, `launch_command()` |
| `crates/commander-adapters/src/patterns.rs` | Regex patterns (idle, working) per adapter |
| `crates/commander-adapters/src/registry.rs` | `AdapterRegistry`, aliases |
| `crates/commander-core/src/output_filter.rs` | `is_claude_ready()`, `is_ui_noise()`, `SPINNER_CHARS`, `detect_adapter()` |
| `crates/commander-telegram/src/session.rs` | `UserSession` struct, `is_idle()`, `should_emit_progress()` |
| `crates/commander-telegram/src/state.rs:175` | `is_claude_ready` import |
| `crates/commander-telegram/src/state.rs:256` | `adapters: AdapterRegistry` field in `TelegramState` |
| `crates/commander-telegram/src/state.rs:614-620` | `tool_id` read from project config |
| `crates/commander-telegram/src/state.rs:624` | `adapters.get(&tool_id)` for launch |
| `crates/commander-telegram/src/state.rs:691` | `tool_id` returned from `connect()` |
| `crates/commander-telegram/src/state.rs:1057` | `is_claude_ready()` call in `poll_topic_output()` |
| `crates/commander-telegram/src/state.rs:1211` | `poll_output()` function |
| `crates/commander-telegram/src/state.rs:1263` | `is_claude_ready()` call in `poll_output()` |
| `crates/commander-telegram/src/bot.rs` | `poll_output_loop()`, `POLL_INTERVAL_MS=500`, `ChatAction::Typing` |

---

## Actionable Recommendations (Prioritized)

### Priority 1 — Fix MPM Idle Detection (Correctness Bug)

Add `adapter_type: String` to `UserSession`, propagate it through `connect()`, and dispatch to `is_mpm_ready()` vs `is_claude_ready()` in `poll_output()`. Without this, MPM sessions may never complete (bot polls forever, response never delivered).

### Priority 2 — Add Immediate Send Acknowledgment (UX Gap)

Send a short "Received — processing..." message immediately when `start_response_collection()` is called, before any output arrives. Edit this message through the subsequent states. This fills the gap between message send and the first "📥 Receiving..." update.

### Priority 3 — Wire `RuntimeAdapter.analyze_output()` into Poll Loop (Architecture Debt)

`TelegramState` already holds `adapters: AdapterRegistry`. Extending its use from launch-time-only to include runtime output analysis would correctly leverage the existing adapter abstraction and make adding new adapters (e.g., `aider`, `cursor`) straightforward.

---

*Research saved to: `docs/research/adapter-processing-detection-analysis-2026-03-04.md`*
