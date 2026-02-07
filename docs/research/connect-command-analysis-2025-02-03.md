# Research: /connect Command Analysis

**Date:** 2025-02-03
**Task:** Understand how `/connect` works and how to support plain tmux sessions

## Summary

The `/connect` command currently ONLY works with registered projects in the state store. However, the **Session Picker (F3)** already supports connecting to ANY tmux session, including unregistered ones. The fix is straightforward: modify `/connect` to fall back to direct tmux session attachment when no project is found.

## Current Behavior

### `/connect <name>` Flow (lines 277-337 in app.rs)

```
User runs: /connect commander-apr
           ↓
Strips "commander-" prefix → "apr"
           ↓
Loads ALL projects from state store
           ↓
Searches for project where: p.name == "apr" OR p.id == "apr"
           ↓
NOT FOUND → Returns error: "Project not found: apr"
           ↓
NEVER checks if tmux session exists!
```

### Session Picker (F3) Flow (lines 1100-1129)

The Session Picker already handles BOTH cases correctly:

```rust
pub fn connect_selected_session(&mut self) {
    if session.is_commander {
        // Case 1: Commander-managed session
        // Extract project name, look up path from store
        // Connect with project_path
    } else {
        // Case 2: External/unregistered tmux session
        // Use session name directly as "project" name
        // Set project_path = None
        // Works perfectly for sending messages!
    }
}
```

## The Gap

`/connect <name>` ONLY tries the registered project path. It never attempts:
1. Check if `commander-<name>` tmux session exists directly
2. Check if `<name>` is an existing tmux session name
3. Fall back to direct tmux connection without project registration

## TmuxOrchestrator Capabilities

The `TmuxOrchestrator` (in `crates/commander-tmux/src/orchestrator.rs`) provides everything needed:

| Method | Purpose |
|--------|---------|
| `session_exists(name)` | Check if any tmux session exists by name |
| `list_sessions()` | Get all tmux sessions |
| `send_line(session, pane, text)` | Send message to session |
| `capture_output(session, pane, lines)` | Read from session |

**Key insight:** `TmuxOrchestrator` does NOT care if a session is "registered" or not. It just needs the session name.

## Suggested Fix

Modify the `/connect` logic to have a fallback chain:

```
/connect <name>
    ↓
1. Try to find registered project by name/id
   └─ FOUND → Use existing connect() logic (start if needed)
    ↓
2. Try tmux session "commander-<name>"
   └─ EXISTS → Direct connect (like connect_selected_session does for external)
    ↓
3. Try tmux session "<name>" (exact match)
   └─ EXISTS → Direct connect
    ↓
4. Return error: "No project or session found: <name>"
```

### Implementation Sketch

```rust
pub fn connect(&mut self, name: &str) -> Result<(), String> {
    // Strip commander- prefix if present
    let name = name.strip_prefix("commander-").unwrap_or(name);

    // Step 1: Try registered project (existing logic)
    let projects = self.store.load_all_projects()
        .map_err(|e| format!("Failed to load projects: {}", e))?;

    if let Some(project) = projects.values()
        .find(|p| p.name == name || p.id.as_str() == name)
    {
        // Existing registered project logic...
        return self.connect_to_registered_project(project);
    }

    // Step 2: Try direct tmux session connection
    if let Some(tmux) = &self.tmux {
        // Try commander-prefixed name first
        let session_name = format!("commander-{}", name);
        if tmux.session_exists(&session_name) {
            return self.connect_to_tmux_session(name, &session_name);
        }

        // Try exact name match
        if tmux.session_exists(name) {
            return self.connect_to_tmux_session(name, name);
        }
    }

    Err(format!("No project or session found: {}", name))
}

fn connect_to_tmux_session(&mut self, display_name: &str, session_name: &str) -> Result<(), String> {
    self.sessions.insert(display_name.to_string(), session_name.to_string());
    self.project = Some(display_name.to_string());
    self.project_path = None;  // No project path for unregistered sessions
    self.messages.push(Message::system(format!("Connected to session '{}'", display_name)));
    Ok(())
}
```

## Behavior After Fix

| Scenario | Before | After |
|----------|--------|-------|
| `/connect myproject` (registered) | Works | Works |
| `/connect commander-apr` (unregistered tmux) | Error: "Project not found: apr" | Works - connects directly |
| `/connect random-session` (any tmux session) | Error | Works - connects directly |
| `/connect nonexistent` | Error | Error (correct) |

## Alternative Approaches Considered

### Option A: Auto-register tmux sessions as projects
- **Pros:** Keeps single code path
- **Cons:** Pollutes project store, unclear semantics, may have side effects

### Option B: Separate command `/attach <session>`
- **Pros:** Clear distinction between project and session
- **Cons:** More commands to remember, `/connect` still confusing

### Option C: Fallback in `/connect` (RECOMMENDED)
- **Pros:** Intuitive "just works" behavior, minimal code change
- **Cons:** None significant

## Files to Modify

1. **`crates/ai-commander/src/tui/app.rs`**
   - Modify `connect()` method (lines 277-337)
   - Add helper method for direct tmux connection

## Testing Scenarios

1. `/connect registered-project` - Should work as before
2. `/connect commander-unregistered` - Should connect to tmux session
3. `/connect some-random-tmux` - Should connect to any tmux session
4. `/connect nonexistent` - Should error clearly
5. Sending messages after connecting to unregistered session - Should work
6. `/inspect` after connecting to unregistered session - Should work

## Related Code

- **Session Picker logic:** `connect_selected_session()` at line 1100
- **@ routing syntax:** `handle_route()` at line 1649 (already supports any session)
- **TmuxOrchestrator:** `crates/commander-tmux/src/orchestrator.rs`

---

**Recommendation:** Implement Option C (fallback chain). The code pattern already exists in `connect_selected_session()` - just need to apply the same logic to `/connect`.
