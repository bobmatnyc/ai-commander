# mpm-sdk Crate and Telegram MPM Integration — Architecture Research

**Date:** 2026-03-26
**Scope:** ai-commander workspace — existing crate structure, Telegram handler pattern, IPC
architecture, and recommended design for `crates/mpm-sdk` plus new Telegram MPM commands.

---

## 1. Existing Crate Inventory

```
crates/
  ai-commander          — TUI binary (ratatui); main interactive CLI
  commander-adapters    — RuntimeAdapter trait + concrete adapters (claude-code, mpm, shell)
  commander-agent       — Agent trait, UserAgent, SessionAgent, LLM client (OpenRouter)
  commander-api         — Axum HTTP API layer
  commander-core        — Shared utilities: output_filter, notification_parser, summarizer,
                          config paths, change_detector, onboarding
  commander-daemon      — Long-running daemon: JSON-RPC IPC server over Unix socket,
                          session lifecycle, memory monitoring, pairing
  commander-events      — Event bus types shared across crates
  commander-gui         — Tauri desktop app (Svelte frontend)
  commander-memory      — MemoryStore trait + persistence (vector search)
  commander-models      — Shared domain types (projects, sessions)
  commander-orchestrator — AgentOrchestrator: ties UserAgent + SessionAgents together
  commander-persistence  — StateStore: JSON file persistence of projects/sessions
  commander-runtime     — Runtime process launching and monitoring
  commander-telegram    — Telegram bot (teloxide polling + webhook); Rust binary
  commander-tmux        — TmuxOrchestrator: creates/reads tmux sessions
  commander-work        — Work item / task tracking types
```

### Dependency Relationships Relevant to mpm-sdk

```
commander-telegram
  -> commander-core       (output filtering, config paths, summarizer)
  -> commander-adapters   (AdapterRegistry, MpmAdapter, RuntimeAdapter)
  -> commander-tmux       (TmuxOrchestrator — session list + capture)
  -> commander-persistence (StateStore)
  -> commander-models
  [optional] commander-orchestrator (feature = "agents")
             -> commander-agent (Agent trait, UserAgent, SessionAgent)
             -> commander-memory

commander-daemon
  -> commander-orchestrator
  -> commander-agent
  -> commander-memory
  -> commander-tmux
  -> commander-persistence
  -> commander-core
```

A new `crates/mpm-sdk` crate would sit between `commander-adapters` (which already has
`MpmAdapter` for output-state detection) and the Telegram/GUI layers, providing headless
spawn + result collection over the `claude-mpm` CLI process.

---

## 2. How Messages Flow: Telegram → tmux → Claude

The existing flow for non-MPM sessions:

1. User sends Telegram message to bot.
2. `handle_message` in `handlers.rs` is called (dispatched by `dptree` in `bot.rs`).
3. The handler calls `state.send_to_session(chat_id, text)` which writes the text to the
   active tmux session via `TmuxOrchestrator`.
4. A background task (`poll_output_loop`) runs every 500 ms, calling
   `state.poll_output(chat_id)` which:
   - Captures the current tmux pane content.
   - Uses `commander_core::is_claude_ready` / `is_mpm_ready` (from `output_filter.rs`) to
     detect when the AI has finished responding.
   - Buffers lines, applies `clean_response` / `find_new_lines`, and optionally summarizes
     with OpenRouter.
   - Returns `PollResult::Complete(text, ...)` when done.
5. `bot.rs` sends the completed text back to the Telegram chat.

For MPM, `UserSession.adapter_type` is set to `"mpm"` and `is_mpm_ready` is used instead
of `is_claude_ready` to detect prompt readiness.

**Key insight:** The current system has no concept of spawning an MPM agent headlessly and
waiting for its result. Everything goes through tmux capture. `mpm-sdk` would add a direct
process-spawn path — no tmux pane required.

---

## 3. IPC Architecture (commander-daemon)

The daemon exposes a **JSON-RPC 2.0 protocol over a Unix domain socket**
(`runtime_state_dir()/daemon.sock`).

### Protocol methods (from `ipc/protocol.rs`)

```
session.create   — SessionCreateParams { project_path, adapter, name }
session.list     — returns Vec<SessionInfo>
session.get      — returns SessionInfo
session.terminate
session.send     — send text to a session
pairing.generate
pairing.validate
status.health
status.memory
daemon.stop / daemon.restart
```

### Client pattern (from `commander-telegram/src/ipc_client.rs`)

`DaemonClient` intentionally does NOT depend on `commander-daemon` to avoid pulling in
heavy transitive deps. It reimplements only the minimal JSON-RPC types needed:

