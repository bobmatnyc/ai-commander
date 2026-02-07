# Project Connection Flow Research

## Summary

This document details how projects are registered, connected, and managed in Commander, identifying where path validation should be added.

## 1. How Projects Are Registered/Created (StateStore)

### StateStore Location
- **File**: `/Users/masa/Projects/ai-commander/crates/commander-persistence/src/state_store.rs`
- **Model**: `/Users/masa/Projects/ai-commander/crates/commander-models/src/project.rs`

### Project Data Structure
```rust
pub struct Project {
    pub id: ProjectId,          // Unique ID (e.g., "proj-abc123")
    pub path: String,           // Project directory path (stored as String, NOT PathBuf)
    pub name: String,           // User-friendly alias
    pub state: ProjectState,    // Idle, Working, Blocked, Paused, Error
    pub config: HashMap<String, serde_json::Value>, // Includes "tool" key for adapter
    // ... other fields
}
```

### Key Observations
1. **Path is stored as String** - The `project.path` field is a plain `String`, not validated or normalized
2. **Project::new()** accepts any string path without validation:
   ```rust
   pub fn new(path: impl Into<String>, name: impl Into<String>) -> Self
   ```
3. **No path existence check** at creation time

### StateStore Operations
```rust
// Save project (no path validation)
store.save_project(&project)?;

// Load all projects (returns HashMap<ProjectId, Project>)
store.load_all_projects()?;
```

---

## 2. How /connect Works

### REPL Connection Flow (repl.rs)

**File**: `/Users/masa/Projects/ai-commander/crates/commander-cli/src/repl.rs`

#### Parsing (Lines 357-418)
```rust
fn parse_connect(arg: Option<String>) -> Self {
    // Detects new vs existing project syntax
    // New: /connect <path> -a <adapter> -n <name>
    // Existing: /connect <project-name>
}
```

#### New Project Connection (Lines 784-846)
```rust
fn handle_connect(&mut self, target: ConnectTarget) {
    match target {
        ConnectTarget::New(args) => {
            // 1. Resolve tool alias (cc -> claude-code)
            let tool_id = self.registry.resolve(&args.tool);

            // 2. Expand tilde in path
            let path = if args.path.starts_with("~") {
                dirs::home_dir().map(|h| h.join(...))
            } else {
                args.path.clone()
            };

            // 3. Check if alias already exists
            let projects = self.store.load_all_projects()?;
            if let Some(existing) = projects.values().find(|p| p.name == args.alias) {
                // Connect to existing
            }

            // 4. Create new project (NO PATH VALIDATION HERE)
            let mut project = Project::new(&path_str, &args.alias);
            project.config.insert("tool", tool_id);

            // 5. Save project
            self.store.save_project(&project)?;

            // 6. Start tmux session
            self.start_project_session(&args.alias, &path_str, &tool_id)?;
        }

        ConnectTarget::Existing(name) => {
            // Look up project by name or ID
            let project = projects.values().find(|p| p.name == name || p.id == name);
            // Start session if not running
        }
    }
}
```

#### Session Start (Lines 894-942)
```rust
fn start_project_session(&mut self, name: &str, path: &str, tool_id: &str) {
    // 1. Check if session already exists
    if tmux.session_exists(&session_name) { return Ok(()); }

    // 2. Get adapter launch command
    let (cmd, args) = adapter.launch_command(path);  // Uses path directly

    // 3. Create tmux session in directory (NO PATH VALIDATION)
    tmux.create_session_in_dir(&session_name, Some(path))?;

    // 4. Send launch command
    tmux.send_line(&session_name, None, &full_cmd)?;
}
```

### TUI Connection Flow (app.rs)

**File**: `/Users/masa/Projects/ai-commander/crates/commander-cli/src/tui/app.rs`

#### Existing Project (Lines 246-300)
```rust
pub fn connect(&mut self, name: &str) -> Result<(), String> {
    // 1. Load all projects
    let projects = self.store.load_all_projects()?;

    // 2. Find by name or ID
    let project = projects.values().find(|p| p.name == name || p.id == name);

    // 3. Check if tmux session exists
    if tmux.session_exists(&session_name) {
        // Just connect
    } else {
        // Start it (NO PATH VALIDATION when starting existing project)
        tmux.create_session_in_dir(&session_name, Some(&project.path))?;
    }
}
```

