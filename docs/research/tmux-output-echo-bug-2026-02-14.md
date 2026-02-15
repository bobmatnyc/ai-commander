# Tmux Output Echo Bug Investigation

**Date:** 2026-02-14
**Status:** Root Cause Identified
**Severity:** Critical

## Executive Summary

**Root Cause Found:** The `agents` feature flag in the Telegram bot causes user input to be processed through the User Agent (an LLM), and the LLM's response is mistakenly sent to the tmux session as input instead of being returned to the user.

**Location:** `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs` lines 965-1004

---

## Bug Description

When the `agents` feature is enabled:
1. User sends a message via Telegram (e.g., "write tests")
2. The message is processed through `orch.process_user_input(message)`
3. The User Agent LLM generates a response like `"[!] I need your input: Which database?"`
4. **BUG:** This LLM response is sent to tmux via `tmux.send_line()` instead of being returned to the user
5. Claude Code receives "[!] I need your input..." as if the user typed it
6. Claude responds to this, creating a feedback loop

---

## Code Analysis

### The Problematic Code

```rust
// /Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs
// Lines 960-1004

pub async fn send_message(&self, chat_id: ChatId, message: &str, message_id: Option<MessageId>) -> Result<()> {
    let tmux = self.tmux.as_ref().ok_or_else(|| {
        TelegramError::TmuxError("tmux not available".to_string())
    })?;

    // Try to process message through orchestrator first (agents feature)
    #[cfg(feature = "agents")]
    let processed_message = {
        let mut orchestrator = self.orchestrator.write().await;
        if let Some(ref mut orch) = *orchestrator {
            match orch.process_user_input(message).await {
                Ok(processed) => {
                    debug!(
                        original = %message,
                        processed = %processed,
                        "Message processed through orchestrator"
                    );
                    processed  // <-- BUG: This is LLM output, not processed user input!
                }
                Err(e) => {
                    warn!(error = %e, "Orchestrator processing failed, using original message");
                    message.to_string()
                }
            }
        } else {
            message.to_string()
        }
    };

    // ...

    // Send the processed message
    tmux.send_line(&session.tmux_session, None, &processed_message)  // <-- BUG: Sends LLM response as input!
        .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

    // ...
}
```

### What `process_user_input` Actually Returns

From `/Users/masa/Projects/ai-commander/crates/commander-orchestrator/src/orchestrator.rs`:

```rust
pub async fn process_user_input(&mut self, input: &str) -> Result<String> {
    let context = self.user_agent.context().clone();
    let response = self
        .user_agent
        .process(input, &context)  // <-- Calls LLM
        .await
        .map_err(OrchestratorError::Agent)?;

    // ...

    Ok(response.content)  // <-- Returns LLM's response text, NOT the processed input
}
```

### User Agent Response Format

From `/Users/masa/Projects/ai-commander/crates/commander-agent/src/user_agent/mod.rs`:

```rust
pub(crate) const DEFAULT_SYSTEM_PROMPT: &str = r#"You are an autonomous AI agent...

## Response Format When Blocked
"[!] I need your input:
[Clear description of what's needed]

Options:
1. [Option A]
2. [Option B]
..."
```

The User Agent is designed to output conversational responses like `"[!] I need your input:"` - these are meant for the USER, not to be typed into Claude Code.

---

## The Bug Flow

```
User sends "write tests" via Telegram
        |
        v
send_message() receives message = "write tests"
        |
        v
[agents feature enabled]
        |
        v
orch.process_user_input("write tests")
        |
        v
User Agent LLM thinks about it...
        |
        v
Returns: "[!] I need your input: Which test framework?"
        |
        v
processed_message = "[!] I need your input: Which test framework?"
        |
        v
tmux.send_line(session, processed_message)  <-- WRONG!
        |
        v
Claude Code receives "[!] I need your input: Which test framework?"
as if the USER typed it
        |
        v
Claude Code responds to this unexpected input
        |
        v
Loop continues...
```

---

## The Fix

### Option A: Remove Orchestrator from Message Path (Recommended)

The User Agent was designed for a different purpose (coordinating tasks) and should NOT be in the message forwarding path.

```rust
// Remove the agents processing from send_message entirely
pub async fn send_message(&self, chat_id: ChatId, message: &str, message_id: Option<MessageId>) -> Result<()> {
    let tmux = self.tmux.as_ref().ok_or_else(|| {
        TelegramError::TmuxError("tmux not available".to_string())
    })?;

    // Send user's message directly (no LLM processing)
    let mut sessions = self.sessions.write().await;
    let session = sessions
        .get_mut(&chat_id.0)
        .ok_or(TelegramError::NotConnected)?;

    // ...

    // Send the user's message as-is
    tmux.send_line(&session.tmux_session, None, message)  // Use original message
        .map_err(|e| TelegramError::TmuxError(e.to_string()))?;

    // ...
}
```

### Option B: Use `send_message_direct` as Default

The codebase already has a `send_message_direct` function (lines 1019-1053) that bypasses the orchestrator. Make this the default behavior.

### Option C: Fix `process_user_input` Semantics

If the User Agent is meant to modify/enhance user input (not generate responses), fix `process_user_input` to return the original message (possibly with modifications) rather than generating new content.

---

## Files Involved

| File | Lines | Purpose |
|------|-------|---------|
| `crates/commander-telegram/src/state.rs` | 960-1004 | Bug location - `send_message()` |
| `crates/commander-telegram/src/state.rs` | 1019-1053 | Correct behavior - `send_message_direct()` |
| `crates/commander-orchestrator/src/orchestrator.rs` | 76-99 | `process_user_input()` returns LLM response |
| `crates/commander-agent/src/user_agent/mod.rs` | 40-87 | User Agent system prompt with output formats |

---

## Immediate Workaround

Until fixed, users can avoid this bug by:
1. Compiling without the `agents` feature flag
2. Or ensuring the orchestrator is not initialized (no OPENROUTER_API_KEY)

---

## Testing the Fix

After applying the fix:

1. Enable `agents` feature
2. Set `OPENROUTER_API_KEY` to enable orchestrator
3. Connect to a Claude Code session via Telegram
4. Send a message
5. Verify the exact message appears in tmux (not an LLM response)
6. Verify Claude Code responds appropriately

---

*Research conducted by Claude Opus 4.5*
