# Telegram Bot: Three Issues Mapped for Targeted Fixes

**Date:** 2026-04-06

---

## Issue 1: `/connect-new` name parameter should be optional

**Handler:** `parse_connect_args()` at `handlers.rs:485`; consumed in `handle_connect()` at `handlers.rs:647`.

**Current signature:** `/connect <path> -a <adapter> -n <name>` (all three flags required).

**Parsing logic** (`handlers.rs:493-527`): Splits on whitespace; first token is `path`; iterates remaining tokens matching `-a`/`--adapter` and `-n`/`--name` flags. Line 526 returns error if `name` is `None`: `"missing -n/--name <name>"`.

**Where `name` flows downstream:**
- `ConnectArgs::New { path, adapter, name }` (handlers.rs:524) passed to `state.connect_new(chat_id, &path, &adapter, &name)` at handlers.rs:695.
- In `state.rs:727`: `let session_name = format!("commander-{}", project.name)` -- the project name becomes the tmux session name.
- Project creation uses `name` as `project.name` in the store.

**What "default to directory name" means:**
- When `-n` is omitted, extract basename from path: e.g. `~/Projects/foo` yields `"foo"`.
- Implementation: In `parse_connect_args`, if `name` is `None` after flag parsing, set `name = path.rsplit('/').next().unwrap_or("unnamed")`.
- Approx 3-line change in the `match (adapter, name)` block at line 523.

---

## Issue 2: `/stop` not working for event-driven sessions

**Handler:** `handle_stop()` at `handlers.rs:1719`.

**Root cause: tmux gate at line 1806-1826.** The flow is:
1. Resolve session name to `commander-{name}` format (line 1744 or 1765).
2. Get tmux orchestrator (line 1807-1814) -- **fails with "tmux not available" if no tmux**.
3. Check `tmux.session_exists(&session_name)` (line 1816) -- **event-driven sessions have synthetic names like `event-driven-{project}`, NOT `commander-{name}`, and have NO tmux session**. This returns false, printing "Session not found".
4. Even if it passed, line 1860 calls `tmux.destroy_session()` which is meaningless for event-driven sessions.

**The disconnect flow does work** (`state.rs:851-877`): `disconnect()` correctly calls `session.event_handle.take_handle()` then `adapter.stop(handle)` -- but `handle_stop` never reaches `disconnect()` because it bails at the tmux existence check.

**Fix needed:** Before the tmux gate, check if session is event-driven (via `is_event_driven_session()` from `state.rs:235`). If so, skip tmux checks/destroy, call `state.disconnect()` directly, and report success.

---

## Issue 3: `/ls` missing sessions due to "commander-" prefix

**Handler:** `handle_list()` at `handlers.rs:1105`.

**How it lists sessions:** Calls `state.list_tmux_sessions_with_status()` (line 1122), which queries **tmux only** (`state.rs:2216-2252`). It calls `tmux.list_sessions()` and maps results.

**The "commander-" prefix logic:**
- `state.rs:2206/2226`: `is_commander = s.name.starts_with("commander-")` -- used for display icon (robot vs terminal).
- `handlers.rs:1175`: `display_name = name.strip_prefix("commander-").unwrap_or(name)` -- strips for display.
- Current session matching at `handlers.rs:1136`: `format!("commander-{}", path.rsplit('/').next()...)` -- assumes `commander-` prefix.

**What sessions are missed:**
- **Event-driven sessions** (`event-driven-{project}`) have NO tmux session at all. They exist only in the in-memory `sessions` map. `list_tmux_sessions_with_status()` queries tmux, so event-driven sessions are completely invisible to `/ls`.
- Sessions that don't follow the `commander-` naming convention would show with a terminal icon instead of robot icon, but would still appear.

**Where "commander-" prefix is added** (key locations in state.rs):
- `state.rs:727`: `format!("commander-{}", project.name)` -- normal session creation
- `state.rs:808/1034`: `format!("commander-{}", base_name)` -- worktree/topic sessions  
- `state.rs:690`: `format!("event-driven-{}", project.name)` -- event-driven (different prefix)

**Where prefix is stripped** (for display/lookup):
- `state.rs:659/894`: `.strip_prefix("commander-")` for session lookup normalization
- `state.rs:1801`: `.strip_prefix("commander-")` for base name extraction
- `handlers.rs:189/280/1175/1239`: `.strip_prefix("commander-")` for display

**Fix needed:** `handle_list` should also query the in-memory sessions map for event-driven sessions and merge them into the list. Event-driven sessions should appear with a distinct marker (e.g. a different emoji). The current tmux-only query is the fundamental gap.

---

## Summary of Required Changes

| Issue | File:Line | Change |
|-------|-----------|--------|
| 1 | `handlers.rs:523-527` | Default `name` to path basename when `-n` omitted |
| 2 | `handlers.rs:1806-1826` | Add event-driven branch before tmux gate; call `disconnect()` directly |
| 3 | `handlers.rs:1122` | Merge in-memory event-driven sessions with tmux session list |
