# Session Deduplication Analysis: How "open-mpm-27" and "open-mpm-9" Point to the Same Tmux Session

**Date:** 2026-04-24
**Objective:** Understand how two distinct session entries in the AIC GUI can point to the same underlying work context, and what data is available to detect or prevent this.

---

## 1. Session Data Model

The core session concept in AIC is the `ToolSession` struct in `crates/commander-models/src/project.rs`.

**`ToolSession` fields:**
| Field | Type | Purpose |
|---|---|---|
| `id` | `SessionId` | UUID-style unique ID (`sess-...`) |
| `project_id` | `ProjectId` | Parent project reference |
| `runtime` | `Option<String>` | Arbitrary runtime tag |
| `tmux_target` | `Option<String>` | The tmux session name or `session:window.pane` target |
| `status` | `String` | e.g., `"created"`, `"active"` |
| `output_buffer` | `Vec<String>` | Recent output lines |
| `created_at` | `DateTime<Utc>` | Creation timestamp |
| `last_output_at` | `Option<DateTime<Utc>>` | Last output timestamp |

**The tmux-linking field is `tmux_target: Option<String>`.**

However, the GUI layer (`commander-gui`) does **not** use `ToolSession` at all for its session list. It works directly against the live `tmux ls` output, using the tmux session name as the primary key. The `ToolSession` model exists in `commander-models` and is stored in project JSON files under `~/.ai-commander/projects/`, but the GUI `list_sessions` command bypasses that and queries tmux directly.

---

## 2. How Sessions Are Listed (GUI Path)

**File:** `crates/commander-gui/src/commands.rs` → `list_sessions()`

The GUI does the following on each poll:
1. Calls `tmux.list_sessions()` to get live sessions from tmux.
2. For each live session, queries `tmux display-message -p -t <name> #{pane_current_path}` to get the working directory.
3. Scans `~/.ai-commander/projects/*.json` for a `ProjectStub` whose `name` (sanitized) or `path` matches the session.
4. Applies display-name overrides from `~/.ai-commander/session-overrides.json`.
5. Appends "registered-only" entries for project JSONs with no matching live tmux session.

**The session list entry struct (`SessionInfo`):**
| Field | Meaning |
|---|---|
| `name` | Raw tmux session name (e.g., `open-mpm-27`) |
| `created_at` | Timestamp from tmux |
| `is_connected` | In `connected_sessions` set |
| `path` | Pane's current working directory |
| `nickname` | Project name or user override |
| `session_state` | `"connected"`, `"disconnected"`, `"registered"` |

---

## 3. How `connect_session` Works

**File:** `crates/commander-gui/src/commands.rs` → `connect_session()`

`connect_session(name)` does:
1. Lists live tmux sessions by name.
2. Calls `resolve_to_tmux_session(input, &live_sessions)` — a fuzzy resolver that tries:
   - Exact name match
   - Case-insensitive match
   - Bracket-suffix stripping (`"foo [bar]"` → `"foo"`)
   - Override map lookup
   - Project JSON path match (pane cwd)
   - Auto-create if registered project has no live session
3. Calls `tmux has-session -t <resolved>` to confirm existence.
4. Inserts the **resolved tmux session name** into `connected_sessions` (an in-memory `HashSet<String>`).
5. Sets `current_session` to the resolved name.
6. Returns the log history for that session name.

**Key point:** `connect_session` connects to a specific tmux session name. It does not detect that two session names share the same underlying tmux pane content.

---

## 4. Root Cause: Tmux Session Groups

Live system evidence (`tmux ls` output):
```
open-mpm-27: 1 windows (created Fri Apr 24 09:13:22 2026) (group open-mpm) (attached)
open-mpm-9:  1 windows (created Wed Apr 22 07:42:11 2026) (group open-mpm) (attached)
```

Both sessions:
- Are in tmux **group** `open-mpm`
- Have pane cwd `/Users/masa/Projects/open-mpm`

**Tmux session groups** are created with `tmux new-session -t <existing-session>`. A grouped session shares the same window and pane content as its group peers — every keystroke, every output line, every terminal state is mirrored across all sessions in the group. This is tmux's built-in multi-terminal view feature (used for pair programming, multi-monitor setups, etc.).

**The `-27` and `-9` suffixes** are tmux's automatic counter appended when you create a new session with a given group name but no explicit session name. The counter is a global tmux integer incremented per new session across the entire tmux server lifetime (not per-group). That is why the numbers are not consecutive — other sessions were created between them.

**How this specific pair arose:** Someone ran (or a tool ran):
```bash
tmux new-session -d -s open-mpm-9 -t open-mpm   # creates session grouped to open-mpm
tmux new-session -d -s open-mpm-27 -t open-mpm  # or with -t <existing grouped session>
```
Or tmux auto-named them when creating grouped sessions without explicit `-s` names.

**Neither AIC nor commander-gui creates grouped sessions.** The AIC `create_session` command calls `tmux new-session -d -s <name> -c <dir>` (no `-t` flag), and `try_auto_create_registered_session` does the same. Grouped sessions are created externally (manually or by scripts outside AIC).

---

## 5. Where Duplicates Enter the AIC Session List

The `list_sessions` command has one explicit dedup guard:
```rust
let mut seen = std::collections::HashSet::new();
// ...
if !seen.insert(s.name.clone()) {
    return None; // duplicate tmux name — skip
}
```
This prevents the same tmux session **name** from appearing twice. It does **not** prevent two different session names that share the same tmux group (and thus the same content) from both appearing.

