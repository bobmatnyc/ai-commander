# Claude MPM SDK/API Research for `mpm-sdk` Crate

**Date:** 2026-03-26
**Purpose:** Understand MPM's programmatic interface to design a headless adapter for ai-commander
**MPM Version:** 6.0.0 (installed), Python package `claude-mpm==5.5.1` at `/Users/masa/Projects/claude-mpm`

---

## 1. MPM CLI Command Surface

### Top-Level Flags Relevant to Headless/Programmatic Use

```
claude-mpm [global-flags] COMMAND [options]
```

Key flags for headless operation:

| Flag | Effect |
|------|--------|
| `--headless` | Disables Rich console; outputs NDJSON to stdout for programmatic parsing |
| `--non-interactive` | Non-interactive mode; reads from stdin or `--input` |
| `-i INPUT / --input INPUT` | Provide prompt text or file path directly |
| `--sdk` | Use Agent SDK runtime (`claude-agent-sdk`) instead of CLI subprocess |
| `--cli` | Force CLI subprocess runtime (default when SDK not installed) |
| `--inject-port PORT` | Start HTTP message injection endpoint on PORT (default: 7856) |
| `--channels CHANNELS` | Enable ChannelHub (requires `--sdk`); comma-separated: `telegram,slack` |
| `--monitor` | Start Socket.IO monitoring server (default port: 8765) |
| `--websocket-port PORT` | Override Socket.IO server port |
| `--launch-method {exec,subprocess}` | How Claude is launched: `exec` replaces process, `subprocess` is child |
| `--no-hooks` | Disable hook service |
| `--no-tickets` | Disable automatic ticket creation |
| `--no-check-dependencies` | Skip startup dependency checks |
| `--no-prompt` | Never prompt for dependency installation (non-interactive) |
| `--resume [SESSION_ID]` | Resume a Claude Code session |
| `--mpm-resume [SESSION_ID]` | Resume an MPM session |
| `--project-dir DIR` | Override working directory auto-detection |
| `--dangerously-skip-permissions` | Skip permission prompts (passed to Claude Code) |
| `--output-format FORMAT` | Output format passthrough to Claude (e.g. `stream-json`) |
| `--input-format FORMAT` | Input format passthrough (e.g. `stream-json` for vibe-kanban compatibility) |

### Subcommands (abbreviated, most relevant first)

```
run          - Run orchestrated Claude session (default)
agents       - Manage agents: list, deploy, create, edit, delete, configure
monitor      - Manage Socket.IO server: start, stop, restart, status
mcp          - MCP Gateway: install, start, stop, status, tools, register, test
message      - Inter-process messaging: send, list, read, sessions
skills       - Manage Claude Code skills
config       - Unified configuration management
profile      - Manage deployment profiles
dashboard    - Web dashboard for monitoring
channels     - Multi-channel connection manager
aggregate    - Event aggregator for capturing agent sessions
queue        - Message queue consumer
analyze      - Code analysis and mermaid diagrams
mpm-search   - Semantic search
doctor       - Diagnostics / health check
debug        - Development debugging tools
```

---

## 2. MPM HTTP / IPC API

MPM exposes **two distinct network APIs**:

### 2a. Message Injection HTTP API (Port 7856)

**Activated by:** `claude-mpm --inject-port 7856 --sdk` (or `CLAUDE_MPM_INJECT_PORT=7856`)

**Source:** `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/agents/message_endpoint.py`

This is a FastAPI server (uvicorn) running at `http://127.0.0.1:7856`.

**Endpoints:**

```
GET  /status    -> { status, runtime, port, history_count }
POST /inject    -> Execute a prompt, return result (blocking)
GET  /session   -> Current SDK session state
GET  /activity  -> Recent agent events (limit param)
GET  /monitor   -> Monitor agent availability info
GET  /history   -> Last 50 injected prompts and results
```

**POST /inject request body:**
```json
{
  "prompt": "string (required)",
  "system_prompt": "string | null",
  "model": "string | null",
  "session_id": "string | null",
  "allowed_tools": ["string"] | null,
  "cwd": "string | null",
  "max_turns": "int | null"
}
```

**POST /inject response body:**
```json
{
  "text": "string",
  "session_id": "string | null",
  "cost_usd": "float | null",
  "num_turns": "int | null",
  "duration_ms": "int | null",
  "is_error": "bool",
  "runtime": "string"  // "sdk" or "cli"
}
```

**Critical:** The `/inject` endpoint only works when MPM is running in `--sdk` mode
(i.e., `claude-agent-sdk` Python package is available). Without SDK, the endpoint
starts but falls back to CLI subprocess execution per the `runtime_bridge.py` logic.

### 2b. Socket.IO Monitoring Server (Port 8765)

**Activated by:** `claude-mpm --monitor` or `claude-mpm monitor start`

**Source:** `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/socketio_server.py`
and the `socketio/` subpackage.

- Uses `python-socketio` + `aiohttp` as the transport
- Default port: 8765 (configurable via `--websocket-port`)
- Broadcasts real-time events: session status, agent activity, file events, git events
- Has a static dashboard served over HTTP on the same port
- Falls back to `SocketIOClientProxy` if a server is already running on port 8765
  (used in `exec` launch mode where MPM replaces itself with claude)

