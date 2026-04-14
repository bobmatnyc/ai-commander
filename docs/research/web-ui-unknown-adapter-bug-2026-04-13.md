# Web UI "unknown adapter: claude-mpm" Bug

**Date**: 2026-04-13
**Type**: Bug Investigation
**Status**: Root cause identified, fix documented

## Summary

The Web UI sends `adapter: "claude-mpm"` when creating a session, but the
`AdapterRegistry` registers the MPM adapter under the key `"mpm"` (not
`"claude-mpm"`). The validation at `web.rs:223` performs a registry lookup
by the raw string and rejects it.

## Root Cause

**Mismatch between UI adapter IDs and registry adapter IDs.**

| Component | Value sent/registered | File:Line |
|-----------|----------------------|-----------|
| Web UI (CreateSessionModal) | `"claude-mpm"` | `CreateSessionModal.svelte:24` |
| AdapterRegistry key (MpmAdapter) | `"mpm"` | `mpm.rs:18` (`id: "mpm"`) |
| Validation check | `state.adapter_registry.get(adapter_id)` | `web.rs:223` |

The `AdapterRegistry::new()` in `registry.rs:46-73` inserts each adapter
using `adapter.info().id` as the HashMap key. `MpmAdapter::new()` sets
`id: "mpm"`, so the registry key is `"mpm"`.

The Web UI `CreateSessionModal.svelte` defines:
```javascript
const ADAPTERS = [
  { id: 'claude-code', label: 'Claude Code' },
  { id: 'claude-mpm', label: 'Claude MPM' },
  ...
];
```

When the user picks "Claude MPM", the UI sends `adapter: "claude-mpm"`.
The handler at `web.rs:222-228` does:
```rust
if let Some(ref adapter_id) = req.adapter {
    if state.adapter_registry.get(adapter_id).is_none() {
        return Err(ApiError::BadRequest(format!(
            "unknown adapter: {}", adapter_id
        )));
    }
}
```

`registry.get("claude-mpm")` returns `None` because the key is `"mpm"`.

## All Adapter ID Mismatches

| Adapter | Registry ID (info().id) | UI ID | Match? |
|---------|------------------------|-------|--------|
| ClaudeCode | `"claude-code"` | `"claude-code"` | YES |
| MPM | `"mpm"` | `"claude-mpm"` | NO |
| Auggie | `"auggie"` | `"auggie"` | YES |
| Codex | `"codex"` | `"codex"` | YES |
| Shell | `"shell"` | `"shell"` | YES |

Only MPM has a mismatch.

## Fix Options

### Option A: Fix the UI (minimal change)

In `CreateSessionModal.svelte:24`, change the adapter ID to match the registry:
```javascript
{ id: 'mpm', label: 'Claude MPM' },
```

### Option B: Fix the backend adapter ID

In `crates/commander-adapters/src/mpm.rs:18`, change:
```rust
id: "claude-mpm".to_string(),
```

This is riskier because `MpmAdapter.info().id` may be used elsewhere
(launch commands, adapter lookups, configuration).

### Option C: Add alias resolution in the API handler

In `web.rs`, resolve `"claude-mpm"` to `"mpm"` before the registry lookup,
similar to how `AdapterType::from_str` already handles aliases like
`"claude-mpm" | "mpm"` in `project.rs:43`.

### Recommendation

**Option A** is the safest single-line fix. However, the naming inconsistency
between `AdapterType` strings (which accept both `"claude-mpm"` and `"mpm"`)
and registry IDs (which only use `"mpm"`) suggests a deeper design issue.
A more robust solution would be Option C -- normalizing adapter strings
through `AdapterType::from_str` before registry lookup, so both forms work.

## Key Files

- `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/CreateSessionModal.svelte` -- UI adapter list (line 24)
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/mpm.rs` -- MpmAdapter ID (line 18)
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/registry.rs` -- AdapterRegistry (lines 46-73)
- `/Users/masa/Projects/ai-commander/crates/commander-api/src/handlers/web.rs` -- Validation (lines 222-228)
- `/Users/masa/Projects/ai-commander/crates/commander-models/src/project.rs` -- AdapterType::from_str (lines 37-52)
