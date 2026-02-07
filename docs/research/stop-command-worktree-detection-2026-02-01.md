# Research: /stop Command and Git Worktree Detection

**Date:** 2026-02-01
**Topic:** Adding git worktree detection to /stop command
**Status:** Actionable

## Summary

The `/stop` command is implemented in two places (REPL and TUI) with identical behavior: commit git changes then destroy tmux session. Adding git worktree detection requires a single check before git operations: `git rev-parse --is-inside-work-tree`.

## 1. /stop Command Implementation Locations

### REPL Implementation
**File:** `/Users/masa/Projects/ai-commander/crates/commander-cli/src/repl.rs`

**Command Definition (lines 115-125):**
```rust
CommandHelp {
    name: "stop",
    aliases: &[],
    brief: "Stop session (commits changes, ends tmux)",
    description: "Stops a session by first committing any uncommitted git changes in the project directory, \
                  then destroying the tmux session. If stopping the connected session, also disconnects.",
    usage: "/stop [session]",
    examples: &[
        ("/stop", "Stop current connected session"),
        ("/stop duetto", "Stop the 'duetto' session"),
    ],
}
```

**Enum Variant (line 169):**
```rust
/// Stop a session (commits git changes, destroys tmux)
Stop(Option<String>),
```

**Parse Logic (line 213):**
```rust
"stop" => ReplCommand::Stop(arg),
```

**Handler (lines 662-670):**
```rust
ReplCommand::Stop(target) => {
    let name = target.or_else(|| self.connected_project.clone());

    if let Some(name) = name {
        self.stop_session(&name)?;
    } else {
        println!("Usage: /stop [session] or connect to a session first");
    }
    Ok(false)
}
```

**stop_session Method (lines 868-915):**
```rust
fn stop_session(&mut self, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let session_name = format!("commander-{}", name);

    // Find project path for git operations
    let project_path = {
        let projects = self.store.load_all_projects()?;
        projects.values()
            .find(|p| p.name == name)
            .map(|p| p.path.clone())
    };

    // Step 1: Commit any git changes
    if let Some(path) = &project_path {
        println!("Checking for uncommitted changes in {}...", path);

        match Self::git_commit_changes(path, name) {
            Ok(true) => println!("Changes committed."),
            Ok(false) => println!("No changes to commit."),
            Err(e) => println!("Git warning: {}", e),
        }
    }

    // Step 2: Destroy tmux session
    if let Some(tmux) = &self.tmux {
        match tmux.destroy_session(&session_name) {
            Ok(_) => {
                println!("Session '{}' stopped.", name);
                self.sessions.remove(name);
                if self.connected_project.as_deref() == Some(name) {
                    self.connected_project = None;
                    println!("Disconnected.");
                }
            }
            Err(e) => {
                println!("Failed to stop session: {}", e);
            }
        }
    } else {
        println!("Tmux not available");
    }

    Ok(())
}
```

**git_commit_changes Method (lines 917-954):**
```rust
fn git_commit_changes(path: &str, project_name: &str) -> Result<bool, String> {
    use std::process::Command;

    // Check if there are changes
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to run git status: {}", e))?;

    let changes = String::from_utf8_lossy(&status.stdout);
    if changes.trim().is_empty() {
        return Ok(false); // No changes
    }

    // Stage all changes
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to stage changes: {}", e))?;

    // Commit with message
    let message = format!("WIP: Auto-commit from Commander session '{}'", project_name);
    let commit = Command::new("git")
        .args(["commit", "-m", &message])
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to commit: {}", e))?;

    if commit.status.success() {
        Ok(true)
    } else {
        let stderr = String::from_utf8_lossy(&commit.stderr);
        Err(format!("Commit failed: {}", stderr))
    }
}
```

### TUI Implementation
**File:** `/Users/masa/Projects/ai-commander/crates/commander-cli/src/tui/app.rs`

**stop_session Method (lines 380-428):**
```rust
pub fn stop_session(&mut self, name: &str) {
    let session_name = format!("commander-{}", name);

    // Find project path for git operations
    let project_path = {
        if let Ok(projects) = self.store.load_all_projects() {
            projects.values()
                .find(|p| p.name == name)
                .map(|p| p.path.clone())
        } else {
            None
        }
    };

    // Step 1: Commit any git changes
    if let Some(path) = &project_path {
        self.messages.push(Message::system(format!("Checking for uncommitted changes in {}...", path)));

        match self.git_commit_changes(path, name) {
            Ok(true) => self.messages.push(Message::system("Changes committed.")),
            Ok(false) => self.messages.push(Message::system("No changes to commit.")),
            Err(e) => self.messages.push(Message::system(format!("Git warning: {}", e))),
        }
    }

    // Step 2: Destroy tmux session
    // ... (same pattern as REPL)
}
```