**Key event types broadcast:** session lifecycle, agent delegation, tool calls,
file changes, git events, error events.

---

## 3. MPM Python Package Importability

**Installed at:** `/opt/homebrew/lib/python3.14/site-packages`
(editable install from `/Users/masa/Projects/claude-mpm`)

**Key importable modules for headless use:**

```python
# Execute a prompt programmatically
from claude_mpm.services.agents.runtime_bridge import execute_agent_prompt
result = await execute_agent_prompt(
    prompt="...",
    system_prompt=None,
    model=None,           # "sonnet", "opus", "haiku" or full API name
    session_id=None,      # resume a session
    cwd=None,             # working directory
    allowed_tools=None,   # list of allowed tool names
    max_turns=None,
)
# Returns: { text, session_id, cost_usd, num_turns, duration_ms, is_error, tool_calls, runtime }

# Runtime selection
from claude_mpm.services.agents.runtime_config import get_runtime_type, get_runtime
# Runtime auto-selects: "sdk" if claude_agent_sdk available, else "cli"
# Override: CLAUDE_MPM_RUNTIME=sdk|cli

# Start the HTTP injection endpoint
from claude_mpm.services.agents.message_endpoint import MessageEndpoint
endpoint = MessageEndpoint(port=7856)
endpoint.run()  # blocking

# Agent runtime ABC
from claude_mpm.services.agents.agent_runtime import AgentRuntime, AgentConfig, AgentResult
```

### Agent Runtime Hierarchy

```
AgentRuntime (ABC)
├── CLIAgentRunner   - Subprocess: runs `claude -p --output-format json <prompt>`
└── SDKAgentRunner   - In-process: uses claude_agent_sdk (not installed locally)
```

**`claude_agent_sdk` is NOT installed** in this environment. MPM defaults to CLI runtime.

---

## 4. How MPM Spawns Agents

### Interactive Mode (default)
- `claude-mpm` (or `claude-mpm run`) launches `claude` as a PTY subprocess
- Launch method: `exec` (replaces process) or `subprocess` (child process)
- Managed by `SubprocessLauncherService` with full PTY/terminal handling
- Output: Rich-formatted terminal output (not machine-parseable)

### Headless Mode (`--headless`)
- MPM initializes (hooks, agents, skills) then calls `os.execvpe()` to replace
  itself with `claude --verbose --output-format stream-json -p <prompt>`
- **Output format: NDJSON** (Newline-Delimited JSON), one JSON object per line
- stdout is clean JSON, stderr gets warnings/debug
- Source: `HeadlessSession` class in `core/headless_session.py`

### Non-Interactive Mode (`-i INPUT` or `--non-interactive`)
- Reads prompt from arg or stdin, runs once and exits
- Uses `oneshot_session.py` or `headless_session.py` depending on `--headless` flag

### SDK Mode (`--sdk`, requires `claude-agent-sdk`)
- In-process agent execution, no subprocess
- Supports streaming callbacks, interruptible sessions, session resume
- Enables `--channels` (ChannelHub for telegram/slack routing)
- Enables the `/inject` HTTP endpoint to work synchronously

---

## 5. MPM Output Format

### Interactive/PTY Output (current ai-commander integration)
Rich console ANSI-formatted. Already handled by `commander-core`:
- `is_mpm_ready()` detects idle state
- `is_ui_noise()` filters `[model|Claude MPM|70%]` status bar lines
- Idle detection: `❯` prompt character or MPM-specific ready markers

### Headless/NDJSON Output (`--headless`)
Claude Code `stream-json` format, one JSON object per line:
```jsonl
{"type": "system", "subtype": "init", "session_id": "abc123", ...}
{"type": "assistant", "message": {"content": [{"type": "text", "text": "..."}]}, ...}
{"type": "tool_use", "name": "Bash", "input": {"command": "..."}, ...}
{"type": "tool_result", "tool_use_id": "...", "content": "...", ...}
{"type": "result", "subtype": "success", "result": "...", "cost_usd": 0.01, ...}
```

### HTTP Inject API Response (structured JSON)
```json
{
  "text": "final text response",
  "session_id": "abc123",
  "cost_usd": 0.005,
  "num_turns": 3,
  "duration_ms": 4200,
  "is_error": false,
  "runtime": "cli"
}
```

---

## 6. Existing ai-commander MPM Integration

The current codebase already has a working MPM adapter at:
`/Users/masa/Projects/ai-commander/crates/commander-adapters/src/mpm.rs`

**Current implementation:**
- `MpmAdapter` wraps `claude-mpm` as a PTY subprocess (interactive mode)
- Launch command: `claude-mpm --project <path>`
- State detection via regex patterns on PTY output
- Registered in `AdapterRegistry`, exposed in CLI/TUI/Telegram as `-a mpm`
- Session agent uses `/mpm-session-pause` and `/mpm-session-resume` commands

**Gap:** No support for headless/programmatic API. All operation is interactive PTY.

---

## 7. Recommended `mpm-sdk` Crate Architecture