#### New Project (Lines 355-379)
```rust
pub fn connect_new(&mut self, path: &str, adapter: &str, name: &str) -> Result<(), String> {
    // 1. Resolve adapter alias
    // 2. Check if project already exists

    // 3. Create project (NO PATH VALIDATION)
    let mut project = Project::new(path, name);
    project.config.insert("tool", tool_id);

    // 4. Save project
    self.store.save_project(&project)?;

    // 5. Connect to it
    self.connect(name)
}
```

---

## 3. Where Project Paths Are Stored and Retrieved

### Storage Location
```
~/.commander/projects/
├── proj-abc123.json
└── proj-def456.json
```

### JSON Format
```json
{
  "id": "proj-abc123",
  "path": "/Users/masa/Projects/my-project",  // String, not validated
  "name": "my-project",
  "state": "idle",
  "config": {
    "tool": "claude-code"
  },
  ...
}
```

### Path Usage Points
1. **Project creation**: `Project::new(path, name)` - accepts any string
2. **Tmux session creation**: `tmux.create_session_in_dir(session, Some(path))`
3. **Adapter launch**: `adapter.launch_command(path)` - uses path for working directory
4. **Git operations**: `git_commit_changes(path, name)` - runs git in path directory

---

## 4. Current Error Handling for Invalid Projects

### No Path Validation Currently Exists

The codebase has **NO validation** for:
- Path existence
- Path being a directory
- Path being accessible

### Current Error Points
1. **Tmux session creation fails silently or with generic error**
2. **Git operations return error if path doesn't exist**:
   ```rust
   fn is_git_worktree(path: &str) -> bool {
       Command::new("git").current_dir(path).output()  // Fails silently
   }
   ```

### Error Messages That May Occur
- REPL: Generic tmux errors like "Failed to create tmux session: {e}"
- TUI: Same generic errors
- No specific "path does not exist" message

---

## 5. Where Path Validation Should Be Added

### Recommended Validation Points

#### Primary: Before Project Creation
1. **REPL `handle_connect` (ConnectTarget::New)** - Lines 784-846 in repl.rs
   - After path expansion, before `Project::new()`

2. **TUI `connect_new`** - Lines 355-379 in app.rs
   - Before `Project::new()` call

#### Secondary: Before Session Start
3. **REPL `start_project_session`** - Lines 894-942 in repl.rs
   - Before `tmux.create_session_in_dir()`

4. **TUI `connect`** (existing project) - Lines 246-300 in app.rs
   - Before `tmux.create_session_in_dir()` when starting a stopped project

#### Tertiary: Model Layer
5. **Project::new()** - Lines 199-219 in project.rs
   - Could add validation at construction time (most comprehensive)
   - Would require changing return type to `Result<Self, Error>`

### Validation Logic Needed
```rust
fn validate_project_path(path: &str) -> Result<PathBuf, String> {
    let expanded = shellexpand::tilde(path);
    let path = PathBuf::from(expanded.as_ref());

    // Canonicalize (resolves symlinks, makes absolute)
    let canonical = path.canonicalize()
        .map_err(|_| format!("Path does not exist: {}", path.display()))?;

    // Check it's a directory
    if !canonical.is_dir() {
        return Err(format!("Not a directory: {}", canonical.display()));
    }

    Ok(canonical)
}
```

---

## 6. Recommended Error Messages

| Scenario | Error Message |
|----------|---------------|
| Path doesn't exist | `Project path does not exist: {path}` |
| Path is a file, not directory | `Project path is not a directory: {path}` |
| Permission denied | `Cannot access project path: {path} (permission denied)` |
| Saved project path now invalid | `Project '{name}' path no longer exists: {path}. Update with /connect <new-path> -a {adapter} -n {name}` |

---

## 7. Implementation Priority

1. **High Priority**: Add validation in REPL and TUI before creating new projects
   - Prevents invalid projects from being saved

2. **Medium Priority**: Add validation when connecting to existing projects
   - Handles case where directory was deleted after project registration

3. **Low Priority**: Add validation at model layer (Project::new)
   - Would require broader refactoring

---

## Files to Modify

| File | Purpose |
|------|---------|
| `crates/commander-cli/src/repl.rs` | REPL /connect validation |
| `crates/commander-cli/src/tui/app.rs` | TUI /connect validation |
| `crates/commander-models/src/project.rs` | Optional: model-level validation |

## Related Files (Reference Only)

| File | Purpose |
|------|---------|
| `crates/commander-persistence/src/state_store.rs` | Project persistence |
| `crates/commander-tmux/src/orchestrator.rs` | Tmux session management |
| `crates/commander-adapters/src/lib.rs` | Adapter registry and launch commands |
