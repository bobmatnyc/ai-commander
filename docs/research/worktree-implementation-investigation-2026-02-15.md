# Git Worktree Implementation Investigation

**Date:** 2026-02-15
**Topic:** Current implementation vs original design intent for git worktrees
**Status:** Actionable

## Executive Summary

The original design intent was for Commander sessions to create git worktrees that get merged on `/stop`. **This is NOT implemented.** The current implementation:
- Creates tmux sessions in the main project directory
- Commits changes directly to the current branch on `/stop`
- Has worktree detection code but NO worktree creation code
- Never creates or manages git worktrees

## Current Implementation

### Session Creation Flow

1. **REPL (`repl.rs:1413`)**:
   ```rust
   // Create tmux session in project directory
   tmux.create_session_in_dir(&session_name, Some(path))
   ```
   - Creates tmux session in the **main project directory**
   - No git worktree creation
   - No branch creation
   - Works directly in whatever branch is currently checked out

2. **TUI (`connection.rs:69`)**:
   ```rust
   // Create tmux session in project directory
   tmux.create_session_in_dir(&session_name, Some(&project.path))
   ```
   - Identical behavior to REPL
   - Creates session in main directory
   - No worktree logic

### /stop Command Implementation

1. **REPL (`repl.rs:1436-1483`)**:
   ```rust
   fn stop_session(&mut self, name: &str) -> Result<(), Box<dyn std::error::Error>> {
       // Step 1: Commit any git changes
       if let Some(path) = &project_path {
           match Self::git_commit_changes(path, name) {
               Ok(true) => println!("Changes committed."),
               Ok(false) => println!("No changes to commit."),
               Err(e) => println!("Git warning: {}", e),
           }
       }

       // Step 2: Destroy tmux session
       tmux.destroy_session(&session_name)
   }
   ```
   - Commits changes to **current branch**
   - No merge operation
   - No worktree cleanup
   - Simply destroys tmux session

2. **TUI has identical logic**

### Worktree Detection (But No Creation!)

Both REPL and TUI have worktree detection:

```rust
fn is_git_worktree(path: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success() && o.stdout.trim() == "true")
        .unwrap_or(false)
}
```

This is used to:
- Skip git operations if not in a git repo
- But NEVER to create worktrees

## Missing Worktree Implementation

### What's Missing

1. **No worktree creation code**:
   - No `git worktree add` commands anywhere
   - No branch creation for sessions
   - No worktree directory management

2. **No merge logic**:
   - `/stop` only commits, doesn't merge
   - No branch switching back to main
   - No worktree removal

3. **No worktree tracking**:
   - Sessions don't track associated worktrees
   - No cleanup of abandoned worktrees

### Search Results

```bash
# No worktree creation commands found
grep -r "git worktree add" .  # 0 results
grep -r "worktree add" .       # 0 results

# Worktree mentions only in:
- .claude-mpm/configuration.yaml:16:  - git-worktrees  # A skill
- docs/research/stop-command-worktree-detection-2026-02-01.md  # Detection only
- crates/ai-commander/src/tui/git.rs  # Detection only
- crates/ai-commander/src/repl.rs     # Detection only
```

## Original Design Intent

Based on the description:
1. **Session creation** → Should create a git worktree for that session
2. **Working in session** → Work happens in the worktree (isolated branch)
3. **/stop command** → Should merge the worktree back to main and clean up

## What Would Need to Change

### 1. Session Creation

```rust
fn create_session_with_worktree(name: &str, path: &str) -> Result<String> {
    // Create worktree branch name
    let branch_name = format!("session/{}", name);
    let worktree_path = format!("{}-worktree", path);

    // Create worktree
    Command::new("git")
        .args(["worktree", "add", "-b", &branch_name, &worktree_path])
        .current_dir(path)
        .output()?;

    // Create tmux session in WORKTREE directory
    tmux.create_session_in_dir(&session_name, Some(&worktree_path))?;

    Ok(worktree_path)
}
```

### 2. /stop Command

```rust
fn stop_session_with_merge(name: &str, worktree_path: &str, main_path: &str) {
    // 1. Commit changes in worktree
    git_commit_changes(worktree_path, name)?;

    // 2. Switch to main repo and merge
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(main_path)
        .output()?;

    let branch_name = format!("session/{}", name);
    Command::new("git")
        .args(["merge", &branch_name])
        .current_dir(main_path)
        .output()?;

    // 3. Remove worktree
    Command::new("git")
        .args(["worktree", "remove", worktree_path])
        .current_dir(main_path)
        .output()?;

    // 4. Delete branch
    Command::new("git")
        .args(["branch", "-d", &branch_name])
        .current_dir(main_path)
        .output()?;

    // 5. Destroy tmux session
    tmux.destroy_session(&session_name)?;
}
```

### 3. Project Model Changes

Would need to track:
- Worktree path separately from main project path
- Branch name for the session
- Whether project uses worktree mode

## Configuration Considerations

The skill `git-worktrees` exists in `.claude-mpm/configuration.yaml` but this is just a Claude skill, not actual implementation.

## Recommendations

### Option 1: Implement Worktree Feature (Complex)

**Pros:**
- Isolated development per session
- Clean git history with merge commits
- Multiple sessions can work on different features

**Cons:**
- Significant refactoring needed
- Worktree management complexity
- Directory structure changes
- Backwards compatibility issues

### Option 2: Keep Current Behavior (Simple)

**Pros:**
- Already working
- Simple and predictable
- No migration needed

**Cons:**
- Sessions work directly in main branch
- No isolation between sessions
- Can't have multiple sessions with different changes

### Option 3: Make it Optional (Hybrid)

Add a flag like `--worktree` to session creation:
```bash
/connect myproject --worktree  # Creates worktree
/connect myproject             # Current behavior
```

**Pros:**
- Backwards compatible
- Users can choose
- Gradual adoption

**Cons:**
- Two code paths to maintain
- More complex testing

## Conclusion

The worktree feature described in the design intent is **completely unimplemented**. The codebase has worktree detection but never creates or manages worktrees. Sessions work directly in the main project directory and commit to whatever branch is currently checked out.

Implementing the original design would require substantial changes to:
1. Session creation (create worktrees)
2. Project tracking (store worktree paths)
3. /stop command (merge and cleanup)
4. Error handling (worktree conflicts, merge failures)

---

**Research saved to:** `/Users/masa/Projects/ai-commander/docs/research/worktree-implementation-investigation-2026-02-15.md`