```rust
pub struct DaemonClient {
    socket_path: PathBuf,
    next_id: AtomicU64,
}

impl DaemonClient {
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        // Opens UnixStream, writes one newline-delimited JSON-RPC request,
        // reads one newline-delimited response, closes connection.
        // Each call is a fresh connection — no pooling.
    }
}
```

**mpm-sdk should follow the same "self-contained client" pattern** if it ever needs to talk
to the daemon. For its primary use-case (spawning an MPM subprocess directly) it does not
need the daemon at all.

---

## 4. commander-core: output_filter and notification_parser

### output_filter.rs — what it provides

```rust
pub fn is_ui_noise(line: &str) -> bool       // filter Claude Code terminal noise
pub fn is_claude_ready(output: &str) -> bool // detect Claude Code prompt ready
pub fn is_mpm_ready(output: &str) -> bool    // detect MPM prompt ready
pub fn clean_response(lines: &[String]) -> String // strip noise, produce clean text
pub fn find_new_lines(old: &str, new: &str) -> Vec<String>
pub fn detect_adapter(output: &str) -> Adapter  // Claude | Shell | Unknown
pub fn detect_selector(output: &str) -> Option<SelectorPrompt>
```

The `Adapter` enum (`Claude`, `Shell`, `Unknown`) and `RuntimeState` (in
`commander-adapters::traits`) already have hooks for MPM.

### notification_parser.rs — what it provides

Parses MPM timer notifications like:
```
[timer] 1 new session(s) waiting for input:
   @izzie-33 - masa@host:/path (main*?) [model|Claude MPM|70%]
```

Into `ParsedSessionStatus { name, path, branch, git_status, model, framework, context_usage }`.

**mpm-sdk will need similar parsing** for:
- Detecting when a spawned MPM agent completes a task.
- Extracting structured results from MPM output streams.
- Identifying error states vs. normal completion.

Recommended: reuse `commander-core` as a dependency in `mpm-sdk` rather than duplicating
these parsers. Add an `mpm_output_parser` module to `commander-core` or directly to
`mpm-sdk` for MPM-specific result parsing.

---

## 5. How to Add a New Crate to the Workspace

The workspace uses `members = ["crates/*"]` with `resolver = "2"` (Cargo.toml line 2-3),
so adding `crates/mpm-sdk/` is automatically discovered. No root Cargo.toml edit needed.

**Steps:**

```bash
mkdir -p crates/mpm-sdk/src
```

Create `crates/mpm-sdk/Cargo.toml`:

```toml
[package]
name = "mpm-sdk"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Headless SDK for spawning and managing Claude MPM agents"

[dependencies]
# Shared parsing and config utilities
commander-core    = { path = "../commander-core" }
commander-adapters = { path = "../commander-adapters" }   # MpmAdapter, RuntimeAdapter

# Async runtime
tokio = { workspace = true }
futures = { workspace = true }

# Serialization
serde       = { workspace = true }
serde_json  = { workspace = true }

# Logging / errors
tracing     = { workspace = true }
thiserror   = { workspace = true }

# Process management
# (tokio::process covers Command; no additional dep needed)

[dev-dependencies]
tempfile = { workspace = true }
tokio    = { workspace = true, features = ["test-util"] }
```

Then add `mpm-sdk` as a dependency wherever it is used:

```toml
# In commander-telegram/Cargo.toml:
mpm-sdk = { path = "../mpm-sdk" }
```

---

## 6. Handler Pattern for Adding New Telegram Commands

### Step 1 — Add variant to the `Command` enum (handlers.rs)

```rust
#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
pub enum Command {
    // ... existing variants ...

    #[command(description = "List available MPM agents")]
    Agents,

    #[command(description = "Spawn an MPM agent: /spawn <agent> <task>")]
    Spawn(String),

    #[command(description = "MPM system status")]
    Mpm,
}
```

### Step 2 — Write the handler function (handlers.rs or a new mpm_handlers.rs module)

```rust
pub async fn handle_agents(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    if !state.is_authorized(msg.chat.id.0).await {
        // standard auth check pattern used by every existing handler
        bot.send_message(msg.chat.id, "Not authorized. Use /pair first.").await?;
        return Ok(());
    }
    typing(&bot, msg.chat.id, None).await;   // cosmetic typing indicator

    // call mpm-sdk
    let agents = mpm_sdk::list_agents().await...;
    bot.send_message(msg.chat.id, format_agents(agents))
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}
```

### Step 3 — Add arm to `handle_command` dispatch (handlers.rs, bottom)

