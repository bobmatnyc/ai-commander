# Summarizer System Analysis

**Date:** 2026-04-01
**Scope:** Complete analysis of the summarization pipeline in ai-commander
**Status:** Informational research

---

## 1. Architecture Overview

The summarizer system transforms raw tmux terminal output from Claude Code sessions into concise, mobile-friendly Telegram messages. It operates as a pipeline:

```
tmux capture_output (200 lines)
    |
    v
find_new_lines() — diff against previous capture, filter UI noise
    |
    v
response_buffer (Vec<String>) — accumulates filtered lines
    |
    v
[Progressive Summary — every 500 chars of new output]
[Incremental Summary — every 50 lines]
    |
    v
Completion detected (idle + prompt visible for 1.5s)
    |
    v
summarize_with_fallback() OR clean_response()
    |
    v
Telegram message
```

## 2. Key Files and Functions

### Core Summarizer Module
**File:** `crates/commander-core/src/summarizer.rs` (430 lines)

| Function | Line | Purpose |
|----------|------|---------|
| `is_available()` | 88 | Checks `OPENROUTER_API_KEY` env var |
| `get_api_key()` | 93 | Returns API key from env |
| `get_model()` | 98 | Returns `OPENROUTER_MODEL` or default `anthropic/claude-sonnet-4` |
| `summarize_async()` | 112 | Async summarization via OpenRouter API |
| `summarize_blocking()` | 163 | Blocking version of above |
| `summarize_with_fallback()` | 207 | Async with auto-fallback to truncation |
| `summarize_blocking_with_fallback()` | 226 | Blocking version of above |
| `summarize_incremental()` | 250 | Brief progress summary (150 max_tokens) |
| `fallback_truncate()` | 19 | Truncates to 10 lines / 500 chars when API unavailable |
| `interpret_screen_context()` | 322 | LLM interprets what Claude is asking (blocking, 10s timeout) |

### Output Filter Module
**File:** `crates/commander-core/src/output_filter.rs` (867 lines)

| Function | Line | Purpose |
|----------|------|---------|
| `is_ui_noise()` | 58 | Detects prompt lines, spinners, box drawing, branding, thinking indicators |
| `find_new_lines()` | 378 | Diffs previous/current tmux capture, filters noise |
| `clean_response()` | 404 | Strips UI artifacts (continuation markers, progress indicators, MCP noise) |
| `clean_screen_preview()` | 429 | Returns last N meaningful lines for status display |
| `is_claude_ready()` | 175 | Detects Claude Code idle prompt patterns |
| `is_mpm_ready()` | 246 | Detects MPM idle patterns |
| `detect_selector()` | 498 | Detects interactive selector prompts (Inquirer.js, numbered lists, Y/N) |

### Notification Parser
**File:** `crates/commander-core/src/notification_parser.rs` (756 lines)

This module parses timer notification strings into structured `ParsedSessionStatus` with session name, path, branch, model, and context usage. It does NOT participate in the summarization pipeline directly -- it handles notification parsing for status display.

### Poll Loop and Session State
**File:** `crates/commander-telegram/src/state.rs` (2269 lines)

| Function | Line | Purpose |
|----------|------|---------|
| `poll_output()` | 1560 | Main poll function for 1:1 chats |
| `poll_topic_output()` | 1065 | Poll function for forum topic sessions |
| `has_summarization()` | 377 | Delegates to `commander_core::is_summarization_available()` |

**File:** `crates/commander-telegram/src/session.rs` (330 lines)

| Function/Field | Line | Purpose |
|----------------|------|---------|
| `response_buffer` | 21 | `Vec<String>` accumulating filtered output lines |
| `chars_since_last_summary` | 58 | Tracks chars for progressive summary trigger |
| `should_emit_incremental_summary()` | 313 | Returns true every 50 lines |
| `should_emit_progress()` | 295 | Returns true every 5 lines |
| `is_idle()` | 282 | True when no new output for given ms threshold |

**File:** `crates/commander-telegram/src/bot.rs`

| Constant/Function | Line | Purpose |
|--------------------|------|---------|
| `POLL_INTERVAL_MS` | 24 | 500ms poll interval |
| `poll_output_loop()` | 422 | Background task processing all PollResult variants |

### Re-exports
**File:** `crates/commander-core/src/lib.rs` (lines 33-36)

Re-exports `is_available` as `is_summarization_available`, along with `summarize_async`, `summarize_blocking`, `summarize_blocking_with_fallback`, `summarize_incremental`, `summarize_with_fallback`.

