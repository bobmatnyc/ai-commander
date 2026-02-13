# Telegram Group Chat Mode Research

**Date:** 2025-02-12
**Status:** Completed
**Researcher:** Claude Opus 4.6 (Research Agent)

## Executive Summary

Implementing a "Group Chat mode" where each ai-commander session appears as a separate user in a Telegram group is **not possible** with Telegram's API constraints. However, several viable alternatives exist that can achieve a similar multi-session experience:

1. **Forum Topics (Recommended)** - Each session gets its own topic in a supergroup
2. **Persona Simulation** - Single bot prefixes messages with session identifiers
3. **Multiple Bots** - One bot per session (complex to manage)

**Recommendation:** Implement Forum Topics approach as primary strategy, with persona simulation as fallback for non-supergroup chats.

---

## Research Findings

### 1. Telegram Bot API Capabilities

#### Can bots send messages "as" different users?
**No.** Telegram bots cannot impersonate individual users. Key constraints:
- Bots have a fixed identity (`first_name`, `username`) set at registration
- The `sender_chat` field exists but only applies to channel/group administration scenarios
- There is no per-message sender customization

#### Can bots create/manage group chats?
**Limited.** Bots can:
- Manage existing chats through administrator functions
- Promote/restrict members and manage permissions
- **Cannot** create new groups programmatically
- Users must add the bot to groups manually

#### Can bots customize message appearance per message?
**No.** Bots cannot:
- Change sender name per message
- Change avatar per message
- The profile photo can only be changed globally via `setMyProfilePhoto`

### 2. Forum Topics Feature (Best Solution)

Telegram's Forum Topics feature enables organized conversations within supergroups:

#### API Support
- `createForumTopic` - Create topics programmatically
- `editForumTopic` - Modify topic name/icon
- `closeForumTopic` / `reopenForumTopic` - Manage topic state
- `message_thread_id` parameter - Send messages to specific topics

#### Topic Properties
- Custom name (e.g., session name)
- Custom icon via `icon_custom_emoji_id` (emoji customization)
- Unique `message_thread_id` for message routing
- Topics can be pinned, closed, hidden

#### Requirements
- Chat must be a **supergroup** with forums enabled (`is_forum: true`)
- Bot must have appropriate admin permissions
- Premium emojis available for topic icons

### 3. teloxide Library Support

Current project uses teloxide v0.13.0, which **supports Forum Topics**:

```rust
// teloxide 0.13 includes:
message_thread_id: Option<ThreadId>  // In SendMessage and other payloads
```

Key supported features:
- `message_thread_id` parameter for sending to specific topics
- Forum topic event filters (created, edited, closed, reopened)
- Full Telegram Bot API 6.x+ compatibility

### 4. Current Implementation Analysis

#### TelegramState Structure (`state.rs`)
```rust
pub struct TelegramState {
    sessions: RwLock<HashMap<i64, UserSession>>,  // chat_id -> session
    tmux: Option<TmuxOrchestrator>,
    adapters: AdapterRegistry,
    store: StateStore,
    authorized_chats: RwLock<HashSet<i64>>,
    #[cfg(feature = "agents")]
    orchestrator: RwLock<Option<AgentOrchestrator>>,
}
```

**Current Model:** 1:1 mapping - one Telegram chat maps to one session at a time.

#### UserSession Structure (`session.rs`)
```rust
pub struct UserSession {
    pub chat_id: ChatId,
    pub project_path: String,
    pub project_name: String,
    pub tmux_session: String,
    pub response_buffer: Vec<String>,
    // ... polling/state fields
}
```

#### Key Extension Points
1. `send_message()` in `state.rs` - Modify to include topic routing
2. `poll_output_loop()` in `bot.rs` - Route responses to correct topics
3. `handle_message()` in `handlers.rs` - Detect topic context from incoming messages

---

## Recommended Approaches

### Approach 1: Forum Topics (Primary Recommendation)

**Concept:** Use Telegram supergroup forums where each session = one topic.

**User Experience:**
- User creates a supergroup and enables forums
- User adds bot to group as admin
- Each project/session appears as a separate topic
- Messages within topics route to correct session
- Bot responses appear in the relevant topic

**Architecture Changes:**

```
Current Model:
  ChatId (1:1) -> Session

New Model:
  GroupChatId + TopicThreadId -> Session

  HashMap<(ChatId, ThreadId), Session>
  or
  HashMap<ChatId, HashMap<ThreadId, Session>>
```

**Implementation Tasks:**

1. **New data structures:**
   ```rust
   pub struct GroupChatConfig {
       pub chat_id: ChatId,
       pub is_forum: bool,
       pub topic_sessions: HashMap<ThreadId, String>, // topic -> session_name
   }

   pub struct TopicSession {
       pub thread_id: ThreadId,
       pub session_name: String,
       pub project_path: String,
       pub tmux_session: String,
   }
   ```