The `registered-only` dedup logic additionally checks:
- `matched_project_names` (project already matched by name)
- `emitted_paths` (project already matched by path, case-insensitive)
- `seen` (tmux session name already in list)

Because `open-mpm-27` and `open-mpm-9` have the **same** path (`/Users/masa/Projects/open-mpm`), the **second** one to be processed in the live-session loop gets `path = Some("/Users/masa/Projects/open-mpm")`. The `emitted_paths` check is only applied to the **registered-only** appendage loop, not to the live-session loop. Both grouped sessions pass through the live-session loop as distinct entries (different `name`), and both emit the same `path`.

**The project JSON matching is also affected:** Both sessions match the same project JSON (by path), so both get the same `nickname`. The session-list result contains two rows with:
- Different `name` values (`open-mpm-9`, `open-mpm-27`)
- Same `path` (`/Users/masa/Projects/open-mpm`)
- Same `nickname` (if a project JSON for that path exists)
- Same tmux group (`open-mpm`, though this is not surfaced in `SessionInfo`)

---

## 6. What Field Links a Session Record to Tmux

There are two separate concepts:

| Layer | Identifier | Type | Notes |
|---|---|---|---|
| `ToolSession` (models) | `tmux_target` | `Option<String>` | Set to the tmux session name (or target) when the session is started by the runtime executor |
| GUI `SessionInfo` | `name` | `String` | Raw tmux session name from `tmux ls` |
| GUI connected set | key in `HashSet<String>` | `String` | Raw tmux session name |

The models-layer `tmux_target` is set by `commander-runtime/src/executor.rs` when spawning an adapter process. The GUI layer does not use `ToolSession` at all — it derives its session list directly from `tmux ls`.

---

## 7. Dedup Options

### Option A: Detect Tmux Session Groups (Recommended — Low Cost)

Query `#{session_group}` alongside `#{session_name}` and `#{session_created}` in `TmuxOrchestrator::list_sessions()`:

```rust
// In orchestrator.rs, change the format string:
self.run_tmux(&["list-sessions", "-F", "#{session_name}:#{session_created}:#{session_group}"])
```

Add `group: Option<String>` to `TmuxSession`. In `list_sessions` command:
- When building the session list, deduplicate by `(group, path)`: if two sessions share the same non-empty group, keep only the one with the lowest `created_at` (the "original") and omit the rest, or merge them with a UI indicator.

This is the most direct fix because tmux already tracks groups authoritatively.

### Option B: Deduplicate by Path in the Live-Session Loop

In `list_sessions` (GUI commands), extend the `seen` set to also track paths:
```rust
let mut seen_paths: HashSet<String> = HashSet::new();
// In the live-session filter_map:
if let Some(ref p) = path {
    if !seen_paths.insert(p.to_lowercase()) {
        return None; // same-path duplicate — skip
    }
}
```

Simpler but has a false-positive risk: two legitimate sessions in the same directory (e.g., a shell and a claude session both in `/Users/masa/Projects/mpm`) would be collapsed. Could be mitigated by only deduplicating when both sessions also have the same tmux group.

### Option C: UI Warning (Non-Invasive)

Surface `session_group` in `SessionInfo` and let the frontend show a badge or tooltip:
> "This session shares content with open-mpm-9"

No sessions are hidden, but the user understands the relationship. This is useful if grouped sessions are an intentional workflow (multi-monitor).

### Option D: Unique Constraint on `tmux_target` in Models Layer

Add validation in `ToolSession` creation to refuse a second session with the same `tmux_target` value. This only applies to the models/runtime path, not the GUI direct-tmux path, so it would not prevent the observed duplicate display.

### Option E: Merge-on-Connect

In `connect_session`, after resolving to a tmux session name, check whether that session is in a group with other sessions already in `connected_sessions`. If so, emit a frontend event that says "this session is grouped with X — connecting both." No data change, just UI transparency.

---

## 8. Summary

| Question | Answer |
|---|---|
| Session data model fields | `id`, `project_id`, `runtime`, `tmux_target`, `status`, `output_buffer`, `created_at`, `last_output_at` |
| Field that maps to tmux | `tmux_target` (models layer); raw `name` from `tmux ls` (GUI layer) |
| How duplicates arise | Tmux session groups (`tmux new-session -t <group>`) create multiple session names that share one pane; AIC lists each tmux session as a separate row without reading `#{session_group}` |
| Where in creation path | External to AIC — grouped sessions are created manually or by scripts; AIC never emits grouped sessions itself |
| Best dedup option | Read `#{session_group}` in `TmuxOrchestrator::list_sessions()`, surface it in `SessionInfo`, and deduplicate in `list_sessions` by `(group, path)` pair |
| Existing dedup | `seen: HashSet<String>` deduplicates by exact tmux session name only; does not cover same-group/same-path pairs |

---

## 9. Files to Change for the Group-Based Fix

1. `crates/commander-tmux/src/session.rs` — Add `group: Option<String>` to `TmuxSession` and update `parse()` to accept a 3-field format.
2. `crates/commander-tmux/src/orchestrator.rs` → `list_sessions()` — Change format string to include `#{session_group}`.
3. `crates/commander-gui/src/commands.rs` → `list_sessions()` — Add `session_group: Option<String>` to `SessionInfo`; extend dedup logic to skip same-group sessions.
4. `crates/commander-gui/ui/src/lib/components/SessionList.svelte` — Optionally show a group indicator badge.
