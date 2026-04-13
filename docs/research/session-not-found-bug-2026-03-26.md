# Bug Investigation: /ls Lists Sessions That Return "Session Not Found"

**Date:** 2026-03-26
**Symptom:** `/ls` lists sessions (e.g. "sfdc-9") but using them returns "Session 'sfdc-9' not found."

---

## Summary of Findings

This is NOT a data-source mismatch bug in the classic sense. Both `/ls` and the session lookup use the same underlying data source: **live tmux** via `TmuxOrchestrator::list_sessions()`. The bug is subtler: a **name-transformation mismatch** between what `/ls` displays and what the deep-link connect path validates against.

---

## 1. Where `/ls` Gets Its Session List

**Command routing:**
- `/ls` and `/list` both dispatch to `handle_list()` in `crates/commander-telegram/src/handlers.rs:1087`
- `handle_list` calls `state.list_tmux_sessions_with_status()` at line 1104

**Data source for `list_tmux_sessions_with_status()`:**
- Defined in `crates/commander-telegram/src/state.rs:1778`
- Calls `tmux.list_sessions()` directly on the live tmux server
- Returns ALL tmux sessions (both `commander-*` prefixed and bare)
- For display, strips the `commander-` prefix: `display_name = name.strip_prefix("commander-").unwrap_or(name)`

**Result for `sfdc-9`:**
- tmux name: `sfdc-9` (confirmed: `sfdc-9: 1 windows (created Thu Mar 12 06:36:21 2026)`)
- `sfdc-9`.strip_prefix("commander-") = `sfdc-9` (no prefix, no strip)
- Displayed in `/ls` as: `sfdc-9`
- Deep link generated as: `t.me/bot?start=connect_sfdc-9`

---

## 2. Where "Session Not Found" Is Generated

There are multiple "not found" paths. For deep-link clicks (the `Connect` links in `/ls` output):

**Path: `handle_start()` -> `handle_deep_link_connect()` (`handlers.rs:81`, `167`)**

```
handle_start(payload="connect_sfdc-9")
  -> handle_deep_link_connect(session_name="sfdc-9")
  -> sessions = state.list_tmux_sessions_with_status()   // same tmux call
  -> session_exists = sessions.iter().any(|(name, _, _, _)| {
         name.strip_prefix("commander-").unwrap_or(name) == "sfdc-9"
     })
```

For `sfdc-9`: `"sfdc-9".strip_prefix("commander-") = "sfdc-9"` == `"sfdc-9"` -> TRUE.

This check PASSES. The "not found" at line 191 is NOT triggered for `sfdc-9`.

**Path: `connect()` in state.rs:638**

After the existence check passes, `handle_deep_link_connect` calls `state.connect_session()` which calls `state.connect()`. The `connect()` function has two lookup stages:

**Stage 1 (line 655):** Search registered projects in `~/.ai-commander/projects/*.json` by `project.name == "sfdc-9"`.
- `sfdc-9` is NOT a registered project (confirmed: registered projects are writing, mvs, aic, agents, mpm, hs, apex, dci, commander, izzie, hyperdev, cto).
- Stage 1 FAILS.

**Stage 2 (line 747):** Fallback to direct tmux lookup:
```rust
let session_candidates = [
    format!("commander-{}", base_name),  // "commander-sfdc-9"
    project_name.to_string(),            // "sfdc-9"
    base_name.to_string(),               // "sfdc-9"
];
for session_name in &session_candidates {
    if tmux.session_exists(session_name) { ... }
}
```

`tmux.session_exists("sfdc-9")` -> TRUE. This SUCCEEDS via the fallback path.

So for `sfdc-9`, the `connect()` function should actually succeed.

**The actual "not found" error likely originates from `/connect sfdc-9` typed directly** (not via deep link), which routes through `handle_connect()`. At `handlers.rs:636-638`:
```rust
let sessions = state.list_tmux_sessions();
if sessions.iter().any(|(s, _)| s == &name) {
```
Here `s` is the FULL tmux name (e.g. `"sfdc-9"`), and `name` is the user's input `"sfdc-9"`. This check passes.

**OR: the error is from `/stop sfdc-9`** at `handlers.rs:1725`:
```rust
if !tmux.session_exists(&session_name) {  // session_name = "commander-sfdc-9"
```
The stop handler prepends `commander-` unconditionally at line 1688:
```rust
let tmux_session = format!("commander-{}", session_arg);  // "commander-sfdc-9"
```
`tmux.session_exists("commander-sfdc-9")` -> FALSE (session is `sfdc-9`, not `commander-sfdc-9`).
This is a confirmed bug: **stop fails for non-`commander-` prefixed sessions**.

---

## 3. Data Source Comparison

| Operation | Data Source | What it reads |
|-----------|-------------|---------------|
| `/ls` display | `tmux list-sessions` (live) | ALL tmux sessions |
| Deep-link connect validation | `tmux list-sessions` (live) | ALL tmux sessions |
| `connect()` Stage 1 | `~/.ai-commander/projects/*.json` | Registered projects only |
| `connect()` Stage 2 fallback | `tmux session_exists` (live) | Single tmux session |
| `/stop` existence check | `tmux session_exists` (live) | Single tmux session with `commander-` prefix hardcoded |

