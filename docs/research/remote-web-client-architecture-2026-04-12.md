# Remote Web Client Architecture Research

**Date:** 2026-04-12
**Scope:** HTTP API, claude-mpm serve API, Svelte GUI, auth/CORS, session management

---

## 1. commander-api (Axum, port 8765)

Endpoints in `/api/v1`:

| Method | Path | What it does |
|--------|------|--------------|
| GET | `/api/health` | Status + version |
| GET/POST | `/api/projects` | List / create projects |
| GET/DELETE | `/api/projects/{id}` | Get / delete project |
| POST | `/api/projects/{id}/start` | Start project instance (tmux) |
| POST | `/api/projects/{id}/stop` | Stop instance |
| POST | `/api/projects/{id}/send` | Send text to running tmux session |
| GET/POST | `/api/events` | List / query events |
| POST | `/api/events/{id}/acknowledge` | Ack event |
| POST | `/api/events/{id}/resolve` | Resolve event |
| GET/POST | `/api/work` | List / create work items |
| POST | `/api/work/{id}/complete` | Complete work item |
| GET | `/api/adapters` | List adapters (includes `claude-code`, `mpm`) |

**Gaps:** No SSE/WebSocket stream endpoint. `send` is fire-and-forget via tmux; output is not returned. No endpoint to read session output.

**Default bind:** `127.0.0.1:8765` — localhost only.

**CORS:** `CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any)` — fully open, no credentials check.

**Auth:** None. Zero token/key mechanism.

---

## 2. claude-mpm serve (UiServiceClient, port 7777)

This is a separate FastAPI Python daemon. The Rust `UiServiceClient` in `crates/mpm-sdk/src/serve_client.rs` wraps it:

| Method | Path | What it does |
|--------|------|--------------|
| GET | `/api/v1/health` | Health check |
| GET/POST | `/api/v1/sessions` | List / create-or-resume sessions |
| GET/DELETE | `/api/v1/sessions/{id}` | Get state / terminate |
| POST | `/api/v1/sessions/{id}/messages` | Send message; `stream: false` returns `{events:[]}`, `stream: true` returns SSE |
| GET | `/api/v1/sessions/{id}/context` | Token usage |
| POST | `/api/v1/sessions/{id}/interrupt` | SIGINT to subprocess |
| DELETE | `/api/v1/sessions/{id}/messages` | Clear history |

**SSE streaming:** Yes — `POST /messages` with `stream: true` returns `text/event-stream`. Events: `text`, `assistant`, `tool_use`, `message_stop`, `result`, `error`.

**Default port:** 7777 (hardcoded in comments; configurable via `--port`).

**Bind:** `127.0.0.1` — localhost only by default.

**Auth:** None observed.

---

## 3. commander-gui (Tauri + Svelte)

- Tauri backend (`crates/commander-gui/src/`) exposes Tauri IPC commands: `list_sessions`, `connect_session`, `disconnect_session`, `send_message`, `stop_session`, `start_bot`, `stop_bot`, `create_session`, `generate_pairing_code`.
- All session I/O goes through tmux directly, not through the HTTP API.
- Svelte frontend uses `invoke()` from `@tauri-apps/api/core` and `listen()` for `session-output` events — **hard Tauri dependency, not browser-portable**.
- The UI is a standard SPA (SessionList, ChatView, InputArea, BotStatus, plus new DashboardView, SettingsModal, CommandPalette components). Vite build target is `es2021/chrome100/safari13`.
- **Could be served standalone** if the Tauri `invoke`/`listen` calls are replaced with fetch + EventSource calls to commander-api or the mpm serve API. The store layer (`app.ts`) is already decoupled from transport.

---

## 4. Auth & Remote Access

| Concern | Current state |
|---------|---------------|
| Auth tokens | None anywhere |
| CORS | `allow_origin(Any)` — open |
| Remote binding | Both APIs bind `127.0.0.1` by default |
| HTTPS | Not configured |

To accept remote connections: change `host` to `0.0.0.0` in `ApiConfig` and add a bearer-token middleware.

---

## 5. Session Management via API

**commander-api** can create projects and send tmux input but cannot stream output back — output is consumed only by the Tauri event bus.

**mpm serve API** has full session lifecycle (create/list/get/delete) and bidirectional messaging with SSE streaming. This is the better foundation for a remote web client.

---

## Gaps for Remote Web Client

1. **Output streaming** — commander-api has no SSE/WS endpoint; use mpm serve's `/api/v1/sessions/{id}/messages?stream=true`.
2. **Auth** — no token layer on either API; must add before exposing remotely.
3. **Remote binding** — both default to `127.0.0.1`; need `0.0.0.0` + TLS for remote use.
4. **Svelte frontend** — currently hard-wired to Tauri IPC; needs a fetch/EventSource transport layer to run as a standalone SPA against the HTTP APIs.
5. **Session output bridging** — the commander-api `send` endpoint writes to tmux but does not capture stdout; to build a web client on top of commander-api requires adding a tmux-to-SSE bridge, or routing through mpm serve instead.
