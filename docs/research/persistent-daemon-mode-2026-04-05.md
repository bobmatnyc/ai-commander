# Persistent Daemon Mode Feasibility for mpm-sdk Adapter

**Date:** 2026-04-05
**Author:** Research Agent (Claude Opus 4.6)
**Project:** ai-commander
**Scope:** Evaluate approaches for keeping a long-running claude-mpm process alive between conversation turns to eliminate cold-start latency.

---

## Executive Summary

The `claude-mpm serve` API already provides exactly what we need: a persistent HTTP daemon on port 7777 with full session management, message sending (blocking and SSE streaming), and session resume. The SDK crate (`mpm-sdk`) already has both `UiServiceClient` (HTTP client) and `ServeManager` (lifecycle manager) implemented and exported. **Option A (use `claude-mpm serve`) is the clear winner** -- the infrastructure is built, tested, and only needs wiring into the `MpmSdkAdapter` as a new session backend.

---

## 1. claude-mpm serve API (Commit ba7c33f)

### 1.1 UiServiceClient (`crates/mpm-sdk/src/serve_client.rs`)

A full HTTP client targeting the `claude-mpm serve` FastAPI daemon on port 7777. Capabilities:

| Method | Endpoint | Description |
|--------|----------|-------------|
| `health()` | `GET /api/v1/health` | Daemon liveness check |
| `list_sessions()` | `GET /api/v1/sessions` | List all active sessions |
| `create_session(req)` | `POST /api/v1/sessions` | Create or resume a session |
| `get_session(id)` | `GET /api/v1/sessions/{id}` | Get session state |
| `delete_session(id)` | `DELETE /api/v1/sessions/{id}` | Terminate session |
| `send_message(id, content)` | `POST /api/v1/sessions/{id}/messages` | Send message, collect full response |
| `send_message_streaming(id, content, tx)` | `POST /api/v1/sessions/{id}/messages` (stream=true) | SSE streaming via `text/event-stream` |
| `get_context(id)` | `GET /api/v1/sessions/{id}/context` | Token usage and compaction status |
| `interrupt(id)` | `POST /api/v1/sessions/{id}/interrupt` | SIGINT to subprocess |
| `clear_messages(id)` | `DELETE /api/v1/sessions/{id}/messages` | Clear message history |

The `CreateSessionRequest` supports `resume_id`, `model`, `cwd`, `project_root`, `bare`, and `permission_mode` fields.

The streaming method parses SSE `data:` lines containing `ServeStreamEvent` JSON, mapping event types:
- `text` / `assistant` -> `AgentEvent::Text`
- `tool_use` -> `AgentEvent::ToolUse`
- `message_stop` / `result` -> `AgentEvent::Complete`
- `error` -> `AgentEvent::Error`

**Key finding:** This streaming API maps directly to the existing `EventDrivenAdapter` trait's `RuntimeEvent` enum. The mapping is nearly 1:1 with the current `map_agent_event` function in `mpm_sdk.rs`.

### 1.2 ServeManager (`crates/mpm-sdk/src/serve_manager.rs`)

Lifecycle manager for the `claude-mpm serve` daemon. Key methods:

| Method | Description |
|--------|-------------|
| `new(binary, port)` | Create manager with explicit binary path |
| `discover(port)` | Find `claude-mpm` on PATH, create manager |
| `start()` | Run `claude-mpm serve start --port <port>` |
| `stop()` | Run `claude-mpm serve stop --port <port>` |
| `status()` | Poll `GET /api/v1/health` |
| `wait_ready(timeout_secs)` | Poll health until 200 or timeout (500ms intervals) |
| `start_and_wait(timeout_secs)` | Start + wait + return connected `UiServiceClient` |
| `client()` | Return `UiServiceClient` for this host:port |

PID file: `~/.claude-mpm/serve-{port}.pid`

### 1.3 Two HTTP Servers in claude-mpm

Per the existing research doc (`docs/research/claude-mpm-serve-api-2026-03-27.md`):

| Server | Port | Command | Purpose |
|--------|------|---------|---------|
| `MessageEndpoint` (legacy) | 7856 | `claude-mpm run --sdk --inject-port PORT` | Inject prompts into running SDK session |
| `ui_service` (new) | 7777 | `claude-mpm serve start` | Full session management REST API |

The legacy endpoint (`MpmHttpClient`) is limited to `/inject` (blocking), `/status`, `/session`, `/activity`, `/history`. The new `ui_service` API is far richer with session CRUD, streaming, context tracking, and interrupt control.

---

## 2. commander-daemon Analysis

### 2.1 Architecture (`crates/commander-daemon/src/`)