## 3. Summarization Triggers

### Trigger 1: Progressive Summary (every 500 chars)
- **Location:** `state.rs` lines 1137-1149 (`poll_topic_output`) and 1624-1636 (`poll_output`)
- **Condition:** `chars_since_last_summary >= 500 && is_summarization_available()`
- **Action:** Calls `summarize_incremental()` with content so far
- **Output:** `PollResult::ProgressiveSummary` with "pencil" prefix
- **Token budget:** 150 max_tokens (incremental prompt)
- **Resets:** `chars_since_last_summary = 0` after trigger

### Trigger 2: Incremental Summary (every 50 lines)
- **Location:** `state.rs` lines 1152-1165 and 1639-1654
- **Condition:** `response_buffer.len() >= last_incremental_summary_line_count + 50`
- **Action:** Also calls `summarize_incremental()`
- **Output:** `PollResult::IncrementalSummary`
- **Token budget:** 150 max_tokens
- **Note:** Both triggers can fire on the same poll cycle (progressive checked first)

### Trigger 3: Final Summary (on completion)
- **Location:** `state.rs` lines 1246-1289 and 1734-1781
- **Condition:** `is_idle(1500) && has_prompt && is_summarization_available()`
- **Action:** First returns `PollResult::Summarizing`, then on next poll calls `summarize_with_fallback()`
- **Token budget:** 500 max_tokens (full summary prompt)
- **Two-pass pattern:** Uses `completion_detected_at` to signal Summarizing on first detection, then does actual work on next poll (prevents re-checking idle which could reset)

### Non-summarized Path
- **Condition:** `is_summarization_available()` returns false (no `OPENROUTER_API_KEY`)
- **Action:** `clean_response()` strips UI artifacts and returns filtered text
- **Fallback truncation:** If summarization was attempted but failed, `fallback_truncate()` limits to 10 lines / 500 chars

## 4. Summarization Backend

### API
- **Service:** OpenRouter API (`https://openrouter.ai/api/v1/chat/completions`)
- **Default model:** `anthropic/claude-sonnet-4` (line 41 of summarizer.rs)
- **Configurable via:** `OPENROUTER_MODEL` env var

### System Prompts
Two distinct prompts:

1. **Final summary** (`SYSTEM_PROMPT`, line 47): "Be concise but informative (2-4 sentences)... Focus on what was DONE or LEARNED..."
2. **Incremental summary** (`INCREMENTAL_SYSTEM_PROMPT`, line 60): "Be VERY concise (2-3 sentences maximum)... Say 'Found X...' or 'Analyzed Y...'"

### Token Limits
| Call Type | max_tokens | Typical Input |
|-----------|-----------|---------------|
| Final summary | 500 | Full response buffer |
| Incremental summary | 150 | Content so far |
| Screen interpretation | 100 | Last 3000 chars of screen |

### Cost Estimate
Using Claude Sonnet 4 via OpenRouter (approximately $3/1M input, $15/1M output):
- **Final summary:** ~2000 input tokens + ~200 output tokens = ~$0.009 per summary
- **Incremental summary:** ~1000 input tokens + ~60 output tokens = ~$0.004 per summary
- **Per response cycle with long output (100+ lines):** 1 final + 2-3 incrementals + 2-3 progressives = ~$0.030

## 5. Configuration

| Env Var | Purpose | Default |
|---------|---------|---------|
| `OPENROUTER_API_KEY` | Enables summarization entirely | None (summarization disabled) |
| `OPENROUTER_MODEL` | Model for all summarization calls | `anthropic/claude-sonnet-4` |

No feature flags, config files, or other toggles. Summarization is a binary on/off based solely on API key presence.

## 6. Performance Characteristics

### Latency
- **Poll interval:** 500ms
- **Idle detection:** 1500ms of no new output
- **API call latency:** Typically 1-3 seconds for final summary, 0.5-1.5s for incremental
- **Two-pass completion:** Adds minimum 500ms delay (one extra poll cycle for Summarizing state)
- **Net response latency:** 2-5 seconds added by summarization after Claude finishes

### Blocking Behavior
- `summarize_with_fallback()` and `summarize_incremental()` are **async** -- they do NOT block the poll loop for other sessions
- However, the `sessions` write lock IS held during the poll for a given session, meaning other sessions' polls cannot interleave with summarization
- The `interpret_screen_context()` function uses **blocking** HTTP with 10s timeout -- but this is only called from status display, not the poll loop