```rust
pub async fn handle_command(bot: Bot, msg: Message, cmd: Command, state: Arc<TelegramState>)
    -> ResponseResult<()>
{
    match cmd {
        // ... existing arms ...
        Command::Agents         => handle_agents(bot, msg, state).await,
        Command::Spawn(args)    => handle_spawn(bot, msg, state, args).await,
        Command::Mpm            => handle_mpm_status(bot, msg, state).await,
    }
}
```

### Step 4 — Register commands with Telegram (bot.rs, near set_my_commands call)

The existing bot already calls `Command::bot_commands()` (teloxide derive macro). Because
the new variants are in the same enum the registration is automatic.

---

## 7. Recommended mpm-sdk API Surface

### Error type

```rust
// crates/mpm-sdk/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum MpmError {
    #[error("MPM process failed to start: {0}")]
    SpawnFailed(String),
    #[error("MPM agent timed out after {0}s")]
    Timeout(u64),
    #[error("MPM returned an error: {0}")]
    AgentError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MpmError>;
```

### Core types

```rust
// crates/mpm-sdk/src/types.rs

/// A registered MPM agent (from `claude-mpm list-agents` or equivalent).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
}

/// A spawned agent task in flight.
pub struct AgentTask {
    pub id: String,
    pub agent_id: String,
    pub prompt: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Running,
    Complete,
    Error(String),
}

/// The completed result of an agent task.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub task_id: String,
    pub agent_id: String,
    pub output: String,                   // cleaned agent output
    pub artifacts: Vec<Artifact>,         // files written, code blocks, etc.
    pub context_usage: Option<u8>,        // % of context window consumed
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub content: String,
    pub path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone)]
pub enum ArtifactKind { Code, File, Text }
```

### Client trait and implementation

```rust
// crates/mpm-sdk/src/client.rs

use async_trait::async_trait;
use tokio::sync::mpsc;

/// Streaming event from a running agent.
#[derive(Debug)]
pub enum AgentEvent {
    /// A chunk of output text.
    Output(String),
    /// Agent completed successfully.
    Done(AgentResult),
    /// Agent failed.
    Error(MpmError),
}

/// Core SDK trait — testable via mock implementations.
#[async_trait]
pub trait MpmClient: Send + Sync {
    /// List agents available in this MPM installation.
    async fn list_agents(&self) -> Result<Vec<AgentInfo>>;

    /// Spawn an agent with a task and collect the full result.
    async fn run_agent(&self, agent_id: &str, prompt: &str) -> Result<AgentResult>;

    /// Spawn an agent and stream events back through a channel.
    async fn run_agent_streaming(
        &self,
        agent_id: &str,
        prompt: &str,
        tx: mpsc::Sender<AgentEvent>,
    ) -> Result<()>;

    /// Get MPM system status (version, installed agents, health).
    async fn status(&self) -> Result<MpmStatus>;
}

/// Real implementation: spawns `claude-mpm` as a subprocess.
pub struct ProcessMpmClient {
    /// Path to claude-mpm binary (discovered via `which` if None).
    pub binary_path: Option<std::path::PathBuf>,
    /// Working directory for spawned processes.
    pub work_dir: std::path::PathBuf,
    /// Timeout in seconds for agent tasks.
    pub timeout_secs: u64,
}

impl Default for ProcessMpmClient { ... }

#[async_trait]
impl MpmClient for ProcessMpmClient {
    async fn list_agents(&self) -> Result<Vec<AgentInfo>> {
        // tokio::process::Command::new("claude-mpm")
        //   .args(["agents", "--json"])
        //   .output().await ...
    }
    // ...
}
```

### MpmStatus

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MpmStatus {
    pub version: String,
    pub agent_count: usize,
    pub active_tasks: usize,
    pub binary_path: String,
    pub is_healthy: bool,
}
```

### Public module layout

```
crates/mpm-sdk/src/
  lib.rs          — pub use everything; crate-level doc comment
  error.rs        — MpmError, Result
  types.rs        — AgentInfo, AgentTask, TaskStatus, AgentResult, Artifact, MpmStatus
  client.rs       — MpmClient trait + ProcessMpmClient
  parser.rs       — MPM output line parser (reuse notification_parser patterns)
  stream.rs       — helpers for reading and streaming tokio::process::ChildStdout
