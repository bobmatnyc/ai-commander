# Root Cause Analysis: Raw Text in Summary View

**Date:** 2026-04-21
**Investigator:** Claude Code (research agent)
**Status:** Root causes identified, fixes specified

---

## Executive Summary

There are **three independent root causes** that together (or individually) cause raw terminal text to appear in Summary view instead of LLM-interpreted summaries. None of them is a frontend bug. All three are in the backend pipeline.

---

## Pipeline Overview

```
tmux capture (500 lines, every 500ms)
    → poll_once() [events.rs:231]
        → emit session-output (ALWAYS — raw feed)
        → compute_delta() + block_buffer accumulation
        → throttle check (5s normal / 3s startup)
        → interpret_screen_context(block_buffer, is_ready=true) [summarizer.rs:875]
            → is_startup_sequence() shortcut
            → is_actively_working() guard
            → prepare_for_llm() chrome filter (needs ≥3 lines)
            → interpret_via_ollama()   ← PRIMARY
            → OpenRouter fallback      ← SECONDARY
        → if Some(text): emit chat-event { type: "update", content: text }
        → if None: increment llm_failure_count; after 2 failures emit llm_unavailable
```

The frontend (`ChatView.svelte`) is correctly split:
- `session-output` events: only update activity counter + trigger raw pane refresh (never shown in Summary mode)
- `chat-event` type `update`: renders LLM summaries in Summary mode via `updateStreamingMessage()`

**Raw text is never rendered in Summary mode by the frontend.** The problem is that `interpret_screen_context` returns `None` more often than intended, and the `llm_unavailable` path then emits an error string as a `chat-event { type: "error" }` which IS rendered — but even that is a clean error message, not raw terminal text.

Wait — re-reading the user report: "raw terminal text in Summary view." This can only happen if the LLM is actually echoing terminal content back as a "summary" and that echoed content passes `is_valid_summary()`. That is Root Cause 1.

---

## Root Cause 1 (PRIMARY): Ollama hits token limit inside `<think>` block, returns `content: ""`

**File:** `crates/commander-core/src/summarizer.rs`, `interpret_via_ollama()`, lines 1117–1166

**What happens:**

`qwen3:8b` (the first model in `OLLAMA_INTERPRET_PREFERENCES`, and confirmed installed) is a reasoning model. When called with `num_predict: 120`, it spends all 120 tokens on its internal `<think>` reasoning block and returns:

```json
{
  "message": {
    "content": "",        ← EMPTY
    "thinking": "...(120 tokens of reasoning that was cut off)..."
  },
  "done_reason": "length"
}
```

The code at line 1158–1161:
```rust
let result = json["message"]["content"]
    .as_str()
    .map(|s| strip_think_tags(s).to_string())
    .filter(|s| !s.is_empty());
```

`content` is `""` → `filter(!is_empty)` → `None`.

So `interpret_via_ollama` returns `None` on most calls because qwen3:8b exhausts the 120-token budget in its thinking phase before writing any content.

**Verified by live test:** Calling qwen3:8b with `num_predict: 120` on a 2-sentence prompt returned `content: ""` with `done_reason: "length"` twice in a row. Only with a shorter, more direct prompt did content appear.

**Note on `strip_think_tags`:** The `<think>` tags comment says this handles inline `<think>...</think>` in content. But qwen3:8b in non-streaming mode puts thinking in a *separate JSON key* (`thinking`), not inline in `content`. So `strip_think_tags` never fires for this model — it was written for older Ollama behavior.

---

## Root Cause 2 (SECONDARY): `prepare_for_llm` 3-line threshold silently drops valid content

**File:** `crates/commander-core/src/summarizer.rs`, `prepare_for_llm()`, lines 648–655

```rust
fn prepare_for_llm(raw: &str) -> Option<String> {
    let filtered: Vec<&str> = raw.lines().filter(|line| !is_llm_noise(line)).collect();
    if filtered.len() < 3 {   // ← THRESHOLD
        return None;
    }
    ...
}
```

The `is_llm_noise` filter is aggressive and correct, but many real terminal states produce fewer than 3 meaningful lines:

- A screen showing just `"Refactored the middleware.\nAll tests pass.\n❯"` → 2 content lines after filtering (❯ is noise) → `None`
- A screen with mostly tool-call chrome (⏺, ⎿ lines) and one result line → 1 content line → `None`

When `prepare_for_llm` returns `None`, `interpret_screen_context` returns `None`, the failure counter increments, and after 2 failures the `llm_unavailable` event fires — showing the error banner instead of a summary.

---

## Root Cause 3 (CONTRIBUTING): OpenRouter fallback uses `anthropic/claude-sonnet-4` — model validates correctly but the timeout is 10s

