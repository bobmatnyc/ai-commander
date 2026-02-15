# Telegram Message Forwarding Analysis

**Date:** 2026-02-14
**Status:** Complete
**Type:** Architectural Analysis + UX Recommendations

## Executive Summary

Investigation of the Telegram bot message forwarding logic revealed that **all session output is forwarded to Telegram** when Claude becomes idle at a prompt, regardless of content type. This causes conversational/clarification responses like "[!] I need your input..." to be forwarded when they probably should not be.

**Root Cause:** There is no filtering for output TYPE - the system uses purely timing-based idle detection without content classification.

---

## Current Architecture

### Message Flow

```
User sends Telegram message
        |
        v
send_message() in state.rs
        |
        v
Message sent to tmux session via tmux_controller::send_input()
        |
        v
start_response_collection() - session enters "waiting" state
        |
        v
poll_output_loop() runs every 500ms (bot.rs)
        |
        v
poll_output() captures tmux output (state.rs)
        |
        v
find_new_lines() detects new content
        |
        v
Lines added to response_buffer
        |
        v
When IDLE (1.5s) + PROMPT VISIBLE + BUFFER NOT EMPTY:
        |
        v
summarize_with_fallback() or clean_response()
        |
        v
Response sent to Telegram
```

### Key Files

| File | Purpose |
|------|---------|
| `crates/commander-telegram/src/bot.rs` | Polling loop, Telegram message sending |
| `crates/commander-telegram/src/state.rs` | Core forwarding decision logic (`poll_output()`) |
| `crates/commander-telegram/src/session.rs` | Session state tracking (`UserSession`) |
| `crates/commander-core/src/output_filter.rs` | Prompt detection, UI noise filtering |
| `crates/commander-core/src/summarizer.rs` | Response summarization via OpenRouter |

### Forwarding Decision Criteria

The forwarding decision is made in `state.rs::poll_output()` at approximately lines 1107-1136:

```rust
// Current decision criteria (simplified)
let is_idle = session.is_idle(1500);      // No output for 1.5 seconds
let has_prompt = is_claude_ready(&current_output);  // Prompt character visible
let has_content = !session.response_buffer.is_empty();

if is_idle && has_prompt && has_content {
    // ALL output matching these criteria is forwarded
    let response = summarize_with_fallback(&query, &raw_response).await;
    return Ok(PollResult::Complete(response, message_id, thread_id));
}
```

### What Gets Filtered (Currently)

The `is_ui_noise()` function in `output_filter.rs` filters:
- Spinner frames and progress indicators
- Box drawing characters (UI borders)
- Claude Code branding
- ANSI escape sequences
- Empty lines

**What is NOT filtered:**
- Clarification questions ("[!] I need your input...")
- Status updates ("Searching files...")
- Interactive prompts ("Should I continue?")
- Error messages requiring user action

---

## Problem Analysis

### Symptom
Claude's conversational responses like "[!] I need your input..." are forwarded to Telegram users when they should trigger different behavior.

### Problem Scenarios

| Scenario | Current Behavior | Expected Behavior |
|----------|-----------------|-------------------|
| Claude asks clarifying question | Forwarded + summarized | Forward question verbatim (for user to answer) |
| Claude gives status update | Forwarded + summarized | Maybe suppress or show briefly |
| Claude completes a task | Forwarded + summarized | Summarize and forward (correct!) |
| Claude shows error needing action | Forwarded + summarized | Forward error clearly, await user action |

### Root Cause
The system uses **timing-based idle detection** without **content classification**. When Claude pauses at a prompt, the system assumes the task is complete and forwards everything as a "response."

---

## Recommendations

### Option 1: Content-Type Classification (Recommended)

Add content classification to distinguish between:

1. **Task Completion** - Claude finished work, shows results
2. **Clarification Request** - Claude asks a question, needs user input
3. **Error/Action Required** - Claude encountered issue, needs user decision
4. **Status Update** - Progress information (may not need forwarding)

**Implementation Approach:**

```rust
// In output_filter.rs, add:

pub enum OutputType {
    TaskCompletion,      // Forward + summarize
    ClarificationRequest, // Forward verbatim (user must respond)
    ActionRequired,       // Forward verbatim with emphasis
    StatusUpdate,         // Maybe suppress or brief notification
    Unknown,              // Default to current behavior
}

pub fn classify_output(content: &str) -> OutputType {
    // Pattern-based classification
    if contains_question_pattern(content) {
        return OutputType::ClarificationRequest;
    }
    if contains_error_pattern(content) {
        return OutputType::ActionRequired;
    }
    if contains_status_pattern(content) {
        return OutputType::StatusUpdate;
    }
    OutputType::TaskCompletion
}

fn contains_question_pattern(content: &str) -> bool {
    let patterns = [
        "[!] I need your input",
        "Would you like",
        "Should I",
        "Do you want",
        "Can you",
        "Is this",
        "Are you sure",
        // End with question mark after Claude's text
    ];
    patterns.iter().any(|p| content.contains(p))
        || content.lines().last().map(|l| l.trim().ends_with('?')).unwrap_or(false)
}
```

