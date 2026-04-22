# Research: Why Project Paths Are Missing in the GUI

**Date:** 2026-04-15
**Scope:** `crates/commander-gui`, `crates/commander-api`, `crates/commander-models`

---

## Summary

Project paths are not shown in the GUI because the `Session` type exposed to the frontend (`SessionInfo` in Tauri commands / `SessionSummary` in the API) does not include a `path` field. Paths exist in the backend `Project` model but are never threaded through to the session list response, and the Svelte components never attempt to render them.

Additionally, `DashboardView.svelte` imports and uses several store exports (`currentView`, `hydrateSessionMessages`, `hiddenSessions`, `hideSession`, `unhideAll`) and `Session` interface fields (`is_active`, `status_line`) that **do not exist** in `app.ts`, which means DashboardView will fail at runtime.

---

## Findings

### 1. Where paths would be displayed — Svelte UI

**`crates/commander-gui/ui/src/lib/components/SessionList.svelte`**
- Lines 287-308: Renders each session as a button showing only `session.name`.
- No reference to `path` anywhere in the file.
- Uses the `Session` interface from `app.ts`.

**`crates/commander-gui/ui/src/lib/components/DashboardView.svelte`**
- Lines 185-188: Renders `session.name` and optionally `session.status_line` (the latter is a new field not in the store interface).
- No reference to `path` anywhere in the file.
- Line 4: Imports `currentView`, `hydrateSessionMessages`, `hiddenSessions`, `hideSession`, `unhideAll` — **none of these are exported from `app.ts`** (the current store file).

### 2. The frontend `Session` interface — `app.ts`

**`crates/commander-gui/ui/src/lib/stores/app.ts` lines 3-7:**
```typescript
export interface Session {
  name: string;
  created_at: string;
  is_connected: boolean;
}
```
- No `path` field.
- No `is_active` field (used by DashboardView lines 36, 104, 165, 181, 182, 192).
- No `status_line` field (used by DashboardView lines 37, 186, 187).
- No `currentView`, `hydrateSessionMessages`, `hiddenSessions`, `hideSession`, `unhideAll` exports (imported by DashboardView line 4 and CommandPalette.svelte line 7-10).

### 3. How sessions are fetched

**Tauri (desktop app):** `SessionList.svelte` and `DashboardView.svelte` both call `invoke('list_sessions')` at lines 98 and 45 respectively. This invokes:

**`crates/commander-gui/src/commands.rs` lines 76-107:**
```rust
pub struct SessionInfo {
    pub name: String,
    pub created_at: String,
    pub is_connected: bool,
}

pub async fn list_sessions(state: State<'_, GuiState>) -> Result<Vec<SessionInfo>, String> {
    // maps TmuxSession → SessionInfo { name, created_at, is_connected }
}
```
- No `path` field in `SessionInfo`.
- No `is_active` field.
- No `status_line` field.

**Web UI:** `GET /api/sessions` handled by:

**`crates/commander-api/src/handlers/web.rs` lines 32-48:**
```rust
pub struct SessionSummary {
    pub name: String,
    pub pane_count: usize,
    pub is_commander: bool,
}
```
- Also has no `path` field.

### 4. The data model — does it have a `path` field?

**`crates/commander-models/src/project.rs` lines 188-258 — `Project` struct:**
```rust
pub struct Project {
    pub id: ProjectId,
    pub path: String,   // ← path IS present here
    pub name: String,
    pub state: ProjectState,
    // ...
}
```
- `path` exists at the domain model level.
- The `Project` struct is used in `commander-api/src/handlers/projects.rs` (separate endpoint), not in the session list.
- The `list_sessions` command operates on **tmux sessions** (`TmuxSession`), not on `Project` entities.

### 5. Is `path` populated or null?

`path` is populated in `Project::new(path, name)` and is required (not Option). However:
- The session list endpoints (`/api/sessions` and `invoke('list_sessions')`) are based on **tmux session metadata**, not the `Project` registry.
- The tmux `TmuxSession` struct does not carry a `path` field at all.
- There is no join between a tmux session name and a `Project.path` in either the Tauri command or the API handler.

---

## Root Cause Chain

```
Project.path  (commander-models)
    ↓  NOT threaded through
TmuxSession   (commander-tmux)
    ↓  mapped to
SessionInfo   (commander-gui/src/commands.rs)   — no path
SessionSummary (commander-api/src/handlers/web.rs) — no path
    ↓  deserialized as
Session interface (app.ts)                       — no path field
    ↓  rendered in
SessionList.svelte / DashboardView.svelte        — never reads path
```

---

## Secondary Issue: DashboardView Uses Non-Existent Store Exports

`DashboardView.svelte` (line 4) imports symbols that are absent from `app.ts`:
- `currentView` — not exported
- `hydrateSessionMessages` — not exported
- `hiddenSessions` — not exported
- `hideSession` — not exported
- `unhideAll` — not exported

And uses `Session` fields that don't exist in the interface:
- `session.is_active` (lines 36, 104, 165, 181, 182, 192)
- `session.status_line` (lines 37, 186, 187)

This means `DashboardView.svelte` references a **newer version of `app.ts`** that has not been committed / is out of sync.

---

## Files and Line Numbers

| File | Lines | Issue |
|------|-------|-------|
| `crates/commander-gui/ui/src/lib/stores/app.ts` | 3-7 | `Session` interface missing `path`, `is_active`, `status_line` |
| `crates/commander-gui/ui/src/lib/stores/app.ts` | (entire file) | Missing exports: `currentView`, `hydrateSessionMessages`, `hiddenSessions`, `hideSession`, `unhideAll` |
| `crates/commander-gui/src/commands.rs` | 76-81 | `SessionInfo` struct missing `path` field |
| `crates/commander-gui/src/commands.rs` | 90-107 | `list_sessions` command never populates path |
| `crates/commander-api/src/handlers/web.rs` | 32-48 | `SessionSummary` missing `path`, `is_active`, `status_line` |
| `crates/commander-models/src/project.rs` | 195 | `path: String` exists but never flows to session list |
| `crates/commander-gui/ui/src/lib/components/SessionList.svelte` | 287-308 | Renders name only, no path |
| `crates/commander-gui/ui/src/lib/components/DashboardView.svelte` | 4, 36-37 | Imports missing store symbols; uses undefined Session fields |

---

## Recommended Fix

### Option A: Add `path` to the tmux session layer (simplest)
1. Add `path: Option<String>` to `SessionInfo` in `commands.rs`.
2. When building `SessionInfo`, look up the matching `Project` by session name and populate `path` from `Project.path`.
3. Add `path?: string` to the `Session` interface in `app.ts`.
4. Render `session.path` in `SessionList.svelte` and `DashboardView.svelte`.

### Option B: Resolve the `app.ts` sync issue first
The `DashboardView.svelte` appears to be written against a newer `app.ts`. The missing exports and fields suggest `app.ts` needs to be updated to match what `DashboardView.svelte` expects before adding `path` support.
