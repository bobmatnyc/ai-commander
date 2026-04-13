# claude-mpm Serve / HTTP API Research

**Date:** 2026-03-27
**MPM Version:** 6.0.1
**Purpose:** Update `mpm-sdk` crate adapter to support the full serve API surface

---

## Summary of Findings

There are now **two distinct HTTP servers** in claude-mpm, serving different use cases:

| Server | Port | Activated by | Purpose |
|--------|------|-------------|---------|
| `MessageEndpoint` (legacy) | 7856 (default) | `claude-mpm run --sdk --inject-port PORT` | Inject prompts into a running SDK session |
| `ui_service` FastAPI (new) | 7777 (default) | `claude-mpm serve start` | Full session management REST API |

The existing `MpmHttpClient` only covers the legacy `MessageEndpoint` (`/inject` + `/status`). The new `ui_service` daemon is a completely different, far richer API.

---

## 1. Legacy MessageEndpoint (`--inject-port`, port 7856)

### How to start

```bash
claude-mpm run --sdk --inject-port 7856
# or with custom port:
claude-mpm run --sdk --inject-port 9000
```

The env var `CLAUDE_MPM_INJECT_PORT` can override the default of 7856.

### Endpoints

#### `POST /inject`

Execute a prompt via the SDK agent runtime (blocking, returns full result).

Request:
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

Response:
```json
{
  "text": "string",
  "session_id": "string | null",
  "cost_usd": "float | null",
  "num_turns": "int | null",
  "duration_ms": "int | null",
  "is_error": false,
  "runtime": "string"
}
```

#### `GET /status`

```json
{
  "status": "running",
  "runtime": "sdk | cli | unknown",
  "port": 7856,
  "history_count": 0
}
```

#### `GET /session`

Returns the current SDK session state from `SessionStateTracker`.
Returns `{"error": "No active SDK session", "state": "unavailable"}` if no session.

#### `GET /activity?limit=50`

Returns recent agent events from `SessionStateTracker`.
```json
{"events": [...]}
```

#### `GET /monitor`

Returns monitor agent availability info.
```json
{"available": true/false, "note": "string"}
```

#### `GET /history`

Last 50 injected prompts (preview only).
```json
{"history": [{"prompt": "...", "result_preview": "...", "is_error": bool, "runtime": "string"}]}
```

---

## 2. New ui_service Daemon (`claude-mpm serve`, port 7777)

This is a full FastAPI application wrapping persistent Claude subprocesses.

### How to start

```bash
# Background daemon (default)
claude-mpm serve start

# With options
claude-mpm serve start --port 7777 --host 127.0.0.1 --project-root /path/to/project

# Foreground (blocks terminal)
claude-mpm serve start --foreground

# With channel adapters (Telegram, Slack, GitHub)
claude-mpm serve start --channels telegram,slack

# Force kill existing instance
claude-mpm serve start --force

# Lifecycle management
claude-mpm serve stop [--port 7777]
claude-mpm serve restart [--port 7777] [--host ...] [--channels ...]
claude-mpm serve status [--port 7777] [--verbose]
```

### Startup / readiness

- Default background mode launches a detached subprocess via `python -m claude_mpm.cli serve start --background`
- Startup timeout: 30 seconds (configurable via `CLAUDE_MPM_SERVE_TIMEOUT` env var)
- Readiness is confirmed by: (1) PID file appearing at `~/.claude-mpm/serve-{port}.pid` AND (2) port becoming bound
- PID file: `~/.claude-mpm/serve-{port}.pid`
- Log file: `~/.claude-mpm/logs/serve-{port}.log`

### Health check

```
GET /health           -> {"service": "claude-mpm-serve", "status": "healthy"}
GET /api/v1/health    -> {"status": "healthy", "active_sessions": N}
```

### API Base Path: `/api/v1`

OpenAPI docs: `http://127.0.0.1:7777/api/v1/docs`

---

### Sessions

