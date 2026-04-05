# Event-Driven Adapter Integration Plan (Telegram Bot, Phase 2)

**Date**: 2026-04-05
**Scope**: Map integration points for `EventDrivenAdapter` / `MpmSdkAdapter` into the Telegram bot. Design only — no implementation.

---

## 1. `UserSession` anatomy (tmux-centric)

Location: `crates/commander-telegram/src/session.rs:11-61`. The struct is **stored as** `HashMap<i64, UserSession>` (keyed by `chat_id.0`) in `TelegramState.sessions` (`state.rs:257`).

| Field | Category | Notes |
|---|---|---|
| `chat_id`, `project_path`, `project_name`, `thread_id`, `is_private_chat` | **General** | Apply to any adapter |
| `tmux_session: String` | **Terminal-specific** | Required non-optional |
| `response_buffer: Vec<String>`, `last_output: String`, `last_output_time` | **Terminal-specific** | Accumulator for tmux-captured lines |
| `pending_query`, `pending_message_id`, `original_message_id`, `is_waiting`, `send_time` | **General** | Request/reply bookkeeping — reusable |
| `last_progress_line_count`, `last_incremental_summary_line_count`, `chars_since_last_summary`, `completion_detected_at`, `stale_poll_count`, `is_summarizing` | **Terminal-specific** | Poll-loop idle/progress heuristics |
| `worktree_info` | **General** | Valid for both adapters |
| `daemon_session_id: Option<String>` | **Terminal-specific** | commander-daemon session ID |
| `adapter_type: String` | **General** | Already present (defaults `"claude-code"`) — would become discriminator |
| `at_session_name` | **General** | Routing state |

**Verdict**: `UserSession` is strongly tmux-centric but *not hopelessly so*. The reply-threading, identity, and routing fields are reusable. The output-capture and idle-detection fields are dead weight for event-driven. `tmux_session` is the hardest field because it is non-optional and used unconditionally throughout `state.rs`. A parallel `Session` enum (terminal vs event-driven) is cleaner than adding a `Box<dyn Any>` to `UserSession`.

---

## 2. Connection flow (terminal today)

`connect_new` (`state.rs:2026-2071`) resolves the adapter alias → creates a `commander_models::Project` → saves it via `store.save_project` → calls `connect`.

`connect` (`state.rs:644-794`) for terminal adapters does:

1. Load projects from store, look up by name.
2. Read `config.tool` → `tool_id` (defaults `"claude-code"`).
3. **Retrieve adapter** via `self.adapters.get(&tool_id)` (registry lookup, `state.rs:681`) — this is the *terminal* `RuntimeAdapter` registry (`AdapterRegistry.adapters`), **not** the `event_driven` registry.
4. If tmux session doesn't exist → `tmux.create_session_in_dir()` + `tmux.send_line(&full_cmd)` to launch the tool.
5. Create `UserSession` with `tmux_session = format!("commander-{}", project.name)`.
6. Optionally register with commander-daemon.
7. Insert into `sessions` map, persist to disk via `save_sessions`.

**Key observation**: the `event_driven` registry side is never touched. `connect` hard-codes a tmux spawn. For mpm-sdk there is no persistent subprocess to spawn at `connect` time — `start_session` should be deferred until the first user message (or called eagerly with an empty priming prompt).

---

## 3. User → Session message path

`handlers.rs::handle_message` (`handlers.rs:1283-1475`) is the entry point for non-command text:

1. Auth check (`state.is_authorized`, line 1289).
2. Thread routing: if `msg.thread_id.is_some()` → `handle_topic_message` (line 1309).
3. `@alias` / reply-chain routing (lines 1315-1443) → `state.send_to_named_session`.
4. Normal path: `state.has_session(msg.chat.id)` check → **`state.send_message(msg.chat.id, &text, Some(msg.id))`** (line 1460).

`TelegramState::send_message` (`state.rs:1304-1350`) then:

- Looks up session by `chat_id.0` (line 1311).
- Captures initial tmux output for diffing (line 1315).
- Sends via daemon **if** `daemon_session_id` is set, else **`tmux.send_line(&session.tmux_session, None, message)`** (line 1325).
- Calls `session.start_response_collection(...)` to arm the poll loop.

**Target-session resolution**: always by `chat_id` (single session per chat), occasionally by `(chat_id, thread_id)` for forum topics, and by `project_name` for `@alias` dispatch.

**Other `tmux.send_line` call sites for user→session routing**:
- `state.rs:1041` (message to topic)
- `state.rs:1325` `send_message`
- `state.rs:1377` `send_message_direct`
- `state.rs:1503` `send_to_named_session`
- `state.rs:694, 870, 1994` (launch commands during `connect*`)

---

## 4. Session → User response path

