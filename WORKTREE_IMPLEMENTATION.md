# Git Worktree Support Implementation

This document describes the implementation of basic git worktree support for the Telegram bot.

## Overview

The implementation adds `/connect-tree` (and `/ct` alias) commands to create sessions with git worktrees, and enhances `/stop` (with `/s` alias) to handle worktree cleanup including merging changes back to the main branch.

## Files Modified

### 1. `crates/commander-telegram/src/handlers.rs`

**Added Commands:**
- `ConnectTree(String)` - Main command for worktree sessions
- `Ct(String)` - Alias for ConnectTree
- `S(String)` - Alias for Stop command

**New Handler:**
- `handle_connect_tree()` - Creates a worktree session
  - Checks authorization
  - Validates session name
  - Calls `state.connect_with_worktree()`
  - Returns friendly message with worktree path and branch info

**New Helper Function:**
- `cleanup_worktree()` - Handles worktree cleanup on stop
  - Checks for uncommitted changes in worktree
  - Commits any pending changes
  - Switches parent repo to default branch if needed
  - Merges worktree branch into current branch
  - Removes worktree directory
  - Deletes branch if merge was successful
  - Returns detailed cleanup message

**Modified Handler:**
- `handle_stop()` - Enhanced to detect and handle worktree sessions
  - Checks if session has worktree info
  - If yes: calls `cleanup_worktree()` for merge/cleanup
  - If no: proceeds with regular commit flow
  - Updated response message to include worktree cleanup status

**Command Routing:**
- Added routing for `ConnectTree`, `Ct`, and `S` commands

### 2. `crates/commander-telegram/src/session.rs`

**New Struct:**
```rust
pub struct WorktreeInfo {
    pub worktree_path: String,
    pub branch_name: String,
    pub parent_repo: String,
}
```

**Modified Struct:**
- Added `worktree_info: Option<WorktreeInfo>` field to `UserSession`
- Updated both constructors (`new()` and `with_thread_id()`) to initialize `worktree_info` as None

### 3. `crates/commander-telegram/src/state.rs`

**New Methods:**

- `get_worktree_info()` - Retrieves worktree info for a session
  ```rust
  pub async fn get_worktree_info(&self, chat_id: ChatId) -> Option<WorktreeInfo>
  ```

- `connect_with_worktree()` - Creates a worktree-based session
  ```rust
  pub async fn connect_with_worktree(
      &self,
      chat_id: ChatId,
      session_name: &str,
  ) -> Result<(String, String)>
  ```

  This method:
  - Verifies we're in a git repository
  - Creates `.worktrees/<session_name>/` directory
  - Creates branch `session/<session_name>`
  - Runs `git worktree add .worktrees/<name> -b session/<name>`
  - Creates tmux session in worktree directory
  - Launches adapter (claude-code or mpm) in worktree
  - Stores worktree info in session state

## Usage

### Creating a Worktree Session

```
/connect-tree my-feature
```
or
```
/ct my-feature
```

This creates:
- Worktree at `.worktrees/my-feature/`
- Branch `session/my-feature`
- Tmux session `commander-my-feature` in the worktree directory

### Stopping a Worktree Session

```
/stop
```
or
```
/s
```

This:
1. Commits any uncommitted changes in the worktree
2. Switches parent repo to default branch (main/master)
3. Merges `session/<name>` branch into current branch
4. Removes the worktree directory
5. Deletes the session branch (if merge was successful)

If there are merge conflicts, the user is notified and manual resolution is required.

### Regular Sessions

Regular sessions (created with `/connect`) continue to work as before - the `/stop` command automatically detects whether it's a worktree session or regular session and handles it appropriately.

## Design Decisions

### Simple Merge Strategy
- Uses `--no-ff` (no fast-forward) to always create a merge commit
- This preserves the history of work done in the worktree session

### Worktree Location
- All worktrees are created in `.worktrees/` directory
- This keeps them organized and easy to find
- The directory is created automatically if it doesn't exist

### Branch Naming
- Uses `session/<name>` prefix for worktree branches
- Makes it easy to identify session branches
- Follows common git workflow conventions

### Cleanup on Stop
- Automatic merge attempt on `/stop`
- Fails gracefully if merge conflicts occur
- User gets clear feedback about what happened

### Error Handling
- Validates git repo before creating worktree
- Checks for existing worktrees with same name
- Provides clear error messages for all failure cases

## Testing

All existing tests pass:
```
test result: ok. 40 passed; 0 failed; 0 ignored
```

## Future Enhancements

Possible improvements for the future:
- Support for specifying base branch (instead of always using current branch)
- Interactive conflict resolution via Telegram
- PR creation option instead of direct merge
- Worktree cleanup command (without merge)
- List active worktrees command
- Support for multiple worktrees per user