**Conclusion:** `/ls` and the connect validation use the SAME live tmux data source. There is no stale-file mismatch for listing. The bugs are in name transformation logic.

---

## 4. State Files: Stale Entries Check

**`~/.ai-commander/state/telegram_sessions.json`:** Contains `{}` (empty). No stale entries.

**`~/.ai-commander/state/sessions/`:** Directory exists but is empty. No stale session files.

**`~/.ai-commander/projects/*.json`:** 12 registered projects, none named `sfdc` or matching the tmux sessions that lack `commander-` prefix.

**Daemon log (`~/.ai-commander/logs/daemon.log`):** Last entry shows normal idle session cleanup (62-minute idle session terminated). No errors. Daemon was cycling (start/stop) on 2026-03-25 at 04:33-04:34, then stable.

---

## 5. tmux Sessions vs State Files

**Live tmux sessions (22 total):**
```
aic-2, aipm-20, apex-1, apex-c-4, commander-aic, commander-izzie,
cto-6, cwr, dci-8, ff-11, go-dre-22, hf-14, hs-18, izzie-3,
kuzu-17, mpm-5, mvs-7, pi-13, repos-0, sdk-19, sfdc-9, w-10
```

**Sessions WITH `commander-` prefix (2):**
- `commander-aic`
- `commander-izzie`

**Sessions WITHOUT `commander-` prefix (20):** These are "unregistered" tmux sessions that `/ls` shows but the stop handler cannot address.

**No stale ghost sessions:** All 22 sessions listed by tmux actually exist. The problem is not ghost/stale entries — it is that 20 of 22 sessions lack the `commander-` prefix that some code paths assume.

---

## 6. Root Cause Analysis

### Bug 1: `/stop` Cannot Stop Non-`commander-` Prefixed Sessions (CONFIRMED)

**File:** `crates/commander-telegram/src/handlers.rs:1684-1688`

```rust
let tmux_session = if session_arg.starts_with("commander-") {
    session_arg.to_string()
} else {
    format!("commander-{}", session_arg)  // BUG: always adds prefix
};
```

Then at line 1725:
```rust
if !tmux.session_exists(&session_name) {
    // Returns "Session 'commander-sfdc-9' not found"
```

`commander-sfdc-9` does not exist; `sfdc-9` does. The stop handler never tries the bare name. This produces the exact "Session 'sfdc-9' not found" error (where the error message shows the user's input but the actual lookup used `commander-sfdc-9`).

### Bug 2: Deep-Link Stop Has Same Problem

**File:** `crates/commander-telegram/src/handlers.rs:297`

```rust
let full_session = format!("commander-{}", session_name);
if !tmux.session_exists(&full_session) {
    // Returns "Session 'sfdc-9' not found"
```

Same pattern: unconditionally prepends `commander-`, fails for bare-named sessions.

### Why Stale Sessions Don't Accumulate

The session persistence system (`telegram_sessions.json`) only stores Telegram _user_ session connections (which user is connected to which tmux session), not a list of available sessions. The available session list always comes from live tmux. Stale accumulation is not possible with this architecture.

The `load_sessions()` function at `state.rs:2016` already correctly validates:
1. Age < 24 hours
2. tmux session still exists (`tmux.session_exists()`)

---

## 7. What Needs to Change

### Fix 1: `/stop` handler must try bare name as fallback

**File:** `crates/commander-telegram/src/handlers.rs` around line 1684

The `tmux_session` variable construction needs to try both forms:
- Try `commander-{session_arg}` first
- Fall back to bare `{session_arg}` if the prefixed form does not exist

Pattern already exists correctly in `connect()` at `state.rs:747-777` — the stop handler should mirror it.

### Fix 2: Deep-link stop must try bare name as fallback

**File:** `crates/commander-telegram/src/handlers.rs` around line 297

Same fix: try `commander-{session_name}`, fall back to bare `{session_name}`.

### Fix 3 (Optional/Cosmetic): Stop error message should show the name that was actually tried

Currently the error message shows the user's input (e.g. `sfdc-9`) but the actual failed lookup was for `commander-sfdc-9`. Making the error message show the attempted tmux name would help debug future issues.

---

## 8. Quick Cleanup vs Code Fix

**No quick cleanup needed.** There are no stale state files. The `telegram_sessions.json` is empty `{}`. The `sessions/` directory is empty.

**A code fix is required.** Two locations in `handlers.rs` need to try the bare tmux session name as a fallback when the `commander-{name}` form does not exist.

---

## 9. Affected Files

- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`
  - Line ~1684-1688: `/stop` session name construction (try bare name fallback)
  - Line ~297: deep-link stop session name construction (try bare name fallback)

- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`
  - No changes needed (connect() fallback is already correct at line 747)

---

## Verification Commands

```bash
# Confirm sfdc-9 exists as bare name (not commander-sfdc-9)
tmux ls | grep sfdc
# sfdc-9: 1 windows ...

# Confirm commander-sfdc-9 does NOT exist
tmux ls | grep "commander-sfdc"
# (no output)

# Confirm sfdc is not a registered project
ls ~/.ai-commander/projects/
# (no sfdc*.json)
```