The daemon provides a central service that manages sessions over IPC (Unix domain sockets / named pipes with JSON-RPC protocol):

**Components:**
- `DaemonService` -- main coordinator: IPC server, cleanup tasks, signal handling, PID management
- `SessionManager` -- session lifecycle with `AgentOrchestrator` instances, memory monitoring, idle cleanup
- `MemoryMonitor` -- per-session RSS tracking with 30s polling intervals
- `PairingManager` -- pairing code generation/validation for client auth
- `IpcServer` -- JSON-RPC over Unix domain sockets

**Session model:** Each `ManagedSession` has: id, name, adapter type, project_path, status (Creating/Active/Idle/Terminating/Terminated/Error), timestamps, PID, error_message.

### 2.2 Interaction with Telegram Bot

The telegram bot (`crates/commander-telegram/src/state.rs`) uses `daemon_session_id` to optionally route messages through the daemon:

```rust
// In session creation:
session.daemon_session_id = Some(daemon_id);

// In message sending:
if let (Some(ref daemon), Some(ref session_id)) =
    (&self.daemon_client, &session.daemon_session_id)
{
    daemon.session_send(session_id, message).await?;
} else {
    tmux.send_line(&session.tmux_session, None, message)?;
}
```

The daemon is optional -- the bot falls back to direct tmux interaction if no daemon client or session ID exists. This is a clean, dual-path pattern.

### 2.3 Suitability for Event-Driven Sessions

The daemon's `SessionManager` currently creates `AgentOrchestrator` instances via `commander_orchestrator`. The `send_to_session` method calls `orch.process_user_input(message)` which returns a string response (not a stream).

