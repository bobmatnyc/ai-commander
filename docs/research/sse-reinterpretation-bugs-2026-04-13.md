# SSE Poller Re-interpretation Bugs

**Date:** 2026-04-13
**Status:** Investigation complete, recommendations ready

## Three Related Issues

1. SSE poller re-interprets the same tmux output blocks repeatedly
2. When new output comes, it should update existing interpretation instead of adding new ones
3. LLM interprets Claude Code "thinking" indicators as questions instead of busy signals

---

## 1. SSE Poller: How It Tracks Interpreted Output

**File:** `crates/commander-api/src/handlers/web.rs`, lines 899-996

### Current mechanism (spawn_session_poller)

Two deduplication layers:

- **`snapshots: HashMap<String, String>`** (line 904) -- stores the full raw tmux capture per session
- **`last_interps: HashMap<String, String>`** (lines 905-906) -- stores the last LLM interpretation per session

### The >100 char diff threshold (line 948)

```rust
if trimmed != prev && trimmed.len().abs_diff(prev.len()) > 100 {
```

**Problem: This compares the WRONG thing.** It compares the total LENGTH difference of the entire 50-line capture, not the actual content change. Two issues:

1. **Length-based, not content-based** -- If Claude writes 50 chars and deletes 50 chars, `abs_diff` = 0, so change is missed entirely
2. **Compares full snapshot, not new lines** -- If the screen scrolls by 3 lines (adding 3 new, dropping 3 old), the length diff might be small even though meaningful new content appeared
3. **No line offset tracking** -- Captures last 50 lines every cycle (line 935: `-S -50`). The same content block stays visible across multiple polls until it scrolls off. Each time something tiny changes (like a spinner), the content passes the threshold again and gets re-interpreted

### Why re-interpretation happens

The second dedup layer (line 966-972) compares interpretation text:
```rust
if interpretation != prev_interp {
```

But the LLM produces slightly different text each time it interprets the same screen content (non-deterministic), so the comparison fails and a new SSE event fires.

### How Telegram avoids this -- see Section 2

---

## 2. Telegram's Approach: How It Avoids Re-interpretation

**File:** `crates/commander-telegram/src/state.rs`, lines 2209-2430
**File:** `crates/commander-telegram/src/session.rs`, lines 346-384

### Key differences from SSE poller

Telegram uses a **fundamentally different model**: it tracks a request-response lifecycle, not continuous screen polling.

1. **`start_response_collection()`** (session.rs:346) -- When user sends a message, captures current tmux output as a baseline (`last_output`), clears `response_buffer`
2. **`find_new_lines()`** (output_filter.rs:378) -- Compares previous and current captures using a **HashSet of line contents**. Only lines NOT in the previous capture are considered "new"
3. **`add_response_lines()`** (session.rs:365) -- Appends only truly new lines to `response_buffer`
4. **Completion detection** -- When `is_claude_ready()` returns true AND session `is_idle(1500ms)`, summarizes accumulated `response_buffer` and sends ONE final message
5. **`last_output` update** (state.rs:2262) -- Updated on every poll cycle: `session.last_output = current_output.clone()`

### Why it works

- Telegram never "re-interprets" the same content because it only processes **delta lines**
- The `find_new_lines()` function uses a HashSet to detect lines that are genuinely new
- Progress/completion are tracked as a state machine (`is_waiting`, `is_summarizing`, `completion_detected_at`)
- Final response is ONE summarized message, not continuous interpretations

### What the SSE poller should copy

1. **Track last_output and use `find_new_lines()`** instead of the length-diff threshold
2. **Accumulate new lines in a buffer** instead of re-interpreting the full screen each time
3. **Only interpret when there's meaningful new content** (new line count > threshold)
4. **Send "update" events** (not new interpretations) when content grows

---

## 3. Claude Code "Thinking" Patterns

### Patterns detected by output_filter.rs

**File:** `crates/commander-core/src/output_filter.rs`

`is_ui_noise()` (lines 58-166) filters these thinking indicators:
- **Spinner chars** (line 33-47): `\u{2733}` (star), `\u{25CF}` (bullet), `\u{25CB}` (circle), `\u{25D0}-\u{25D3}` (half circles), `\u{00B7}` (dot), `\u{23FA}` (record)
- **Thinking text** (lines 119-125): "spelunking", "(thinking)", "thinking...", "thinking\u{2026}"
- **Box drawing / UI** (lines 91-104): `\u{256D}`, `\u{2570}`, `\u{2502}`, etc.
- **Status messages** (lines 128-129): "ctrl+b", "to run in background"
- **Branding** (lines 133-138): "Claude Code v", "claude max", "opus 4", "sonnet"

### is_claude_ready() (lines 175-237)

Detects idle state via:
- Prompt character `\u{276F}` alone or at end of line
- Input box separators (`\u{2500}\u{2500}\u{2500}`, `\u{256D}\u{2500}`, `\u{2570}\u{2500}`)
- "bypass permissions" hint
- Ready indicators `\u{2502} \u{276F}`, `>`, `[ready]`

### What's missing

The `is_ui_noise()` does NOT filter these common Claude Code busy patterns:
- **Braille spinners**: `\u{280B}`, `\u{2819}`, `\u{2839}`, `\u{2838}` (the `\u{28xx}` range)
- **"Generating..."** text
- **Tool execution markers**: lines like `\u{23FA} Read(file.rs)`, `\u{23FA} Bash(ls)`
- **Progress bars**: `[=====>    ]` style

