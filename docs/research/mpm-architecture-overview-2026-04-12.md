# Claude MPM Architecture Overview

**Date:** 2026-04-12
**Purpose:** Architecture research for planning major feature set

---

## 1. Project Structure

**Language:** Rust (Cargo workspace)
**Package manager:** Cargo
**Key directories:**

```
ai-commander/
├── crates/
│   ├── ai-commander/          # CLI entry point (main binary, REPL, TUI)
│   ├── commander-adapters/    # Adapter pattern (ClaudeCode, MPM, Shell, MpmSdk)
│   ├── commander-agent/       # User/Session agent types
│   ├── commander-api/         # HTTP API layer
│   ├── commander-core/        # Core business logic, notification parsing
│   ├── commander-daemon/      # Persistent daemon service (IPC, sessions)
│   ├── commander-events/      # Event types
│   ├── commander-gui/         # Tauri-based GUI (Svelte frontend)
│   ├── commander-memory/      # Memory store
│   ├── commander-models/      # Domain models (Project, etc.)
│   ├── commander-orchestrator/# AgentOrchestrator coordinating agents
│   ├── commander-persistence/ # StateStore
│   ├── commander-runtime/     # Runtime management
│   ├── commander-telegram/    # Telegram bot client
│   ├── commander-tmux/        # Tmux session/pane management
│   ├── commander-work/        # Work/task management
│   └── mpm-sdk/               # Headless SDK for spawning claude-mpm
├── ~/.claude-mpm/             # Runtime state directory
```

---

## 2. Instance/Session Management

**Session tracking** lives in two places:

### ~/.claude-mpm/ state
- `sessions/active_sessions.json` — JSON map of session UUIDs to metadata (context, created_at, last_used, use_count, agents_run)
- `session-registry.db` — SQLite DB identical schema to `messaging.db` (sessions + messages tables)
- `messaging.db` — Primary SQLite store for sessions and cross-project messages

### `commander-daemon` (`DaemonService` + `SessionManager`)
- `ManagedSession` struct tracks: id (UUID), name, adapter type, project_path, status (Creating/Active/Idle/Terminating/Terminated/Error), pid, timestamps
- Sessions keyed in `HashMap<String, Arc<RwLock<ManagedSession>>>`
- Builds on `commander-orchestrator::AgentOrchestrator`
- IPC via Unix domain sockets / named pipes with JSON-RPC protocol

### `mpm-sdk` sessions
- `ServeSession` type tracked by `claude-mpm serve` daemon on port 7777
- Created via `POST /api/v1/sessions` with `CreateSessionRequest` (resume_id, model, cwd, project_root, bare, permission_mode)
- Sessions have `claude_session_id` for resuming Claude Code sessions with `--resume`

**No tmux-based session registration** — tmux is purely a pane/window orchestration tool, not the session registry.

---

## 3. Message System (mpm-messaging)

**Primary store:** `~/.claude-mpm/messaging.db` (SQLite, 2.9MB active)

**Schema:**
```sql
CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    project_path TEXT NOT NULL,
    project_name TEXT NOT NULL,
    started_at TEXT,
    last_active TEXT,
    status TEXT DEFAULT 'active',
    pid INTEGER
);

CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    from_project TEXT NOT NULL,
    from_agent TEXT NOT NULL DEFAULT 'pm',
    to_project TEXT NOT NULL,
    to_agent TEXT NOT NULL DEFAULT 'pm',
    message_type TEXT NOT NULL DEFAULT 'notification',
    priority TEXT NOT NULL DEFAULT 'normal',
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'unread',
    created_at TEXT NOT NULL,
    read_at TEXT,
    replied_to TEXT,
    task_injected INTEGER NOT NULL DEFAULT 0,
    metadata TEXT,
    attachments TEXT
);
```

**Secondary store:** `~/.claude-mpm/message_queue.db` — SQLite queue with `task`, `schedule`, `kv` tables (priority-based FIFO task queue, not message store)

**MCP tools available:** `mcp__plugin_claude-mpm_mpm-messaging__*` — message_send, message_read, message_check, message_list, message_archive, message_reply, shortcut_add/remove/list/resolve

**Polling mechanism:** `message_check_state.json` tracks last-checked timestamps per project

---

## 4. Health/Startup

### `claude-mpm serve` health endpoint
- `GET /api/v1/health` — polled by `ServeManager::wait_ready()` with configurable timeout
- `ServeManager::status()` wraps health check, returns bool

### `commander-daemon` monitoring
- `MemoryMonitor` crate tracks `SessionMemoryInfo` per session
- No evidence of kuzu-memory or mcp-vector-search startup validation in current code
- Gap: No dedicated health check for optional MCP servers