The Telegram bot runs its **own** poll loop — it does **not** use `commander-runtime::OutputPoller`. That poller is used by the CLI/TUI only.

`bot.rs::poll_output_loop` (`bot.rs:423-740+`):

- Interval-ticked loop over `state.get_waiting_sessions()` (`state.rs:1806`).
- For each `(session_key, chat_id, thread_id)`: calls `state.poll_output(chat_id)` or `state.poll_topic_output(chat_id, thread_id)`.
- Matches on `PollResult` (`state.rs:31-46`): `Progress`, `IncrementalSummary`, `ProgressiveSummary`, `Summarizing`, **`Complete(String, Option<MessageId>, Option<ThreadId>)`**, `SelectorDetected`, `NoOutput`.
- Routes replies to Telegram via `bot.send_message(chat_id, ...)` with optional `message_thread_id(tid)` and `reply_parameters(pending_message_id)`.

`poll_output` (`state.rs:1564-1795`) internally:
- `tmux.capture_output` → diffs against `session.last_output` → appends new lines.
- Applies 5-min inactivity timeout, selector detection, idle heuristics, summarization triggers.
- Returns `PollResult::Complete` when idle+prompt detected.

**How it knows where to route**: the `UserSession` stores `chat_id`, `thread_id`, `pending_message_id`, `original_message_id`. These are returned in `PollResult::Complete` and used by `bot.rs` to construct the Telegram reply.

---

## 5. Prior art: `spawn_agent_with_streaming` (handlers.rs:2691-2812)

**Call sites**:
- `handle_spawn` (`/spawn <agent> <prompt>`, line 2655)
- `handle_ask` (`/ask <question>` → spawns `research` agent, line 2687)

**Architecture**:
1. `mpm_sdk::MpmClient::discover()` — discovers `.claude-mpm` installation (similar to `MpmSdkAdapter`).
2. Sends initial `"Spawning agent..."` status message → retains `status_msg.id` for **message editing**.
3. Creates `tokio::sync::mpsc::channel::<mpm_sdk::AgentEvent>(64)`.
4. Spawns background task: `client.run_streaming(agent_id, prompt, tx).await`.
5. Loop on `rx.recv()`, matching `AgentEvent::{Text, ToolUse, Complete, Error}`:
   - `Text(chunk)` → accumulates into `String`, **edits status message every 2 seconds** via `bot.edit_message_text` (rate-limited).
   - `Complete(result)` → deletes status message, sends final result with cost/duration footer.
   - `Error(e)` → sends error message.

**Message editing/rate-limiting**: `edit_interval = Duration::from_secs(2)`, bounded by `truncate_for_telegram(&accumulated, 3500)` for live previews, 4000 for final.

**Reusability assessment**: the **event-consumption loop is highly reusable** for mpm-sdk integration. The key difference:
- `spawn_agent_with_streaming` is a **one-shot** operation — no session state is stored in `TelegramState`, no `UserSession` is created, the stream dies with the function.
- For Phase 2 we need **persistent** event streams: the `SessionHandle` survives across turns, and follow-up messages invoke `adapter.send(&handle, message)` which returns a *new* `EventStream` for that turn.

The 2-second debounced edit cadence, the accumulate-and-truncate pattern, and the `Complete`/`Error` handling are all directly portable.

Note: `mpm_sdk::AgentEvent` (used here) and `commander_adapters::RuntimeEvent` (emitted by `EventDrivenAdapter`) are parallel enums with the same shape but different types. `MpmSdkAdapter` internally translates `AgentEvent` → `RuntimeEvent`. Downstream consumers should only see `RuntimeEvent`.

---

## 6. Session lifecycle for event-driven

| Aspect | Terminal adapter | Event-driven (mpm-sdk) |
|---|---|---|
| Resource lifetime | Long-lived tmux process until `/disconnect` | `SessionHandle` persists; each turn spawns a fresh subprocess |
| Startup cost | tmux create + launch cmd on `connect` | Zero on `connect`; `start_session` called on first message |
| State holder | `tmux_session: String` name | `SessionHandle { id: String }` |
| Cleanup | `tmux.kill_session` on `/disconnect` | `adapter.stop(handle)` on `/disconnect` |

**Natural mapping**: Add `session_handle: Option<SessionHandle>` to `UserSession` (or split into a variant). Populate on first `send`, reuse on subsequent `send`s (triggering `--resume` behavior via mpm-sdk). Call `adapter.stop(handle)` in `TelegramState::disconnect` (`state.rs:797`).

---

## 7. Session persistence

Persistence lives in `state.rs:123-151` (`load_persisted_sessions` / `save_persisted_sessions`) writing to `~/.commander/telegram_sessions.json`. `PersistedSession` (`session.rs:76-93`) stores `chat_id`, `project_path`, `project_name`, `tmux_session`, `thread_id`, `worktree_info`, `created_at`, `last_activity`. Valid 24h (`is_valid`, `session.rs:125`).