The `clean_response()` function (lines 404-423) filters `\u{23BF}` and `\u{23FA}` prefixed lines but the screen interpretation prompt does not know these indicate "busy".

---

## 4. The LLM Prompt (SCREEN_INTERPRET_PROMPT)

**File:** `crates/commander-core/src/summarizer.rs`, lines 442-451

```
const SCREEN_INTERPRET_PROMPT: &str = r#"You are analyzing an AI coding assistant terminal session.
Analyze the screen content and tell me in ONE sentence what is happening.

Rules:
- If the assistant asked a question, state the question directly
- If the assistant completed a task, summarize what was done in past tense
- If there is an error, state the error briefly
- Be concise - respond with ONLY the content, no preamble or prefix
- Do NOT add prefixes like "Claude is asking:" or "Ready after:" -- just state what happened
- Never mention "the screen shows" or similar meta-language"#;
```

### What the prompt tells the LLM about thinking/busy states

**Nothing.** The prompt has zero guidance about thinking/busy indicators. It only passes a `state_hint` (lines 465-469):

```rust
let state_hint = if is_ready {
    "The session IS ready for input (showing prompt)."
} else {
    "The session is NOT ready - Claude is still processing."
};
```

This hint tells the LLM whether it's idle, but the LLM still interprets the raw screen content which contains spinners, progress indicators, and partial output -- and often misinterprets these as "Claude is asking about X" when it's really just showing a spinner next to a status line.

### How to fix the prompt

Add these rules:

```
- If the session is NOT ready and you see spinner characters, progress bars, or "Thinking..." text,
  respond ONLY with "Processing..." -- do not interpret partial output as questions or actions
- Spinner indicators include: dots (... or ...), braille patterns, circle animations, star symbols
- Lines starting with record symbols or containing "(MCP)" "(Bash)" "(Read)" are tool invocations, not questions
```

---

## Recommended Changes

### A. Fix SSE poller tracking (web.rs spawn_session_poller)

**Current approach** (lines 948): length-diff of full snapshot
**Recommended approach**: Use `find_new_lines()` like Telegram

```rust
// Instead of:
if trimmed != prev && trimmed.len().abs_diff(prev.len()) > 100 {

// Use:
let new_lines = commander_core::find_new_lines(&prev, &trimmed);
if !new_lines.is_empty() && new_lines.len() > 2 {
    // Only interpret when there are meaningful new lines
```

Also: clean the content with `clean_response()` BEFORE comparing, so spinners and UI noise don't trigger false diffs.

### B. Add interpretation update events (not new events)

Currently the SSE poller broadcasts a new `"interpretation"` event every time. The web ChatView appends each one as a new message.

Change to: include a `sequence_id` or `replaces` field in the SSE event so the frontend can UPDATE the existing interpretation bubble instead of adding a new one.

```rust
SessionEvent {
    session_name: session.clone(),
    event_type: "interpretation".to_string(),
    content: interpretation.clone(),
    replaces_previous: true, // NEW FIELD
    timestamp: ...,
    adapter,
}
```

### C. Fix the LLM prompt (summarizer.rs)

Add to `SCREEN_INTERPRET_PROMPT` (after existing rules, ~line 450):

```
- If the session is NOT ready and output contains spinners, progress indicators, or tool execution
  markers (lines with record/play symbols), say "Processing..." -- do not interpret busy output as questions
- Treat "Thinking...", spinner symbols, and progress bars as busy signals, NOT as questions
```

### D. Pre-filter thinking patterns before LLM call

In `interpret_screen_context()` (summarizer.rs:464), add early return for obvious busy states:

```rust
pub fn interpret_screen_context(screen_content: &str, is_ready: bool) -> Option<String> {
    // Fast path: if not ready and content is mostly UI noise, skip LLM call
    if !is_ready {
        let meaningful_lines: Vec<&str> = screen_content.lines()
            .filter(|l| !is_ui_noise(l.trim()) && !l.trim().is_empty())
            .collect();
        if meaningful_lines.len() < 3 {
            return Some("Processing...".to_string());
        }
    }
    // ... existing code
}
```

### E. Add missing spinner patterns to output_filter.rs

Add braille spinner detection to `is_ui_noise()`:

```rust
// Braille spinners (Unicode block 0x2800-0x28FF)
if line.chars().next().map(|c| ('\u{2800}'..='\u{28FF}').contains(&c)).unwrap_or(false) {
    return true;
}
```

---

## Summary of Lines to Change

| File | Lines | Change |
|------|-------|--------|
| `web.rs` | 948 | Replace length-diff with `find_new_lines()` |
| `web.rs` | 958-989 | Clean content before interpreting, accumulate new lines |
| `web.rs` | 973 | Add `replaces_previous` field to SSE event |
| `summarizer.rs` | 442-451 | Add busy/thinking rules to SCREEN_INTERPRET_PROMPT |
| `summarizer.rs` | 464-468 | Add early return for obvious busy states |
| `output_filter.rs` | 80-88 | Add braille spinner range detection |
| Frontend ChatView | N/A | Handle `replaces_previous` to update instead of append |