**Forwarding Logic Update:**

```rust
// In state.rs::poll_output()
let output_type = classify_output(&raw_response);

match output_type {
    OutputType::ClarificationRequest => {
        // Don't summarize - forward the question verbatim
        let question = extract_question(&raw_response);
        return Ok(PollResult::ClarificationNeeded(question, message_id, thread_id));
    }
    OutputType::ActionRequired => {
        // Forward with emphasis, don't summarize
        let error = format!("Action Required:\n{}", clean_response(&raw_response));
        return Ok(PollResult::ActionRequired(error, message_id, thread_id));
    }
    OutputType::StatusUpdate => {
        // Optionally suppress or send brief notification
        continue; // Skip this poll cycle
    }
    OutputType::TaskCompletion | OutputType::Unknown => {
        // Current behavior - summarize and forward
        let response = summarize_with_fallback(&query, &raw_response).await;
        return Ok(PollResult::Complete(response, message_id, thread_id));
    }
}
```

### Option 2: LLM-Based Classification

Use the existing OpenRouter integration to classify output type before deciding how to handle it.

**Pros:**
- More accurate classification
- Can handle nuanced cases

**Cons:**
- Additional API call (latency, cost)
- May be overkill for common patterns

**Implementation:**
```rust
const CLASSIFICATION_PROMPT: &str = r#"Classify this Claude Code output:
1. TASK_COMPLETE - Claude finished a task
2. QUESTION - Claude is asking the user something
3. ERROR - Claude encountered an error needing user action
4. STATUS - Just progress information

Respond with ONLY the classification word."#;

async fn classify_with_llm(content: &str) -> OutputType {
    // Call OpenRouter with classification prompt
}
```

### Option 3: Hybrid Approach (Best)

Use pattern-based classification for common cases, fall back to LLM for ambiguous content.

```rust
fn classify_output(content: &str) -> OutputType {
    // Fast pattern match first
    if let Some(output_type) = pattern_classify(content) {
        return output_type;
    }

    // Fall back to LLM for ambiguous cases
    if is_available() {
        classify_with_llm(content).await.unwrap_or(OutputType::Unknown)
    } else {
        OutputType::Unknown
    }
}
```

---

## Implementation Plan

### Phase 1: Pattern-Based Classification (Quick Win)

1. Add `OutputType` enum to `output_filter.rs`
2. Implement `classify_output()` with common patterns
3. Add `PollResult::ClarificationNeeded` variant
4. Update `poll_output()` to use classification
5. Update bot.rs to handle new `PollResult` variants

**Estimated Effort:** 2-4 hours

### Phase 2: Enhanced Handling

1. Add different Telegram message formatting per output type
2. Consider inline keyboard buttons for clarification questions
3. Add "conversation mode" tracking for multi-turn Q&A

**Estimated Effort:** 4-8 hours

### Phase 3: LLM Classification (Optional)

1. Add LLM classification prompt
2. Implement caching for repeated patterns
3. Add confidence threshold for hybrid approach

**Estimated Effort:** 2-4 hours

---

## Testing Strategy

### Test Cases

| Test Case | Input | Expected Output |
|-----------|-------|-----------------|
| Task completion | "Created 3 files successfully" | Forward + summarize |
| Question pattern | "[!] I need your input about..." | Forward verbatim |
| Should I continue | "Should I continue with deployment?" | Forward verbatim |
| Error message | "Permission denied: /etc/hosts" | Forward with emphasis |
| Status update | "Searching 500 files..." | Suppress or brief |

### Unit Tests

```rust
#[test]
fn test_classify_clarification_question() {
    let content = "[!] I need your input: Which database should I use?";
    assert_eq!(classify_output(content), OutputType::ClarificationRequest);
}

#[test]
fn test_classify_task_completion() {
    let content = "Successfully created user authentication module with login/logout.";
    assert_eq!(classify_output(content), OutputType::TaskCompletion);
}
```

---

## Decision Matrix

| Approach | Accuracy | Latency | Cost | Complexity |
|----------|----------|---------|------|------------|
| Pattern-based only | Medium | Low | Free | Low |
| LLM-based only | High | Medium | Low | Medium |
| Hybrid (recommended) | High | Low (cached) | Low | Medium |

---

## Summary

The current Telegram forwarding system treats all output identically based on timing alone. By adding content-type classification, the bot can provide more appropriate UX:

- **Clarification questions** forwarded verbatim for user response
- **Task completions** summarized as currently done
- **Errors** highlighted for user attention
- **Status updates** optionally suppressed

**Recommended first step:** Implement pattern-based classification (Phase 1) as a quick win, then iterate based on user feedback.