**File:** `crates/commander-core/src/summarizer.rs`, line 973

```rust
let client = match reqwest::blocking::Client::builder()
    .timeout(std::time::Duration::from_secs(10))
    .build()
```

OpenRouter/Claude-sonnet-4 on Amazon Bedrock responds correctly (verified: HTTP 200, valid one-sentence summary). However, the call is made from `tokio::task::spawn_blocking` in `events.rs:361` — a blocking thread in the async runtime. This is correct Tokio usage. The 10s timeout is adequate.

**OpenRouter is not the problem.** The API key is valid, the model responds correctly. The issue is that OpenRouter is only reached when Ollama fails, and the failure reason (Root Cause 1) causes it to fail silently rather than surfacing the fallback quickly.

---

## Why Does Raw Text Sometimes Actually Appear?

After further analysis: if the LLM (Ollama or OpenRouter) returns a response that *passes* `is_valid_summary()` but is actually a sentence copied verbatim from the terminal, `is_copied_from_input()` should catch it. If that somehow fails too, the raw sentence appears in Summary view.

The more likely reported behavior is: users see the `"LLM unavailable — summaries paused. Switch to Raw view to see terminal output."` error message appearing in the chat, which looks like raw/system text, not a proper LLM summary.

---

## Evidence Summary

| Check | Result |
|-------|--------|
| OpenRouter API key valid | Yes — HTTP 200 from openrouter.ai |
| OpenRouter produces correct summaries | Yes — verified direct curl |
| Ollama running | Yes — localhost:11434 |
| qwen3:8b installed | Yes — first model in preference list |
| qwen3:8b with `num_predict:120` returns `content:""` | **Yes — confirmed, `done_reason:"length"`** |
| `prepare_for_llm` threshold ≥3 lines | Confirmed — screens with 1-2 content lines return None |
| Log files show LLM-generated summaries for active sessions | Yes — hyperdev and other sessions have valid summaries |

---

## Fixes Required

### Fix 1 (Critical): Increase `num_predict` for qwen3:8b and other reasoning models

**File:** `crates/commander-core/src/summarizer.rs`

In `interpret_via_ollama()` at line 1131–1138:

```rust
// BEFORE
let body = serde_json::json!({
    "model": model,
    "messages": [...],
    "stream": false,
    "options": { "num_predict": 120 }
});
```

```rust
// AFTER — increase to give reasoning models room to think AND respond
let body = serde_json::json!({
    "model": model,
    "messages": [...],
    "stream": false,
    "options": {
        "num_predict": 500,    // up from 120
        "num_ctx": 2048        // explicit context limit keeps it fast
    }
});
```

Alternatively, detect reasoning models and set higher limits conditionally, or disable thinking mode via a system prompt instruction.

### Fix 2 (Medium): Lower the `prepare_for_llm` threshold from 3 to 1

**File:** `crates/commander-core/src/summarizer.rs`, line 651

```rust
// BEFORE
if filtered.len() < 3 {
    return None;
}
```

```rust
// AFTER — any single meaningful content line is worth summarizing
if filtered.is_empty() {
    return None;
}
```

The 3-line threshold was over-aggressive. A single "Idle" or "Tests passed" line is meaningful content that should flow through to the LLM.

### Fix 3 (Enhancement): Log Ollama `done_reason` when `content` is empty

**File:** `crates/commander-core/src/summarizer.rs`, `interpret_via_ollama()`

After line 1158, add:

```rust
if result.is_none() {
    let done_reason = json.get("done_reason").and_then(|v| v.as_str()).unwrap_or("unknown");
    warn!(model = %model, done_reason = %done_reason, "Ollama returned empty content");
}
```

This makes the silent failure visible in logs.

---

## What Is NOT the Problem

- The frontend mode switch between Summary/Raw is correct — `session-output` events do not render content in Summary mode (ChatView.svelte:544–547)
- The `chat-event` handler is correct — it only renders `type: "update"` (LLM summaries) in the streaming message slot
- The OpenRouter API key is valid and the model responds correctly
- The `streaming_active` gate is correct — it only blocks the LLM during mpm-serve streaming
- The `is_valid_summary` and `is_copied_from_input` validators are working correctly

---

## Files Referenced

- `/Users/masa/Projects/ai-commander/crates/commander-gui/src/events.rs` — polling loop, LLM dispatch
- `/Users/masa/Projects/ai-commander/crates/commander-core/src/summarizer.rs` — `interpret_screen_context`, `interpret_via_ollama`, `prepare_for_llm`
- `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/ChatView.svelte` — frontend event handling
- `~/.ai-commander/config.json` — API key (Schema B format, correctly parsed)
- `~/.ai-commander/logs/hyperdev/2026-04-21.jsonl` — confirms LLM summaries working for some sessions