On restart, `load_sessions` (`state.rs:2095+`) validates tmux session still exists. For mpm-sdk, a `SessionHandle.id` could be persisted similarly, but reattaching to a half-dead mpm-sdk subprocess is non-trivial.

**Recommendation**: **Defer event-driven persistence to Phase 3.** In Phase 2, mpm-sdk sessions are in-memory only — they are lost on restart. Users would need to re-`/connect` after a bot restart (acceptable MVP behavior since each turn creates a fresh subprocess internally anyway).

---

## 8. Recommended integration approach

**Flag field + routing fork** (simplest MVP). Avoid a full `Session` enum refactor for Phase 2. Concretely:

1. Add `event_handle: Option<SessionHandle>` to `UserSession`. Keep `tmux_session` populated with a dummy value (e.g. `"event-driven-{project_name}"`) so existing reads don't panic.
2. Use `session.adapter_type` as the discriminator. When `adapter_type == "mpm-sdk"` (or `registry.is_event_driven(&tool_id)`), take the event-driven path.
3. In `connect`, when the adapter is event-driven: **skip** tmux spawn, **skip** launch command, insert `UserSession` with `event_handle = None`.
4. In `send_message`, fork on `adapter_type`: event-driven path calls `adapter.start_session` (first turn) or `adapter.send` (subsequent turns), spawns a background task that consumes the `EventStream` and pushes events directly to Telegram (mirroring `spawn_agent_with_streaming`).
5. **Bypass the poll loop entirely** for event-driven sessions. `get_waiting_sessions` should filter them out. The event-stream task owns the entire response lifecycle.
6. In `disconnect`, if `event_handle.is_some()`, call `adapter.stop(handle)`.

This avoids modifying the poll loop, `PollResult`, `poll_output`, and `bot.rs` entirely. The event-driven flow runs as a parallel background task per turn.

---

## 9. Code locations requiring modification (Phase 2)

| File:Line | Change |
|---|---|
| `session.rs:11-61` | Add `event_handle: Option<SessionHandle>` field + update constructors |
| `state.rs:261` | Ensure `AdapterRegistry` exposes `get_event_driven` (already does per `registry.rs:93`) |
| `state.rs:644-794` (`connect`) | Branch on `registry.is_event_driven(&tool_id)`; skip tmux spawn for event-driven |
| `state.rs:1304-1350` (`send_message`) | Fork routing: event-driven → spawn background event-consumer task |
| `state.rs:797-813` (`disconnect`) | Call `adapter.stop(handle)` if `event_handle.is_some()` |
| `state.rs:1806+` (`get_waiting_sessions`) | Exclude event-driven sessions from poll loop |
| `state.rs:2039` (`connect_new`) | Accept `"mpm-sdk"` alias (resolution already works via `adapters.resolve`) |
| `handlers.rs:2691-2812` | **Extract** event-consumption loop into a reusable helper taking `EventStream` + `chat_id` + `message_id`; reuse from new `send_message` event-driven path |
| Session persistence (`state.rs:123-151`, `session.rs:76-152`) | **No change in Phase 2** (deferred to Phase 3) |

---

## 10. Scope recommendations

**MVP (Phase 2)**:
- `/connect-new <path> mpm-sdk <name>` creates a project and a `UserSession` with `adapter_type="mpm-sdk"` and no tmux session.
- First user message calls `adapter.start_session`, streams events to Telegram via a lifted version of `spawn_agent_with_streaming`'s consumer loop.
- Subsequent messages call `adapter.send(&handle, text)` and stream identically.
- `/disconnect` calls `adapter.stop(handle)`.
- In-memory only: sessions lost on bot restart.

**Deferrable (Phase 3+)**:
- Persistence of `SessionHandle` in `PersistedSession`.
- Restoration of event-driven sessions after restart.
- Topic/group-mode support for event-driven (`connect_topic`, `handle_topic_message`).
- `@alias` reply routing to event-driven sessions.
- Integration with commander-daemon.
- Unifying `AgentEvent` and `RuntimeEvent` (currently parallel enums).

---

## Key file paths

- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/session.rs` — `UserSession`, `PersistedSession`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs` — `TelegramState`, `connect`, `connect_new`, `send_message`, `poll_output`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs` — `handle_message` (line 1283), `spawn_agent_with_streaming` (line 2691)
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs` — `poll_output_loop` (line 423)
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/event_driven.rs` — trait + `SessionHandle` + `RuntimeEvent`
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/mpm_sdk.rs` — `MpmSdkAdapter` impl
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/registry.rs` — `is_event_driven`, `get_event_driven`
