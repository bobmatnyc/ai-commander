# @alias Message Routing Investigation

**Date:** 2026-02-02
**Researcher:** Claude (Research Agent)
**Issue:** User reports "@alias messages don't seem to address the right connection"

---

## Executive Summary

**Finding: @alias message routing is NOT IMPLEMENTED in commander-telegram.**

The reported issue is not a bug in routing logic - the feature simply does not exist. Users may be expecting @alias syntax to route messages to different projects, but the current implementation:

1. **Telegram bot**: Only supports a single connected session per chat. There is no @alias parsing or multi-session routing.
2. **CLI REPL**: Has placeholder @mention parsing (lines 286-294 in `repl.rs`) with a comment "For now, just echo - actual implementation in Phase 7"

---

## Research Questions Answered

### 1. How does @alias message routing work?

**Answer: It doesn't exist.**

In the Telegram bot (`handlers.rs`), the `handle_message` function (lines 549-586):
- Checks if the user has an active session
- Sends the message directly to the connected tmux session
- There is NO parsing for `@` prefix or alias syntax
- There is NO multi-session routing capability

```rust
// handlers.rs:549-586 - Current implementation
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let Some(text) = msg.text() else {
        return Ok(());
    };

    // Check if connected - ONLY ONE SESSION PER CHAT
    if !state.has_session(msg.chat.id).await {
        bot.send_message(
            msg.chat.id,
            "Not connected to any project.\n\nUse /connect <project> to connect first.",
        )
        .await?;
        return Ok(());
    }

    // Send message to the SINGLE connected project
    match state.send_message(msg.chat.id, text, Some(msg.id)).await {
        // ...
    }
}
```

### 2. Where is the @alias syntax parsed and handled?

**Answer: Nowhere in commander-telegram.**

The only @mention handling exists in the CLI REPL (`repl.rs` lines 286-294):

```rust
} else if let Some(stripped) = input.strip_prefix('@') {
    // @mention syntax - treat as send to project
    let parts: Vec<&str> = stripped.splitn(2, ' ').collect();
    if parts.len() == 2 {
        // For now, just echo - actual implementation in Phase 7
        ReplCommand::Send(input.to_string())
    } else {
        ReplCommand::Text(input.to_string())
    }
}
```

This code:
- Detects `@` prefix
- Splits into alias and message parts
- BUT just sends the entire raw input (including `@alias`) as the message
- Does NOT route to a different project

### 3. How does it look up the target session/project?

**Answer: It doesn't.**

The session lookup in `state.rs` is purely based on `chat_id`:

```rust
// state.rs - Session lookup is ALWAYS by chat_id
let session = sessions
    .get_mut(&chat_id.0)
    .ok_or(TelegramError::NotConnected)?;
```

There is no mechanism to:
- Parse alias from message
- Look up project by alias
- Route to a different tmux session

### 4. Is there a bug in the routing logic?

**Answer: No - the feature is unimplemented.**

The user expectation and reality mismatch:

| User Expects | Reality |
|--------------|---------|
| `@project1 message` routes to project1 | Message sent as-is to currently connected session |
| Multiple projects addressable | Only ONE session per chat |
| Alias resolution | No alias parsing exists |

---

## Code Flow Analysis

### Current Message Flow (Telegram)

```
User sends: "@myproject hello"
      |
      v
handle_message() receives text "@myproject hello"
      |
      v
has_session(chat_id)?
      |
      +--> NO: "Not connected" error
      |
      +--> YES: state.send_message(chat_id, "@myproject hello")
                      |
                      v
              Sends LITERAL "@myproject hello" to
              the ONE connected tmux session
```

### State Structure (One Session Per Chat)

```rust
pub struct TelegramState {
    // KEY LIMITATION: One session per chat_id
    sessions: RwLock<HashMap<i64, UserSession>>,
    // ...
}
```

Each Telegram chat can only be connected to ONE project at a time. To switch projects, users must:
1. `/disconnect` from current
2. `/connect <other_project>`

---

## Root Cause

The user expectation of `@alias` routing likely comes from:

1. **CLI REPL code visibility** - The parsing exists but is non-functional
2. **Common convention** - Slack/Discord style `@mention` routing
3. **Documentation gap** - No clear docs on multi-project handling

---

## Recommended Fix

### Option 1: Implement @alias Routing (Feature Addition)

Add multi-session support to commander-telegram:

1. **Parse @alias prefix** in `handle_message`:
   ```rust
   // Detect @alias prefix
   if let Some(rest) = text.strip_prefix('@') {
       if let Some((alias, message)) = rest.split_once(' ') {
           // Route to different project
           return self.send_to_project(alias, message).await;
       }
   }
   ```

2. **Add multi-session support** to `TelegramState`:
   ```rust
   // Change from single session to multi-session
   sessions: RwLock<HashMap<i64, HashMap<String, UserSession>>>,
   // Or: maintain "default" session + allow @alias routing
   ```

3. **Add project alias lookup** method:
   ```rust
   async fn resolve_alias(&self, alias: &str) -> Option<&UserSession> {
       // Look up project by name/alias
       // Return session if connected
   }
   ```

### Option 2: Document Current Behavior (Documentation)

If multi-session routing is not desired:

1. Update `/help` command to clarify single-session model
2. Add documentation explaining `/connect` switches context
3. Suggest workflow: "Use `/connect project2` to switch projects"

### Option 3: Quick Alias Switch (Hybrid)

Implement `@alias` as a shortcut for `/connect`:

```rust
// @project1 hello = /connect project1 + send "hello"
if let Some(rest) = text.strip_prefix('@') {
    if let Some((alias, message)) = rest.split_once(' ') {
        self.connect(chat_id, alias).await?;
        return self.send_message(chat_id, message).await;
    }
}
```

---

## Files Requiring Changes

If implementing @alias routing:

| File | Change |
|------|--------|
| `handlers.rs` | Add @alias parsing in `handle_message` |
| `state.rs` | Add `resolve_alias()` method, possibly multi-session support |
| `session.rs` | May need `project_alias` field |

---

## Conclusion

**The @alias routing issue is a missing feature, not a bug.**

The user's expectation that `@alias` would route to a specific project is reasonable but unsupported. The current implementation:
- Supports ONE session per chat
- Sends all messages verbatim to that session
- Has no alias parsing or multi-project routing

### Recommendation

Implement Option 3 (Quick Alias Switch) as a minimal viable solution:
- Parse `@alias message` syntax
- Auto-connect to the specified project
- Send the message after connection
- Provides expected UX with minimal code changes

---

## References

### Files Analyzed
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/session.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-cli/src/repl.rs`

### Related Documentation
- `/Users/masa/Projects/ai-commander/docs/research/telegram-bot-architecture-2026-02-01.md`
- `/Users/masa/Projects/ai-commander/docs/ROADMAP.md`

---

*Research completed: 2026-02-02*