### Primary Recommendation: HTTP Client to Injection API

**Architecture:** Rust HTTP client (reqwest) calling the inject endpoint.

```
ai-commander
  └── mpm-sdk crate
        ├── MpmProcess    - Manages the claude-mpm subprocess lifecycle
        ├── MpmHttpClient - reqwest client for POST /inject, GET /status
        └── MpmSession    - Session state, resume support
```

**Workflow:**
1. Spawn `claude-mpm --sdk --inject-port 7856 --headless --no-check-dependencies --no-prompt`
   in background, wait for HTTP server to become available (poll GET /status)
2. Send prompts via `POST /inject` (blocking, returns structured JSON)
3. Resume sessions by passing `session_id` to subsequent `/inject` calls
4. Monitor via Socket.IO on port 8765 (optional, for real-time streaming)
5. Shutdown by killing the subprocess

**Pros:**
- Clean Rust/HTTP boundary, no FFI or Python embedding
- Structured JSON responses, no output parsing needed
- Session continuity via `session_id`
- Already has `/status`, `/session`, `/activity` endpoints for monitoring

**Cons:**
- Requires `--sdk` mode which needs `claude_agent_sdk` Python package
- Without SDK, falls back to CLI runtime (still works, but single-turn only)
- Need to manage subprocess lifecycle robustly (crash recovery, port conflicts)

### Secondary Recommendation: PTY Subprocess with NDJSON (`--headless`)

**Architecture:** Extend existing `MpmAdapter` to use `--headless` flag.

```
claude-mpm run --headless --non-interactive -i "prompt text"
```

**Workflow:**
1. Spawn subprocess with `--headless -i <prompt>` flags
2. Collect NDJSON lines from stdout
3. Parse `{"type":"result",...}` terminal event for final answer
4. Extract session_id for subsequent resume calls

**Pros:**
- No dependency on `claude_agent_sdk`
- Works today with current MPM installation
- No HTTP server management

**Cons:**
- One subprocess per prompt (no persistent session unless using `--resume`)
- Must parse NDJSON stream correctly
- Session resume requires `--resume SESSION_ID` flag on next invocation
- No real-time streaming back to Rust without complex pipe handling

### Not Recommended: Python FFI

Embedding Python or calling MPM as a library via PyO3 would be extremely
complex given MPM's async/event-loop architecture and heavy dependency tree.

---

## 8. Implementation Checklist for `mpm-sdk` Crate

### Phase 1: Headless Subprocess (no extra deps)
- [ ] `MpmHeadlessRunner`: spawns `claude-mpm run --headless -i <prompt>`
- [ ] NDJSON parser for Claude stream-json format
- [ ] Session ID extraction and resume support (`--resume SESSION_ID`)
- [ ] Timeout handling and process cleanup

### Phase 2: HTTP Injection Client (requires `--sdk` mode)
- [ ] `MpmDaemon`: spawns `claude-mpm --sdk --inject-port PORT --no-prompt --no-check-dependencies`
- [ ] Wait for GET /status to return 200 (startup detection)
- [ ] `MpmClient`: reqwest-based client for POST /inject
- [ ] Port allocation (use `global-port-registry.json` model or random available port)
- [ ] Graceful shutdown on drop

### Phase 3: Monitoring Integration (optional)
- [ ] Socket.IO client connecting to port 8765 (monitor server)
- [ ] Forward agent events to ai-commander event bus
- [ ] Real-time streaming of tool calls and agent activity

---

## 9. Key File Locations

| What | Path |
|------|------|
| MPM source | `/Users/masa/Projects/claude-mpm/src/claude_mpm/` |
| HTTP inject endpoint | `.../services/agents/message_endpoint.py` |
| Runtime bridge | `.../services/agents/runtime_bridge.py` |
| Runtime config (sdk/cli selection) | `.../services/agents/runtime_config.py` |
| CLI runtime (subprocess) | `.../services/agents/cli_runtime.py` |
| SDK runtime | `.../services/agents/sdk_runtime.py` |
| Headless session | `.../core/headless_session.py` |
| Socket.IO server | `.../services/socketio_server.py` |
| Existing Rust adapter | `crates/commander-adapters/src/mpm.rs` |
| MPM patterns | `crates/commander-adapters/src/patterns.rs` |
| Output filter (is_mpm_ready) | `crates/commander-core/src/output_filter.rs` |
| MPM global state dir | `~/.claude-mpm/` |
| Port registry | `~/.claude-mpm/global-port-registry.json` |
| Session registry | `~/.claude-mpm/session-registry.db` |
| Message queue | `~/.claude-mpm/message_queue.db` |

---

## 10. Environment Variables

| Variable | Effect |
|----------|--------|
| `CLAUDE_MPM_INJECT_PORT` | Port for HTTP injection endpoint (default: 7856) |
| `CLAUDE_MPM_RUNTIME` | Force runtime: `sdk` or `cli` |
| `CLAUDE_MPM_NO_SKIP_PERMISSIONS` | Set to `1` to disable `--dangerously-skip-permissions` |
| `CLAUDE_MPM_USER_PWD` | Working directory override for headless mode |