### Caching
- **No caching** of any kind. Every summarization call is a fresh API request.
- Identical content could be re-summarized if progressive and incremental triggers overlap.

### Memory
- `response_buffer` grows unbounded during a response cycle (all filtered lines kept in memory)
- No limit on buffer size -- a very long response could accumulate thousands of lines

## 7. The Non-Summarized Path (`clean_response`)

When `OPENROUTER_API_KEY` is not set:

```
raw tmux output
    |
    v
clean_response() — strips:
  - Empty lines
  - Continuation markers (U+23BF)
  - Progress indicators (U+23FA)
  - Lines containing "hook" or "ctrl+o"
  - Lines containing "(MCP)"
  - Lines starting with "Reading" or "Searched"
    |
    v
Remaining lines joined with \n
    |
    v
Sent directly as Telegram message
```

This can result in very long messages being sent to Telegram (4096 char limit per message, handled by `send_long_message()` in bot.rs).

## 8. Notification Parser Role

The `notification_parser.rs` module is NOT part of the summarization pipeline. It parses timer notification strings (like `[timer] 1 new session(s) waiting for input: @session-name ...`) into structured `ParsedSessionStatus` objects for display purposes. The `to_conversational()` and `to_brief()` methods format session status for human consumption.

## 9. TODO/FIXME Comments

**None found** in summarizer.rs, state.rs, or session.rs related to summarization.

## 10. Opportunities for Improvement

### Critical Issues

1. **Redundant API calls from overlapping triggers:**
   - Progressive summary (every 500 chars) and incremental summary (every 50 lines) can both fire in the same poll cycle. Progressive is checked first, but if it fails, incremental still fires. Both call the same `summarize_incremental()` function with the same content.
   - **Fix:** Unify these into a single trigger mechanism, or skip incremental if progressive already fired this cycle.

2. **No input size limit for summarization:**
   - The entire `response_buffer` is passed as input to the LLM. A 500-line response could mean 10K+ tokens of input.
   - **Fix:** Truncate or sample input to last N lines / N chars before calling the API.

3. **Write lock held during async API call:**
   - `poll_output()` holds a `sessions.write()` lock while awaiting `summarize_with_fallback()`. This blocks ALL session reads/writes during the 1-3 second API call.
   - **Fix:** Extract response data, drop the lock, perform summarization, then use a separate write to mark completion.

### Performance Improvements

4. **No caching of summaries:**
   - If the same content triggers both progressive and incremental summaries, two API calls are made.
   - **Fix:** Cache last summary content hash and skip if content hasn't changed significantly.

5. **Progressive summary threshold too aggressive:**
   - 500 chars is very frequent. For actively streaming output, this could fire every 1-2 seconds, creating substantial API costs.
   - **Fix:** Increase threshold to 1000-2000 chars, or add a minimum time interval between progressive summaries.

6. **`interpret_screen_context()` uses blocking HTTP:**
   - This function creates a `reqwest::blocking::Client` on every call with no connection pooling.
   - **Fix:** Use async version or maintain a shared client.

### Architectural Improvements

7. **Duplicated poll logic:**
   - `poll_output()` and `poll_topic_output()` are nearly identical (~200 lines each). The only difference is the session key calculation.
   - **Fix:** Extract shared poll logic into a private method that takes a session key.

8. **Model hardcoded as `claude-sonnet-4`:**
   - For incremental/progressive summaries, a cheaper/faster model like Haiku would be more appropriate.
   - **Fix:** Add `OPENROUTER_INCREMENTAL_MODEL` env var, default to Haiku for incremental summaries.

9. **No observability on API costs:**
   - No tracking of tokens used, API costs, or latency per summarization call.
   - **Fix:** Log token usage from OpenRouter response and track cumulative costs.

10. **Fallback truncation is too aggressive:**
    - When API key is missing, truncation to 10 lines / 500 chars loses substantial information.
    - **Fix:** Increase fallback limits or implement a basic extractive summarization (first + last paragraphs).

### Cost Reduction

11. **Use cheaper model for incremental summaries:**
    - Claude Sonnet at ~$3/1M input is expensive for brief progress updates. Claude Haiku at ~$0.25/1M would be 12x cheaper.
    - Estimated savings: 60-80% of incremental summary costs.

12. **Batch progressive and incremental into single call:**
    - When both would fire, make one call and use the result for both purposes.