2. **New commands:**
   - `/groupmode` - Enable group chat mode for current supergroup
   - `/addtopic <session>` - Create topic for a session
   - `/topics` - List topics and their sessions

3. **Modified message routing:**
   ```rust
   // In handle_message():
   let thread_id = msg.thread_id; // Telegram includes this for forum messages
   if let Some(tid) = thread_id {
       // Route to session mapped to this topic
       let session = get_session_for_topic(chat_id, tid);
   }
   ```

4. **Response routing:**
   ```rust
   // In poll_output_loop():
   bot.send_message(chat_id, &response)
       .message_thread_id(session.thread_id)  // Route to correct topic
       .await
   ```

**Pros:**
- Clean separation per session
- Native Telegram UX (topics are a first-class feature)
- Each topic can have custom name/icon
- Supports pinning, closing, organizing topics

**Cons:**
- Requires supergroup (not available in regular groups/DMs)
- User must enable forum mode manually
- Slightly more complex state management

---

### Approach 2: Persona Simulation (Fallback)

**Concept:** Single bot prefixes messages with session identifier.

**User Experience:**
- Messages appear as: `[project-name] Response from Claude...`
- Emoji or icon prefix per session
- User routes messages with `@session-name message` (already partially implemented!)

**Current Implementation Reference:**
```rust
// In handlers.rs handle_message():
if let Some(rest) = text.strip_prefix('@') {
    if let Some((alias, message)) = rest.split_once(' ') {
        // Route to session by alias
        match state.connect(msg.chat.id, alias).await {
            Ok((project_name, _tool_id)) => {
                // Send to specific session
            }
        }
    }
}
```

**Enhancement:**
```rust
// Response formatting:
fn format_session_response(session_name: &str, response: &str) -> String {
    let emoji = session_emoji(session_name);  // Generate consistent emoji
    format!("{} <b>[{}]</b>\n\n{}", emoji, session_name, response)
}

// Session emoji mapping:
fn session_emoji(session_name: &str) -> &'static str {
    let emojis = ["🔵", "🟢", "🟠", "🟣", "🔴", "🟡", "⚪", "🟤"];
    let hash = session_name.bytes().map(|b| b as usize).sum::<usize>();
    emojis[hash % emojis.len()]
}
```

**Pros:**
- Works in any chat (DM, group, supergroup)
- Simple implementation
- No Telegram API limitations
- Already partially implemented (`@alias` routing)

**Cons:**
- Not a "true" multi-user appearance
- All messages come from same bot identity
- Visual differentiation only through prefixes
- Can feel cluttered in long conversations

---

### Approach 3: Multiple Bots (Not Recommended)

**Concept:** Create separate bot for each session via BotFather.

**Architecture:**
- Each project registers with a unique bot token
- Bots are added to a shared group
- Each bot represents one session

**Pros:**
- True visual separation (different bot identities)
- Each bot can have unique name/avatar

**Cons:**
- **Extremely complex** token management
- Users must register bots manually
- Each bot needs separate BotFather setup
- Security concerns with multiple tokens
- Not scalable
- Against Telegram ToS if automated bot creation attempted

**Verdict:** Not recommended due to management complexity and scalability issues.

---

## Implementation Plan

### Phase 1: Persona Simulation Enhancement (Quick Win)

**Timeline:** 1-2 days
**Impact:** Immediate improvement for all users

**Files to modify:**
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs`

**Changes:**

1. **Add session context to responses** (`state.rs`):
   ```rust
   impl TelegramState {
       pub async fn poll_output_with_session(
           &self,
           chat_id: ChatId
       ) -> Result<(PollResult, Option<String>)> {
           // Return session name with poll result
       }
   }
   ```

2. **Format responses with session prefix** (`bot.rs`):
   ```rust
   async fn poll_output_loop(bot: Bot, state: Arc<TelegramState>) {
       // ...
       Ok(PollResult::Complete(response, message_id)) => {
           let formatted = format_session_response(&session_name, &response);
           bot.send_message(ChatId(chat_id), &formatted)
               .parse_mode(ParseMode::Html)
               .await
       }
   }
   ```

3. **Improve `@alias` command feedback** (`handlers.rs`):
   - Clear visual indication of active session
   - `/active` command to show current session

### Phase 2: Forum Topics Support (Full Solution)

**Timeline:** 3-5 days
**Impact:** Full group chat experience

**Files to modify:**
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs` (new data structures)
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/session.rs` (topic tracking)
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs` (new commands)
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs` (topic-aware routing)

**New commands:**
```rust
#[derive(BotCommands, Clone, Debug)]
pub enum Command {
    // Existing commands...