**Limitations for event-driven use:**
- `process_user_input` returns `String`, not a stream -- no streaming support
- Sessions are `AgentOrchestrator`-based, which likely wraps tmux-style adapters
- No `EventDrivenAdapter` integration yet
- Adding streaming would require significant refactoring of the IPC protocol (JSON-RPC doesn't natively support streaming)

**Verdict:** The daemon is tmux-centric today. Extending it for event-driven sessions is possible but requires substantial work compared to Option A.

---

## 3. claude-mpm CLI Modes and Session Persistence

### 3.1 Current MpmClient Subprocess Model

The `MpmClient` (`crates/mpm-sdk/src/client.rs`) spawns `claude-mpm run --headless` per turn with these flags:
```
run --headless --non-interactive --no-check-dependencies --no-prompt --output-format stream-json
```

If `last_session_id` is set from a previous run, it appends `--resume <session_id>`.

### 3.2 Session ID Tracking

`MpmClient.last_session_id` is extracted from the NDJSON output during streaming (via `extract_session_id` in the parser). On subsequent calls, `--resume` passes this ID back to `claude-mpm`.

**Key behavior:** Session state is persisted by `claude-mpm` itself on disk. The `--resume` flag tells claude-mpm to load that session's history from its internal store. This means:
- Session context survives across separate process invocations
- `--resume` recovers full conversation history (as much as claude-mpm stores)
- Each `run_streaming` call is NOT truly fresh -- it continues from the last session
- The MpmClient is essentially stateless except for tracking which session ID to resume

### 3.3 Known CLI Modes

Based on the codebase:
- `claude-mpm run` -- single-shot or interactive execution (with `--headless`, `--sdk`, `--inject-port`)
- `claude-mpm serve start --port <port>` -- start persistent HTTP daemon
- `claude-mpm serve stop --port <port>` -- stop daemon
- `claude-mpm agents list [--json]` -- list available agents
- `claude-mpm --version` -- version info

The `serve` command is the persistent daemon mode we need.

---

## 4. Session Persistence in mpm-sdk

### 4.1 How --resume Works

1. First call: `MpmClient` spawns `claude-mpm run --headless -i "prompt"` (no `--resume`)
2. claude-mpm runs, emits NDJSON including session init with session ID
3. `MpmClient` extracts and stores session ID in `last_session_id`
4. Second call: spawns `claude-mpm run --headless --resume <session_id> -i "new prompt"`
5. claude-mpm loads session history from disk, continues conversation

### 4.2 Implications

- Session history IS persisted by claude-mpm (not by our code)
- Cold-start per turn is the issue -- not session loss
- Each subprocess invocation has: binary loading, Python/Node startup, model initialization
- The serve API eliminates this by keeping the process alive

---

## 5. Design Options Assessment

### Option A: Use `claude-mpm serve` (RECOMMENDED)

**Approach:** Start `claude-mpm serve` once via `ServeManager`, create sessions via `UiServiceClient`, send messages via the HTTP streaming API.

**Implementation plan:**
1. Add a `ServeBackedMpmSdkAdapter` (or modify `MpmSdkAdapter` to support both modes)
2. On `start_session`: call `ServeManager::start_and_wait()` if daemon not running, then `UiServiceClient::create_session()`
3. On `send`: call `UiServiceClient::send_message_streaming()`, map SSE events to `RuntimeEvent`
4. On `stop`: call `UiServiceClient::delete_session()`
5. Daemon lifecycle: start on first session, keep alive, stop on adapter drop or explicit shutdown

**Effort:** Low (2-3 days). All HTTP client code exists. Need:
- Wire `UiServiceClient` streaming into `EventStream` (map `ServeStreamEvent` -> `RuntimeEvent`)
- Add daemon lifecycle management to adapter (start/stop, health checks)
- Handle daemon failure/restart gracefully

**Pros:**
- Infrastructure already built and exported from `mpm-sdk`
- Streaming support via SSE already implemented
- Session management (create, resume, delete) already works
- Context tracking (`get_context`) enables smart compaction decisions
- Interrupt support for cancellation
- Clean separation: daemon manages sessions, adapter just talks HTTP

**Cons:**
- Dependency on `claude-mpm serve` being stable (but it's already shipping)
- Extra process to manage (but ServeManager handles this)
- Port management (but PID files prevent conflicts)

**Cold-start elimination:** First request incurs daemon startup (~2-5s). Subsequent requests have zero cold-start -- just an HTTP round-trip.

### Option B: Extend commander-daemon for Event-Driven Sessions

**Approach:** Add `EventDrivenAdapter` support to `commander-daemon`'s `SessionManager`.

**Effort:** High (1-2 weeks). Need:
- Add streaming response support to IPC protocol (JSON-RPC SSE or WebSocket)
- Create `EventDrivenSession` variant in `SessionManager`
- Wire `MpmClient` (or `UiServiceClient`) into daemon sessions
- Update all IPC handlers for streaming
- Test concurrent session streaming

**Pros:**
- Unified session management for all adapter types
- Single daemon for monitoring, cleanup, pairing

**Cons:**
- Significant refactoring of IPC layer
- Duplicates what `claude-mpm serve` already does
- commander-daemon currently orchestrator-based, not event-driven
- Adds complexity without clear benefit over Option A

**Verdict:** Not recommended short-term. Could be a Phase 2 consolidation.

### Option C: Thin Daemon Wrapper (Keep stdin/stdout Pipes Open)

**Approach:** Spawn `claude-mpm` once, keep pipes open, multiplex turns via a custom protocol.

**Effort:** Medium (1 week). Need:
- Custom process management with persistent stdio pipes
- Turn delimiting protocol over stdout (since NDJSON stream-json uses newline-delimited events)
- Input injection via stdin
- Handle process crashes/restarts

**Pros:**
- No dependency on `claude-mpm serve`
- Direct, low-overhead communication

**Cons:**
- `claude-mpm run --headless` is designed for single-shot invocation, not persistent stdin
- Would need claude-mpm to support a "repl" or "listen" mode -- which is exactly what `serve` does
- Fragile: process crashes lose all state
- No session management, context tracking, or interrupt support
- Reinventing what `serve` already provides

**Verdict:** Not recommended. This is what `serve` mode was built to replace.

### Option D: Optimize Per-Turn Subprocess

**Approach:** Accept subprocess model, optimize startup time.

**Implementation ideas:**
- Binary caching: ensure `claude-mpm` is pre-compiled and in PATH (already done)
- Warm pool: pre-spawn N idle processes that accept prompts
- Session pre-loading: pass `--resume` to skip init

**Effort:** Low (1-2 days for warm pool, 0 for resume optimization).

**Pros:**
- Minimal changes
- No new dependencies

**Cons:**
- Fundamental cold-start remains (Python/Node runtime, model init)
- Warm pool adds complexity for marginal gain
- `--resume` already works -- the bottleneck is process startup, not session loading

**Verdict:** Acceptable as stopgap, but Option A is strictly better.

---

## 6. Summarization Pipeline Analysis

### 6.1 Architecture

The summarization system lives in `commander-core` and has a 3-tier pipeline:

**Tier 1 -- Structured Extraction (free, instant):**
- `structured_summarizer::extract()` parses terminal output lines for patterns
- Extracts: files edited/read, test results, git ops, errors, tools used, build status
- Uses regex patterns (LazyLock compiled) to identify facts
- Produces a `StructuredSummary` with confidence score
- If confidence >= 0.7 (configurable via `SUMMARIZER_CONFIDENCE_THRESHOLD`), generates summary from template

**Tier 2 -- Cheap LLM (Haiku):**
- When confidence >= 0.4 but < threshold
- Sends pre-digested context (structured facts + key lines) to `anthropic/claude-haiku-3.5`
- Uses OpenRouter API

**Tier 3 -- Full LLM (Sonnet):**
- Fallback when confidence < 0.4
- Sends full raw response to `anthropic/claude-sonnet-4`
- Uses OpenRouter API

**Fallback:** When no API key set, truncates to 10 lines / 500 chars with "more lines/chars" indicator.

### 6.2 Triggers in Telegram Bot

Two distinct summarization triggers:

1. **Incremental (progressive) summaries:** Every 500 characters of new output (`chars_since_last_summary >= 500`), calls `summarize_incremental_tiered()`. Used for long-running operations to show progress.

2. **Final summary:** On completion detection, calls `summarize_with_fallback(query, raw_response)` which delegates to `summarize_tiered()`. Only when `is_summarization_available()` returns true (OpenRouter API key set).

### 6.3 Decoupling Assessment

The summarizer **can be called as a standalone function** with string inputs:
- `summarize_with_fallback(query: &str, raw_response: &str) -> String` (async)
- `summarize_blocking_with_fallback(query: &str, raw_response: &str) -> String` (sync)
- `summarize_tiered(query: &str, raw_response: &str) -> (String, u8)` (async, returns tier used)
- `summarize_incremental_tiered(content: &str, line_count: usize) -> Result<String>` (async)

**It is NOT tightly coupled to tmux output.** The only coupling is in `structured_summarizer` which parses terminal-style output patterns (tool spinners, cargo test output, etc.). For event-driven adapters that produce clean text, tier 1 extraction may have lower confidence, pushing to tier 2/3 LLM summarization -- which works on any text.

---

## 7. Recommendation

### Primary: Option A -- Wire `UiServiceClient` into `MpmSdkAdapter`

**Phase 1 (2-3 days):**
1. Add `ServeBackend` to `MpmSdkAdapter` that uses `ServeManager` + `UiServiceClient`
2. On first `start_session`, start serve daemon if not already running
3. Map SSE streaming events to `EventStream` (reuse existing `map_agent_event` logic)
4. Support session resume via `CreateSessionRequest.resume_id`
5. Add health check loop for daemon monitoring

**Phase 2 (optional, 1 week):**
1. Add serve-backed sessions to `commander-daemon` for unified monitoring
2. Expose `get_context()` to telegram for smart compaction/summarization decisions
3. Add `interrupt()` support for user-initiated cancellation via telegram commands

**Why this wins:**
- Zero cold-start after first request
- All infrastructure already built and tested
- Streaming support ready
- Session persistence handled by claude-mpm
- Context tracking enables future smart features
- Minimal code changes needed

### Fallback: Option D -- Keep subprocess model with `--resume`

If `claude-mpm serve` proves unreliable, the current subprocess + `--resume` model works. The main optimization would be measuring actual cold-start time and determining if it's acceptable for the use case.

---

## 8. Files Referenced

| File | Purpose |
|------|---------|
| `crates/mpm-sdk/src/serve_client.rs` | UiServiceClient HTTP client for serve API |
| `crates/mpm-sdk/src/serve_manager.rs` | ServeManager lifecycle for serve daemon |
| `crates/mpm-sdk/src/client.rs` | MpmClient subprocess-based client |
| `crates/mpm-sdk/src/http_client.rs` | MpmHttpClient for legacy inject API |
| `crates/mpm-sdk/src/types.rs` | Shared types (AgentEvent, ServeSession, etc.) |
| `crates/mpm-sdk/src/lib.rs` | SDK public exports |
| `crates/commander-daemon/src/lib.rs` | Daemon architecture overview |
| `crates/commander-daemon/src/sessions.rs` | SessionManager with AgentOrchestrator |
| `crates/commander-daemon/src/service.rs` | DaemonService main coordinator |
| `crates/commander-adapters/src/mpm_sdk.rs` | Current MpmSdkAdapter (subprocess model) |
| `crates/commander-adapters/src/event_driven.rs` | EventDrivenAdapter trait |
| `crates/commander-adapters/src/registry.rs` | Adapter registry (terminal + event-driven) |
| `crates/commander-telegram/src/state.rs` | Telegram bot session/polling/summarization |
| `crates/commander-telegram/src/daemon.rs` | Telegram daemon lifecycle (unrelated to commander-daemon) |
| `crates/commander-telegram/src/event_consumer.rs` | RuntimeEvent stream consumer for telegram |
| `crates/commander-core/src/summarizer.rs` | 3-tier summarization pipeline |
| `crates/commander-core/src/structured_summarizer.rs` | Tier 1 structured fact extraction |
| `docs/research/claude-mpm-serve-api-2026-03-27.md` | Prior research on serve API |