#### `GET /api/v1/sessions`
List all active sessions.
Response: array of `ManagedSessionState` objects.

#### `POST /api/v1/sessions` (201)
Create or resume a session. Spawns a `claude --output-format stream-json --print` subprocess.

Request:
```json
{
  "resume_id": "string | null",
  "model": "string | null (default: claude-opus-4-5)",
  "bare": false,
  "cwd": "string | null",
  "permission_mode": "default",
  "project_root": "string | null"
}
```

Response (`ManagedSessionState`):
```json
{
  "id": "uuid",
  "claude_session_id": "string | null",
  "status": "starting | idle | busy | compacting | terminated",
  "model": "claude-opus-4-5",
  "cwd": ".",
  "project_root": "string | null",
  "created_at": "ISO8601",
  "last_activity": "ISO8601",
  "context_tokens_used": 0,
  "context_tokens_total": 200000,
  "context_percent_used": 0.0,
  "permission_mode": "default"
}
```

Error: `409 Conflict` if max sessions (default 10) reached.

#### `GET /api/v1/sessions/{session_id}`
Get session state. Returns `ManagedSessionState`. 404 if not found.

#### `DELETE /api/v1/sessions/{session_id}` (204)
Send SIGTERM and remove session from tracking.

#### `PATCH /api/v1/sessions/{session_id}`
Update mutable session properties. Changes take effect on next message.

Request:
```json
{
  "model": "string | null",
  "permission_mode": "string | null",
  "output_format": "string | null"
}
```

#### `POST /api/v1/sessions/{session_id}/fork` (201)
Send `/fork` to session stdin.
Response: `{"message": "Fork command sent", "session_id": "..."}`

#### `POST /api/v1/sessions/{session_id}/interrupt`
Send SIGINT to subprocess.
Response: `{"message": "Interrupt sent", "session_id": "..."}`

#### `PUT /api/v1/sessions/{session_id}/plan-mode`
Toggle plan mode (sends `/plan`).
Response: `{"message": "Plan mode toggled", "session_id": "..."}`

---

### Messages

#### `GET /api/v1/sessions/{session_id}/messages`
Get conversation history.
Response: `{"messages": [{"role": "user|assistant|system", "content": "..."}]}`

#### `POST /api/v1/sessions/{session_id}/messages`
Send a message. Can stream (SSE) or collect (JSON array).

Request:
```json
{
  "content": "string (required)",
  "stream": false
}
```

When `stream=false`: `{"events": [StreamEvent, ...]}`

When `stream=true`: `Content-Type: text/event-stream`, each line is:
`data: <StreamEvent JSON>\n\n`
Ends with: `data: {"type": "message_stop"}\n\n`

`StreamEvent` schema:
```json
{
  "type": "string (system|assistant|result|error|timeout|...)",
  "session_id": "string | null",
  "content": "string | null",
  "usage": {"input_tokens": N, ...} | null,
  "data": {...raw event...}
}
```

Terminal event types: `result`, `error`

#### `DELETE /api/v1/sessions/{session_id}/messages` (204)
Send `/clear` to session stdin and wipe in-memory history.

#### `POST /api/v1/sessions/{session_id}/messages/compact`
Send `/compact` optionally with a retain hint.

Request (optional):
```json
{"retain_hint": "string | null"}
```

Response: `{"message": "Compact command sent", "command": "/compact [hint]"}`

---

### Context

#### `GET /api/v1/sessions/{session_id}/context`
```json
{
  "tokens_used": 0,
  "tokens_total": 200000,
  "percent_used": 0.0,
  "compaction_recommended": false
}
```
Compaction recommended when `percent_used > 75.0`.

#### `POST /api/v1/context/count-tokens`
Proxy to Anthropic token count API. Falls back to character-based estimate.
Accepts any JSON body; returns `{"input_tokens": N}` or `{"input_tokens": N, "estimated": true}`.

---

### WebSocket

