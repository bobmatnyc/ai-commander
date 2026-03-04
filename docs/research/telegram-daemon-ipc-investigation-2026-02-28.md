# Telegram Bot / Daemon Communication Investigation

**Date:** 2026-02-28
**Status:** Root cause identified

---

## Executive Summary

The Telegram bot and the `commander-daemon` **do not communicate with each other at all**. The `commander-daemon` was added in a recent "Phase 1" commit (commit `0e0bafd`) as a separate process that implements a Unix domain socket IPC server and JSON-RPC protocol. However, `commander-telegram` has zero integration with it. The bot communicates exclusively with **tmux** directly, bypassing the daemon entirely.

This is not a bug in the IPC wiring — there is simply no IPC wiring between the two processes.

---

## Architecture Discovery

### How the Telegram Bot Actually Works

The Telegram bot (`commander-telegram`) uses **tmux** as its sole inter-process communication layer:

1. User sends a command to the Telegram bot.
2. Bot calls `TmuxOrchestrator::send_line(session_name, None, message)` — writing the message directly to a tmux pane via `tmux send-keys`.
3. Bot polls `TmuxOrchestrator::capture_output(session_name, None, Some(200))` — reading pane output via `tmux capture-pane`.
4. When output stabilizes, it's returned to the user via Telegram.

Relevant code:
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`, lines 1037–1066 (`send_message`)
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`, lines 1105–1190 (`poll_output`)

### What the Daemon Provides (But Nobody Uses)

The `commander-daemon` crate (`crates/commander-daemon/`) implements:
- A Unix domain socket at `~/.ai-commander/state/daemon.sock`
- JSON-RPC 2.0 protocol (`ipc/protocol.rs`)
- IPC server (`ipc/server.rs`) that accepts connections and dispatches to handlers
- Methods: `session.create`, `session.send`, `session.list`, `pairing.generate`, etc.

The daemon is a **completely isolated binary**. Nothing connects to its socket. The Telegram bot's `Cargo.toml` does not list `commander-daemon` as a dependency. Zero code in `commander-telegram/src/` makes a socket connection.

### The `daemon.rs` Confusion

`commander-telegram/src/daemon.rs` is NOT an IPC client for `commander-daemon`. It manages the lifecycle of `commander-telegram` itself (starts/stops the Telegram bot process), reading and writing `~/.ai-commander/state/telegram.pid`. It has no relation to `commander-daemon`.

---

## File-Level Evidence

| File | What It Does | IPC Connection? |
|------|-------------|-----------------|
| `crates/commander-daemon/src/ipc/server.rs` | Listens on `daemon.sock` for JSON-RPC | Server (listener) |
| `crates/commander-daemon/src/ipc/protocol.rs` | Defines JSON-RPC message types | Protocol only |
| `crates/commander-telegram/src/daemon.rs` | Manages *telegram bot* PID lifecycle | None |
| `crates/commander-telegram/src/state.rs` | Sends messages via tmux | None (tmux only) |
| `crates/commander-telegram/Cargo.toml` | Dependencies | No `commander-daemon` dep |

---

## Daemon Runtime State

No daemon is running:
```
~/.ai-commander/state/
├── authorized_chats.json
├── bot_version.json
├── notifications.json
├── pairings.json
├── sessions/
├── telegram_sessions.json
└── telegram.pid   (contains PID 13279 - Telegram bot)
# daemon.sock is ABSENT
# daemon.pid is ABSENT
```

The daemon socket (`daemon.sock`) and daemon PID file (`daemon.pid`) do not exist. The daemon process is not running.

---

## Recent Changes That Introduced the Gap

Commit `0e0bafd` (2026-02-27): "feat(daemon): implement stable daemon architecture Phase 1"

This commit created the entire `commander-daemon` crate from scratch with the Unix socket IPC server, session management, and JSON-RPC protocol. However, it did NOT update `commander-telegram` to use this new IPC layer. The Telegram bot continued using the pre-existing tmux approach.

Subsequent daemon commits (`efa4ac4`, `b1eb9b5`, `3ff5f48`, `b45d10c`) only fixed warnings and improved daemonization — they did not add any Telegram integration.

---

## Root Cause

**The `commander-daemon` IPC server was implemented but the Telegram bot client side was never implemented.** The two processes have no communication channel between them.

The stated goal (Telegram bot sends commands to daemon, daemon manages Claude sessions) is architecturally correct but **Phase 1 only built the daemon server half**. Phase 2 (building the client in `commander-telegram`) was never committed.

---

## What Needs to Be Built

To connect the Telegram bot to the daemon:

### 1. Add `commander-daemon` as a dependency to `commander-telegram`

File: `/Users/masa/Projects/ai-commander/crates/commander-telegram/Cargo.toml`

```toml
commander-daemon = { path = "../commander-daemon" }
```

### 2. Create an IPC client in `commander-telegram`

A client needs to:
1. Connect to the Unix socket at `~/.ai-commander/state/daemon.sock` (path comes from `commander_core::config::runtime_state_dir().join("daemon.sock")`)
2. Send JSON-RPC requests (line-delimited JSON matching the protocol in `crates/commander-daemon/src/ipc/protocol.rs`)
3. Read JSON-RPC responses

The server uses `tokio::net::UnixListener` and line-delimited JSON. A client would use `tokio::net::UnixStream`.

The IPC message format (from `protocol.rs`):
```json
{"jsonrpc":"2.0","method":"session.send","params":{"session_id":"...","message":"..."},"id":1}
```

### 3. Replace tmux calls in `state.rs` with IPC calls

Current flow (lines 1053, 1089 of `state.rs`):
```rust
tmux.send_line(&session.tmux_session, None, message)
```

Target flow:
```rust
ipc_client.send_rpc("session.send", SessionSendParams { session_id, message }).await
```

### 4. Ensure the daemon is running before the bot starts

The daemon must be running and its socket must exist before the bot tries to connect.

---

## Why Commands Fail Now

When a user sends a Telegram message:
1. The bot looks for a registered `tmux_session` name in the user's session.
2. It calls `tmux send-keys` to that session.
3. If the tmux session does not exist (was never created, or was killed), the command fails silently or returns `TmuxError`.
4. The daemon's session management (which *would* handle this robustly) is never invoked.

The daemon cannot help because:
- It is not running (no `daemon.pid`, no `daemon.sock`)
- Even if it were running, nothing connects to it

---

## Immediate Diagnostic Steps

1. Check if tmux sessions exist: `tmux ls`
2. Check if the daemon is running: `ls ~/.ai-commander/state/daemon*`
3. Attempt to start the daemon manually: `./target/debug/commander-daemon start --foreground`
4. Verify the socket appears: `ls ~/.ai-commander/state/daemon.sock`

---

## Files Examined

- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/daemon.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/session.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/main.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/lib.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/Cargo.toml`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/main.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/service.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/ipc/mod.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/ipc/protocol.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/ipc/server.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/ipc/unix.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-core/src/config.rs`