    #[command(description = "Enable group mode for this supergroup")]
    GroupMode,

    #[command(description = "Create topic for session: /topic <session>")]
    Topic(String),

    #[command(description = "List topics and their sessions")]
    Topics,
}
```

**Data structure changes:**
```rust
// New: Topic-to-session mapping
pub struct TopicConfig {
    pub thread_id: i32,
    pub session_name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// Modified: Support both 1:1 and group modes
pub enum ChatMode {
    Single(UserSession),           // 1:1 chat
    Group(HashMap<i32, UserSession>),  // Forum topics
}
```

**Key implementation details:**

1. **Detect forum messages:**
   ```rust
   // teloxide provides thread_id on Message
   let thread_id = msg.thread_id;
   ```

2. **Create topics programmatically:**
   ```rust
   bot.create_forum_topic(
       chat_id,
       &session_name,  // Topic name
   )
   .icon_custom_emoji_id(custom_emoji_id)
   .await
   ```

3. **Route responses to topics:**
   ```rust
   bot.send_message(chat_id, &response)
       .message_thread_id(ThreadId(topic_id))
       .await
   ```

---

## Code Snippets: Current Patterns to Extend

### Message Sending Pattern
```rust
// Current pattern in state.rs:
pub async fn send_message(&self, chat_id: ChatId, message: &str, message_id: Option<MessageId>) -> Result<()> {
    let tmux = self.tmux.as_ref().ok_or_else(|| {
        TelegramError::TmuxError("tmux not available".to_string())
    })?;

    // ... process message ...

    tmux.send_line(&session.tmux_session, None, &processed_message)
        .map_err(|e| TelegramError::TmuxError(e.to_string()))?;
}

// Extension for topic support:
pub async fn send_message_to_topic(
    &self,
    chat_id: ChatId,
    thread_id: ThreadId,  // NEW
    message: &str,
    message_id: Option<MessageId>
) -> Result<()> {
    // Lookup session by (chat_id, thread_id) instead of just chat_id
}
```

### Response Polling Pattern
```rust
// Current pattern in bot.rs:
async fn poll_output_loop(bot: Bot, state: Arc<TelegramState>) {
    // ...
    Ok(PollResult::Complete(response, message_id)) => {
        let send_result = if let Some(msg_id) = message_id {
            bot.send_message(ChatId(chat_id), &response)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await
        } else {
            bot.send_message(ChatId(chat_id), &response).await
        };
    }
}

// Extension for topic support:
Ok(PollResult::Complete(response, message_id, thread_id)) => {  // NEW: thread_id
    let mut req = bot.send_message(ChatId(chat_id), &response);

    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);  // Route to topic
    }

    if let Some(msg_id) = message_id {
        req = req.reply_parameters(ReplyParameters::new(msg_id));
    }

    req.await
}
```

### Session Lookup Pattern
```rust
// Current pattern:
pub async fn has_session(&self, chat_id: ChatId) -> bool {
    let sessions = self.sessions.read().await;
    sessions.contains_key(&chat_id.0)
}

// Extension for topic support:
pub async fn has_topic_session(&self, chat_id: ChatId, thread_id: ThreadId) -> bool {
    let sessions = self.sessions.read().await;
    sessions
        .get(&chat_id.0)
        .map(|chat_sessions| chat_sessions.contains_key(&thread_id.0))
        .unwrap_or(false)
}
```

---

## Appendix: Telegram Bot API Reference

### Relevant API Methods

| Method | Description |
|--------|-------------|
| `sendMessage` | Send message with optional `message_thread_id` |
| `createForumTopic` | Create new topic in forum |
| `editForumTopic` | Edit topic name/icon |
| `closeForumTopic` | Close a topic |
| `reopenForumTopic` | Reopen a closed topic |
| `deleteForumTopic` | Delete a topic |
| `getForumTopicIconStickers` | Get available topic icons |

### Message Threading Fields

| Field | Type | Description |
|-------|------|-------------|
| `message_thread_id` | `Integer` | Thread/topic ID for forum messages |
| `is_topic_message` | `Boolean` | True if message is in a topic |
| `forum_topic_created` | `Object` | Service message when topic created |
| `forum_topic_edited` | `Object` | Service message when topic edited |

---

## Conclusion

**Primary Recommendation:** Implement Forum Topics support as the main group chat solution. This provides the cleanest user experience with native Telegram UI support.

**Secondary Recommendation:** Enhance persona simulation as fallback for non-supergroup chats, leveraging the existing `@alias` routing mechanism.

**Not Recommended:** Multiple bots approach due to management complexity and scalability issues.

The current teloxide v0.13 in the project fully supports Forum Topics via `message_thread_id`, making implementation straightforward.