#### `WS /api/v1/ws/sessions/{session_id}`

Bidirectional WebSocket for a session.

Client sends JSON:
- `{"type": "message", "content": "..."}` — send message; server streams back `StreamEvent` objects then `{"type": "message_stop"}`
- `{"type": "interrupt"}` — SIGINT; server replies `{"type": "interrupt_ack"}`
- `{"type": "command", "name": "/compact"}` — slash command; server replies `{"type": "command_sent", "command": "..."}`

Close code `4004` if session not found.

---

### Models

#### `GET /api/v1/models`
List models. Tries Anthropic API live if key available, falls back to static list.
Response: `{"data": [{id, name, context_window}], "source": "fallback|live"}`

#### `GET /api/v1/models/current`
`{"model": "claude-opus-4-5"}`

#### `PUT /api/v1/models/current`
Request: `{"model": "claude-sonnet-4-5"}`
Response: `{"model": "...", "updated": true}`

---

### Config

#### `GET /api/v1/config?level=user|project|local`
Read `settings.json`. Levels: `user` (~/.claude/), `project` ({cwd}/.claude/), `local` ({cwd}/.claude/settings.local.json).

#### `PATCH /api/v1/config?level=user|project|local`
Shallow merge-update settings. Request: `{"data": {...}}`.

---

### Permissions

#### `GET /api/v1/config/permissions`
`{"allow": [...], "deny": [...], "ask": [...], "defaultMode": "default"}`

#### `POST /api/v1/config/permissions` (201)
Add permission rule. Request: `{"rule": "Bash", "list": "allow|deny|ask"}`

#### `DELETE /api/v1/config/permissions/{rule_id}` (204)
URL-encoded rule string as path segment.

#### `PUT /api/v1/config/permissions/mode`
Request: `{"mode": "string"}`

---

### Auth

#### `GET /api/v1/auth/status`
`{"authenticated": bool, "method": "api_key|null", "account": "masked_key|null", "provider": "anthropic|null"}`

#### `POST /api/v1/auth/login`
Request: `{"method": "api_key", "api_key": "sk-..."}`  <!-- pragma: allowlist secret -->

#### `POST /api/v1/auth/logout`
`{"authenticated": false, "message": "Logged out"}`

---

### Hooks

#### `GET /api/v1/config/hooks`
`{"hooks": [{id, event, command, matcher}]}`

#### `POST /api/v1/config/hooks` (201)
Request: `{"event": "PreToolUse", "command": "...", "matcher": "string|null"}`

#### `PUT /api/v1/config/hooks/{hook_id}`
Request: `{"event": "string|null", "command": "string|null", "matcher": "string|null"}`

#### `DELETE /api/v1/config/hooks/{hook_id}` (204)

---

### MCP Servers

#### `GET /api/v1/mcp`
`{"servers": [{name, command, args, env, transport}]}`

#### `POST /api/v1/mcp` (201)
Request: `{"name": "...", "command": "...", "args": [...], "env": {...}, "transport": "..."}`

#### `DELETE /api/v1/mcp/{server_name}` (204)

---

### Memory (CLAUDE.md)

#### `GET /api/v1/memory?scope=project|user`
`{"scope": "project", "path": "...", "content": "...", "exists": true}`

#### `PUT /api/v1/memory`
Request: `{"content": "...", "scope": "project|user"}`

---

### Tools

#### `GET /api/v1/tools`
`{"tools": [{name, permission: "allow|deny|ask|default"}]}`

#### `PUT /api/v1/tools/{name}/permission`
Request: `{"permission": "allow|deny|ask"}`

---

### Commands (Slash Commands)

#### `GET /api/v1/commands?scope=all|project|user`
`{"commands": [{name, scope, path}]}`

#### `POST /api/v1/commands` (201)
Request: `{"name": "...", "content": "markdown", "scope": "project|user"}`

#### `GET /api/v1/commands/{name}`
`{"name": "...", "path": "...", "content": "..."}`

