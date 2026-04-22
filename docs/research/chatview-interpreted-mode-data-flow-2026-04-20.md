# ChatView Interpreted Mode: Data Flow Research

**Date:** 2026-04-20
**Scope:** How Claude responses reach ChatView "Interpreted" mode in the Tauri desktop app.

---

## Files Analyzed

- `crates/commander-gui/src/events.rs` — background polling loop, `session-output` emission
- `crates/commander-gui/src/main.rs` — event loop wiring
- `crates/commander-gui/src/commands.rs` — `send_message_streaming`, `interpret_session`, `get_session_summary`
- `crates/commander-gui/ui/src/lib/components/ChatView.svelte` — frontend event handling

---

## Architecture Summary

### Background Polling (events.rs lines 128-222)

`main.rs:19` spawns a single background task: `events::start_session_polling()`.

Every 500ms it:
1. Calls `tmux.capture_output(&session_name, None, Some(500))` — up to 500 lines of scrollback
2. Runs `clean_output()` — strips ANSI escape codes and UI-chrome (box-drawing, MPM banners, spinners)
3. Diffs line count + content hash against previous poll
4. Emits `"session-output"` on change

**Payload structure (SessionOutput):**
```rust
pub struct SessionOutput {
    pub session: String,
    pub content: String,       // incremental new lines; empty on full-refresh
    pub full_content: String,  // always the full cleaned snapshot
}
```
Content is cleaned but unstructured tmux text — no semantic parsing of Claude's response.

### `chat-event` Emission (commands.rs lines 303-454)

`chat-event` is emitted **only** by `send_message_streaming`. That command:
1. POSTs to `http://localhost:7777/api/v1/sessions/{session}/messages` (mpm-serve SSE endpoint)
2. Streams SSE events and re-emits as Tauri `chat-event` with types: `text`, `tool_use`, `complete`, `error`

**Fallback path (lines 442-450):** When port 7777 is unreachable or returns non-2xx, it falls back to `tmux.send_line()` — no `chat-event` is emitted.

---

## Key Findings

### Q1. Does `chat-event` fire for all session types?

**No.** Only for `claude-mpm` sessions when the mpm-serve daemon is running on port 7777 AND the message was sent via `send_message_streaming`. For claude-code, shell, auggie, codex, or mpm without serve: no `chat-event`.

### Q2. `session-output` payload content

Cleaned but unstructured tmux text — Claude Code terminal chrome (file-edit confirmations, tool-use lines, thinking indicators, prompts) all appear verbatim after ANSI/box-drawing stripping.

### Q3. What does the user see in interpreted mode when `chat-event` never fires?

Almost nothing. In ChatView.svelte `onMount` (lines 398-422), `session-output` in interpreted mode only updates the activity counter and blink dot:

```js
// Track activity indicator only (no content added in interpreted mode)
lineCount += raw.split('\n').length;
isActive = true;
// Interpreted mode: do nothing here. Content is driven purely by the
// `chat-event` stream (user messages + Claude responses).
```

The user sees only the "Connected to session: X" system marker and the green activity dot. No Claude response text appears.

### Q4. Is there parsing of Claude responses from tmux output?

In the Tauri desktop path: **no**. `interpret_session` (commands.rs lines 21-46) calls `commander_core::interpret_screen_context()` (OpenRouter/Ollama LLM call), but this is only triggered by explicit user action (Status button), not on new output events.

The web-mode SSE path (`handleSseEvent`, ChatView.svelte lines 248-289) handles `event_type === 'interpretation'` from the REST API — but in Tauri mode `startSseSubscription` is a no-op (line 293: `if (isDesktop()) return`).

**No automatic parsing of Claude responses from tmux output exists for the Tauri desktop interpreted view.**

### Q5. What would "properly interpreting results" look like?

Three approaches:

**Option A — Pattern-based extraction (no LLM, zero latency):**
After `clean_output()` in `events.rs`, identify Claude reply text heuristically. Claude Code output has structure: user prompt is echoed, reply follows. Lines between tool-use blocks and before the next `>` prompt are the response. Brittle but instant. Emit a `"session-interpretation"` event instead of relying on `chat-event`.

**Option B — Hook `interpret_session` into polling loop (low effort, correct path):**
In `start_session_polling`, when new lines are detected, call `commander_core::clean_screen_preview()` (already exists, used in `get_session_summary`) to extract last N meaningful lines, and emit a `"session-interpretation"` Tauri event. ChatView listens for it and adds a `received` message. No LLM cost, works for all session types.

**Option C — Structured streaming via mpm-serve (current design intent):**
Ensure all sends go through `send_message_streaming` and mpm-serve is always running. Gives structured `text`/`tool_use`/`complete` events. Gap: only works for mpm sessions with serve active; claude-code would need a parallel adapter.

**Recommended lowest-effort fix:** Option B. The polling loop already does the heavy lifting; adding a `clean_screen_preview()` call and emitting a second event would give interpreted mode useful content for all session types without LLM cost.

---

## Data Flow Diagram

```
tmux pane
    |
    | (every 500ms)
    v
events::start_session_polling()
    |
    | clean_output() — strip ANSI + UI chrome
    | diff line count + hash
    v
Tauri event: "session-output"
    {session, content (incremental), full_content (snapshot)}
    |
    v
ChatView.svelte onMount listener (line 398)
    |
    +--[raw mode]--> refreshRawContent() --> capture_session_output command --> display
    |
    +--[interpreted mode]--> update lineCount + activity dot ONLY
                              ^
                              NO RESPONSE TEXT DISPLAYED

send_message_streaming (commands.rs:303)  <-- only path to interpreted content
    |
    | POST localhost:7777 (mpm-serve only)
    v
SSE stream --> Tauri event: "chat-event"
    {type: "text"|"tool_use"|"complete"|"error", content, accumulated}
    |
    v
ChatView.svelte onMount listener (line 424)
    |
    +-[text]--> updateStreamingMessage() --> displayed as "claude" received message
    +-[tool_use]--> system message "Using tool: X"
    +-[complete]--> finalizeStreamingMessage() + optional cost
    +-[error]--> system message "Error: X"
```

---

## Action Items

- [ ] Implement Option B: emit `"session-interpretation"` from polling loop using `clean_screen_preview()` for all session types
- [ ] Or: expose `interpret_session` as a Tauri event hook triggered by `session-output` (with debounce to avoid LLM spam)
- [ ] Document that interpreted mode currently only works for mpm sessions with mpm-serve active
