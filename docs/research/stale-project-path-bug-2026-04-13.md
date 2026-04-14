# Stale Project Path Bug - "cto" Project

**Date:** 2026-04-13
**Status:** Root cause identified, fix options documented

## Problem

The "cto" project displays a stale/wrong path in status and listings. The tmux session is connected and working, but the path shown is old.

## Root Cause

**Stored path (stale):** `/Users/masa/Clients/Duetto/CTO`
- File: `~/.ai-commander/projects/proj-d46ff7ea-7672-4fcb-98e4-8e4d01bd938e.json`
- Registered: 2026-02-15
- This path does NOT exist on disk (returns empty from `ls`)

**Actual tmux working directory:** `/Users/masa/Duetto/cto`
- This path exists and is the real project directory
- Confirmed via `tmux display-message -p -t cto '#{pane_current_path}'`

The project was registered on Feb 15 when it lived at `/Users/masa/Clients/Duetto/CTO`. At some point the directory was moved to `/Users/masa/Duetto/cto`, but the stored project JSON was never updated.

## How Paths Flow Through the System

### Storage Layer
- Projects stored as JSON files in `~/.ai-commander/projects/`
- `crates/commander-persistence/src/state_store.rs` - `StateStore` reads/writes these
- `crates/commander-models/src/project.rs` - `Project` struct has `path: String` field (line 195)
- Path is set at registration time and never updated thereafter

### Display Paths

1. **API `/api/projects`** (web UI project list):
   - `crates/commander-api/src/handlers/projects.rs` -> `list_projects()` (line 19)
   - Returns `ProjectSummary` which copies `project.path.clone()` directly from stored JSON
   - Always shows the stored (stale) path

2. **Telegram `/status`**:
   - `crates/commander-telegram/src/handlers.rs` -> line 990: calls `state.get_session_status()`
   - `crates/commander-telegram/src/state.rs` -> line 775: returns `session.project_path.clone()`
   - The `project_path` in `UserSession` is set at `/connect` time:
     - **Registered project path** (line 825-828): uses `project.path.clone()` from stored JSON
     - **Unregistered tmux fallback** (line 943-944): uses `get_tmux_cwd()` which gets the ACTUAL path
   - So if "cto" connects via registered project lookup (Try 1, line 798), it gets the stale stored path

3. **Web UI session list** (`/api/sessions`):
   - `crates/commander-api/src/handlers/web.rs` -> `list_sessions()` (line 185)
   - Only returns tmux session name + pane count, does NOT include path
   - Path is not shown in session listings (only in project listings)

### Registration
- REPL `register` command in `crates/ai-commander/src/repl.rs`
- Web API `POST /api/projects` in `crates/commander-api/src/handlers/projects.rs`
- Telegram `/connect <path> -a <adapter> --name <name>`
- All set `path` once at creation time; no mechanism updates it later

## The Stale File

```
~/.ai-commander/projects/proj-d46ff7ea-7672-4fcb-98e4-8e4d01bd938e.json
```

Contents:
```json
{
  "id": "proj-d46ff7ea-7672-4fcb-98e4-8e4d01bd938e",
  "path": "/Users/masa/Clients/Duetto/CTO",
  "name": "cto",
  "state": "idle",
  "config_loaded": false,
  "config": { "tool": "mpm" },
  "sessions": {},
  "work_queue": [],
  "completed_work": [],
  "pending_events": [],
  "event_history": [],
  "thread": [],
  "created_at": "2026-02-15T10:37:22.439988Z"
}
```

## Fix Options

### Quick Fix (Manual)

Edit the JSON file directly:
```bash
# Update the path in the stored project file
# Change: "path": "/Users/masa/Clients/Duetto/CTO"
# To:     "path": "/Users/masa/Duetto/cto"
```

File: `~/.ai-commander/projects/proj-d46ff7ea-7672-4fcb-98e4-8e4d01bd938e.json`

### Systemic Fix Option A: Validate + warn on connect

In `crates/commander-telegram/src/state.rs` around line 802-803, the code already calls `validate_project_path(&project.path)` which checks the path exists. If the directory was moved, this should fail and the connect would fall through to "Try 2" (tmux session lookup at line 929-960) which uses `get_tmux_cwd()` to get the real path.

However, if the old path `/Users/masa/Clients/Duetto/CTO` still exists as an empty directory or symlink, validation passes and the stale path is used.

### Systemic Fix Option B: Cross-check tmux CWD at connect time

When connecting to a registered project that has a matching tmux session, compare the stored path with `get_tmux_cwd()`. If they differ, update the stored path:

```rust
// In state.rs connect(), after finding registered project:
if tmux.session_exists(&session_name) {
    if let Some(tmux_cwd) = get_tmux_cwd(&session_name).await {
        if tmux_cwd != project.path {
            warn!("Project path mismatch: stored={} tmux={}", project.path, tmux_cwd);
            // Optionally update stored path
        }
    }
}
```

### Systemic Fix Option C: Add `update-path` command

Add a REPL/API command to update a project's path:
```
aic> update-path cto /Users/masa/Duetto/cto
```

## Recommendation

1. **Immediate**: Manually edit the JSON file to fix the "cto" path
2. **Short-term**: Add an `update-path` REPL command for easy path changes
3. **Long-term**: Cross-check tmux CWD on connect and auto-update (Option B)