#### `PUT /api/v1/commands/{name}`
Request: `{"content": "..."}`

#### `DELETE /api/v1/commands/{name}` (204)

#### `POST /api/v1/commands/{name}/execute`
Request: `{"session_id": "uuid", "arguments": ""}`
Response: `{"command": "/name args", "session_id": "...", "sent": true}`

---

### Diagnostics

#### `GET /api/v1/diagnostics`
`{"ok": bool, "checks": {"auth": {ok, message}, "cli": {ok, version, message}, "node": {ok, version, message}, "settings": {ok, message}}}`

#### `GET /api/v1/diagnostics/version`
`{"cli_version": "...", "service_version": "...", "python_version": "..."}`

---

## 3. Port Registry

`~/.claude-mpm/global-port-registry.json` — currently has empty `allocations: {}`, updated timestamp.
Not used by serve daemon for port assignment (port is explicit via `--port`).

---

## 4. What needs updating in `MpmHttpClient`

The existing `MpmHttpClient` targets the **legacy `MessageEndpoint`** (port 7856, `--inject-port` mode).

New work needed:

### New endpoints on legacy MessageEndpoint (port 7856)

These are new since the original implementation:
- `GET /session` — SDK session state
- `GET /activity?limit=50` — agent activity stream
- `GET /monitor` — monitor agent availability
- `GET /history` — recent injection history

### Entire new client for ui_service daemon (port 7777)

A `UiServiceClient` (or extend `MpmHttpClient`) should cover:

**Priority 1 (core session management):**
- `POST /api/v1/sessions` — create session
- `GET /api/v1/sessions/{id}` — get state
- `DELETE /api/v1/sessions/{id}` — terminate
- `POST /api/v1/sessions/{id}/messages` — send message (streaming + non-streaming)
- `GET /api/v1/health` — health check

**Priority 2 (session control):**
- `GET /api/v1/sessions` — list sessions
- `PATCH /api/v1/sessions/{id}` — update properties
- `POST /api/v1/sessions/{id}/interrupt` — SIGINT
- `POST /api/v1/sessions/{id}/fork` — fork
- `PUT /api/v1/sessions/{id}/plan-mode` — toggle plan mode

**Priority 3 (context + utility):**
- `GET /api/v1/sessions/{id}/context` — token usage
- `GET /api/v1/sessions/{id}/messages` — history
- `DELETE /api/v1/sessions/{id}/messages` — clear
- `POST /api/v1/sessions/{id}/messages/compact` — compact
- `GET /api/v1/diagnostics` — health checks
- `GET /api/v1/diagnostics/version` — versions

**WebSocket:** `WS /api/v1/ws/sessions/{session_id}` for real-time streaming.

---

## 5. Lifecycle Notes

- Session state persisted to `~/.claude-mpm/sessions/{session_id}.json` (metadata only, process not reattached on restart)
- Max sessions: 10 (configurable via `CLAUDE_MPM_UI_MAX_SESSIONS`)
- Session timeout: 60 minutes idle (configurable via `CLAUDE_MPM_UI_SESSION_TIMEOUT`)
- Background cleanup loop runs every 60 seconds
- Startup detection: poll for PID file + port binding, max 30s
- Shutdown: SIGTERM to all subprocesses via `ProcessManager.stop()`
- CORS: allows all localhost/127.0.0.1 origins with any port

---

## Files Read

- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/serve_daemon.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/app.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/config.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/process_manager.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/models/session.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/models/message.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/sessions.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/messages.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/commands.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/auth.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/config.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/diagnostics.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/hooks.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/mcp.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/memory.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/models.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/permissions.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/ui_service/routers/tools.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/services/agents/message_endpoint.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/cli/commands/serve.py`
- `/Users/masa/Projects/claude-mpm/src/claude_mpm/cli/parsers/serve_parser.py`
- `/Users/masa/Projects/ai-commander/crates/mpm-sdk/src/http_client.rs`