```

---

## 8. Telegram Commands to Implement for MPM Features

### Priority 1 — Core MPM control

| Command | Args | Handler | Description |
|---------|------|---------|-------------|
| `/agents` | — | `handle_agents` | List installed agents with name + description. Returns HTML list. |
| `/spawn <agent> <task>` | `agent_id task_text` | `handle_spawn` | Spawn agent, stream output back to chat as it arrives (using bot.edit_message_text to update a single message with progress), send final result when done. |
| `/mpm` | — | `handle_mpm_status` | MPM system status: version, agent count, binary path, health. |

### Priority 2 — Task management

| Command | Args | Handler | Description |
|---------|------|---------|-------------|
| `/tasks` | — | `handle_tasks` | List in-flight MPM tasks for this chat session. |
| `/cancel <task_id>` | task ID | `handle_cancel_task` | Cancel a running agent task (SIGTERM the subprocess). |

### Priority 3 — Quality of life

| Command | Args | Handler | Description |
|---------|------|---------|-------------|
| `/ask <question>` | free text | `handle_ask` | Quick shorthand: spawn the default MPM agent with the question as the prompt. |
| `/results <task_id>` | task ID | `handle_results` | Re-display the result of a completed task (from in-memory or session log). |

### Implementation notes for `/spawn` streaming

The streaming handler is the most complex. The recommended pattern:

```
1. Validate auth (is_authorized check).
2. Parse args: first word = agent_id, rest = prompt.
3. Send initial "Spawning <agent>..." message, capture message_id.
4. Create mpsc::channel::<AgentEvent>.
5. tokio::spawn(mpm_client.run_agent_streaming(agent_id, prompt, tx)).
6. In the handler task, loop on rx:
   - AgentEvent::Output(chunk) -> accumulate buffer; every N chars or 2s,
     bot.edit_message_text(chat_id, message_id, buffer_so_far).await
   - AgentEvent::Done(result)  -> send final formatted result as new message
   - AgentEvent::Error(e)      -> send error message
7. Respect Telegram rate limits: max 1 edit/second per message.
```

This mirrors the existing `poll_output_loop` pattern but push-based rather than polling.

---

## 9. output_filter / notification_parser Reuse in mpm-sdk

The `parser.rs` module in `mpm-sdk` should:

- Depend on `commander-core` for `strip_ansi`, `is_ui_noise`, and `ParsedSessionStatus`.
- Add MPM-specific line classifiers:
  - `is_mpm_completion_marker(line: &str) -> bool` — detect agent-done lines.
  - `is_mpm_error(line: &str) -> bool` — detect fatal error lines.
  - `extract_artifact_blocks(output: &str) -> Vec<Artifact>` — extract code fences and
    file paths from agent output.

These are thin wrappers over the regex patterns already present in
`commander-adapters/src/patterns.rs` (the `mpm` submodule).

---

## 10. Summary

### Adding the crate

- Create `crates/mpm-sdk/` — workspace auto-discovers it (no root Cargo.toml edit).
- Core deps: `commander-core`, `commander-adapters`, `tokio`, `serde`, `thiserror`.
- Add `mpm-sdk = { path = "../mpm-sdk" }` to `commander-telegram/Cargo.toml`.

### Key design decisions

1. `mpm-sdk` spawns `claude-mpm` as a **subprocess** via `tokio::process::Command`, not
   through tmux. This gives clean stdout capture without pane-scraping noise.
2. Expose a **`MpmClient` trait** so the Telegram handlers can be tested with a mock
   implementation that returns canned responses.
3. Output parsing reuses `commander-core` (ANSI stripping, noise filtering) and
   `commander-adapters` MPM patterns rather than duplicating regex logic.
4. The IPC daemon is not required — `mpm-sdk` is a self-contained subprocess SDK.
   If daemon integration is needed later, replicate the `DaemonClient` pattern (thin
   JSON-RPC client with no dependency on `commander-daemon`).

### Handler addition checklist

1. Add variant(s) to `Command` enum in `handlers.rs`.
2. Write `async fn handle_<name>(bot, msg, state) -> ResponseResult<()>` following the
   auth-check + typing + send pattern.
3. Add arm(s) to `handle_command` match block at bottom of `handlers.rs`.
4. Add `mpm-sdk` dependency to `commander-telegram/Cargo.toml`.
5. Wire `ProcessMpmClient` into `TelegramState` (or construct per-call for stateless ops).

### Suggested Telegram MPM command set (final)

```
/agents           — list available agents
/spawn <agent> <task>  — spawn agent, stream output, send result
/mpm              — MPM system health and version
/tasks            — list running agent tasks for this session
/cancel <id>      — cancel a running agent task
/ask <question>   — shorthand: spawn default agent with question
```

---

**Files investigated:**
- `/Users/masa/Projects/ai-commander/Cargo.toml`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/Cargo.toml`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/session.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/ipc_client.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-core/src/lib.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-core/src/output_filter.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-core/src/notification_parser.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/lib.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/mpm.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/traits.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-agent/src/lib.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-orchestrator/src/lib.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/lib.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/ipc/protocol.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/ipc/unix.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/sessions.rs`