**git_commit_changes Method (lines 430-504):**
Nearly identical to REPL version but with pre-commit hook retry logic.

## 2. Current Stop Behavior Flow

```
1. Parse /stop [session] command
   |
2. Determine session name (arg or connected_project)
   |
3. Look up project path from StateStore
   |
4. If path exists:
   |-- git status --porcelain (check for changes)
   |-- git add -A (stage all)
   |-- git commit -m "WIP: Auto-commit..."
   |   (with pre-commit hook retry in TUI)
   |
5. Destroy tmux session (format: "commander-{name}")
   |
6. Clean up internal state:
   |-- Remove from sessions HashMap
   |-- Clear connected_project if it was current
```

## 3. Session Detection: External vs Commander-Created

Sessions are detected as "commander-created" by their naming convention:

**Pattern:** `commander-{project_name}`

**Detection Logic (repl.rs line 634, tui/app.rs line 715):**
```rust
let is_commander = session.name.starts_with("commander-");
```

**External Session Handling (tui/app.rs lines 763):**
```rust
self.messages.push(Message::system("Cannot connect to external session"));
```

External sessions are listed but cannot be connected to or managed.

## 4. Existing Git Operations in Codebase

All git commands use `std::process::Command`:

| File | Operation | Command |
|------|-----------|---------|
| build.rs | Get commit hash | `git rev-parse --short HEAD` |
| repl.rs | Check changes | `git status --porcelain` |
| repl.rs | Stage changes | `git add -A` |
| repl.rs | Commit | `git commit -m "..."` |
| tui/app.rs | Same as repl.rs | Same as repl.rs |

**Pattern used:**
```rust
let output = Command::new("git")
    .args(["command", "args"])
    .current_dir(path)
    .output()
    .map_err(|e| format!("Error message: {}", e))?;
```

## 5. Git Worktree Detection

**Command:** `git rev-parse --is-inside-work-tree`

**Returns:**
- Exit code 0 + stdout "true\n" = inside git worktree
- Exit code non-zero = not in git repo

**Implementation:**
```rust
fn is_git_worktree(path: &str) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}
```

## 6. Recommended Insertion Points

### Option A: Early Check in git_commit_changes (Recommended)

**Location:** Start of `git_commit_changes` function in both files

**REPL (repl.rs line 918, insert after `use std::process::Command;`):**
```rust
fn git_commit_changes(path: &str, project_name: &str) -> Result<bool, String> {
    use std::process::Command;

    // Check if this is a git worktree
    let is_worktree = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false);

    if !is_worktree {
        return Ok(false); // Not a git repo, nothing to commit
    }

    // ... rest of function unchanged
}
```

**TUI (tui/app.rs line 432, same location):**
Same change.

### Option B: Check in stop_session Before git_commit_changes Call

**Location:** Before the `git_commit_changes` call in both `stop_session` methods

**REPL (repl.rs around line 882):**
```rust
if let Some(path) = &project_path {
    // Check if path is a git worktree
    let is_git = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if is_git {
        println!("Checking for uncommitted changes in {}...", path);
        match Self::git_commit_changes(path, name) {
            // ... existing handling
        }
    } else {
        println!("Skipping git commit (not a git repository)");
    }
}
```

### Recommendation: Option A

**Reasons:**
1. **Single responsibility:** `git_commit_changes` handles all git logic
2. **DRY:** Check happens in one place, not duplicated per caller
3. **Backward compatible:** Callers don't need to change
4. **Graceful:** Returns `Ok(false)` like "no changes to commit"

## 7. Implementation Checklist

- [ ] Add `is_git_worktree` check to `git_commit_changes` in repl.rs (line ~918)
- [ ] Add `is_git_worktree` check to `git_commit_changes` in tui/app.rs (line ~432)
- [ ] Consider extracting shared git utilities to a common module (optional)
- [ ] Add unit tests for non-git directory handling
- [ ] Update help text if behavior change is user-visible (optional)

## 8. Code Duplication Note

The `git_commit_changes` function is duplicated between:
- `crates/commander-cli/src/repl.rs` (lines 917-954)
- `crates/commander-cli/src/tui/app.rs` (lines 430-504)

The TUI version has additional pre-commit hook retry logic (lines 465-503). Consider extracting to a shared utility module in the future.

---

**Research saved to:** `/Users/masa/Projects/ai-commander/docs/research/stop-command-worktree-detection-2026-02-01.md`