### `MpmStatus` type
```rust
pub struct MpmStatus {
    pub version: String,
    pub binary_path: String,
    pub agent_count: usize,
    pub healthy: bool,
}
```

---

## 5. SDK / CLI Entry Points

### Process-based SDK (`mpm-sdk::MpmClient`)
- Spawns `claude-mpm run --headless` as child process
- Parses NDJSON stream output
- Supports `--resume <session_id>` for session continuation
- `DEFAULT_TIMEOUT_SECS = 300` (5 min)

### HTTP inject API (`mpm-sdk::MpmHttpClient`)
- Only available when MPM started with `--sdk --inject-port PORT`
- `POST /inject` with `InjectRequest` { prompt, session_id, system_prompt, model, allowed_tools, cwd, max_turns }

### HTTP serve API (`mpm-sdk::UiServiceClient`)
- `claude-mpm serve start --port <port>` launches FastAPI daemon on port 7777
- Full REST API: sessions CRUD, `POST /api/v1/sessions/{id}/messages` with streaming SSE

### CLI binary: `ai-commander` crate
- REPL (`/connect`, `/list`, `/send`, etc.)
- TUI (`tui/connection.rs` wraps session streaming)
- Daemon commands via `daemon_commands.rs`

---

## 6. Adapter Pattern

**Fully implemented** in `commander-adapters` crate:

**`RuntimeAdapter` trait** — unified interface:
- `info()` → `AdapterInfo` (id, name, description, command, default_args)
- `launch_command(project_path)` → `(String, Vec<String>)`
- `analyze_output(output)` → `OutputAnalysis` (state, confidence, errors, data)
- `is_idle()`, `is_error()`, `format_message()`
- `idle_patterns()`, `error_patterns()`

**States:** Starting, Idle, Working, Error, Stopped

**Registered adapters (`AdapterRegistry`):**
- `ClaudeCodeAdapter` — "claude-code"
- `MpmAdapter` — "mpm"
- `ShellAdapter` — "shell"
- `MpmSdkAdapter` (event-driven) — separate `EventDrivenAdapter` trait

**Event-driven variant:** `EventDrivenAdapter` trait with `EventStream`, `RuntimeEvent`, `SessionHandle` — more sophisticated for streaming events vs. terminal scraping.

**Gap:** No Codex or Augment/Auggie adapter exists yet. The trait interface is ready; adapters need implementation.

---

## 7. Tmux Integration

**`commander-tmux` crate** — `TmuxOrchestrator`:
- `new()` / `is_available()` — discovers tmux binary via `which`
- `create_session(name)` / `create_session_in_dir(name, dir)` — `tmux new-session -d -s`
- `destroy_session(name)` — cleanup
- `create_pane(session)` — split window
- `send_line(session, pane_id, command)` — send input
- `capture_output(session, pane_id, lines)` — read pane buffer
- Graceful degradation: `TmuxError::NotFound` if tmux unavailable

**Usage pattern:** Tmux is the execution substrate for running `claude-mpm` or `claude` processes inside managed panes, not a session registry.

---

## 8. Key Gaps for Feature Planning

| Area | Current State | Gap |
|------|--------------|-----|
| Adapters | Claude Code, MPM, Shell implemented | No Codex/Augment adapter |
| Health startup | `serve` health endpoint exists | No kuzu-memory/mcp-vector-search validation |
| Session registry | JSON file + SQLite dual storage | Potential consistency issues |
| SDK modes | Process-spawn + HTTP inject + serve API | No unified SDK entry point |
| Message polling | Timestamp-based check_state | No push/webhook; polling only |

---

## Key File Paths

- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/traits.rs` — `RuntimeAdapter` trait
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/registry.rs` — `AdapterRegistry`
- `/Users/masa/Projects/ai-commander/crates/commander-tmux/src/orchestrator.rs` — tmux management
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/sessions.rs` — `ManagedSession`, `SessionManager`
- `/Users/masa/Projects/ai-commander/crates/mpm-sdk/src/serve_client.rs` — REST API client (port 7777)
- `/Users/masa/Projects/ai-commander/crates/mpm-sdk/src/http_client.rs` — inject API (--sdk mode)
- `/Users/masa/Projects/ai-commander/crates/mpm-sdk/src/client.rs` — process-spawn SDK
- `/Users/masa/Projects/ai-commander/crates/mpm-sdk/src/types.rs` — all shared types
- `~/.claude-mpm/messaging.db` — primary message + session store (SQLite)
- `~/.claude-mpm/sessions/active_sessions.json` — JSON session registry
- `~/.claude-mpm/message_queue.db` — task priority queue (SQLite)
