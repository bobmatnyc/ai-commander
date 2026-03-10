# Session Management Investigation for Alias System

**Date:** 2026-02-18
**Research Question:** How are sessions managed in AI Commander and what's required to implement aliasing?
**Status:** Complete - Comprehensive system analysis

---

## Executive Summary

AI Commander uses a **multi-layered session management system** with:
1. **Tmux layer**: Physical session management (`commander-{name}`)
2. **Project layer**: Persistent project metadata (JSON files)
3. **TUI layer**: In-memory session tracking (HashMap)

**Key finding:** Session aliases would require persistence in both the Project model and TUI App state, with special handling for the tmux session name mapping.

---

## 1. Current Session Management Architecture

### 1.1 Session Data Structures

#### Tmux Session (Physical Layer)
```rust
// crates/commander-tmux/src/session.rs
pub struct TmuxSession {
    pub name: String,                    // e.g., "commander-myproject"
    pub created_at: DateTime<Utc>,
    pub panes: Vec<TmuxPane>,
}
```

**Key characteristics:**
- Parsed from `tmux list-sessions` output
- Format: `session_name:created_timestamp`
- Session names CANNOT contain colons (tmux limitation)
- Provides real-time session state (running/stopped)

#### Project (Persistence Layer)
```rust
// crates/commander-models/src/project.rs
pub struct Project {
    pub id: ProjectId,                    // e.g., "proj-abc123"
    pub path: String,                     // e.g., "/Users/masa/my-project"
    pub name: String,                     // e.g., "myproject"
    pub state: ProjectState,              // Idle/Working/Blocked/Paused/Error
    pub config: HashMap<String, Value>,   // Includes "tool" (adapter type)
    pub sessions: HashMap<SessionId, ToolSession>,  // Active tool sessions
    // ... work queue, events, thread messages
}
```

**Storage mechanism:**
```
~/.commander/projects/
├── proj-abc123.json    # Full project state
├── proj-def456.json
└── ...
```

**Key characteristics:**
- Persisted as individual JSON files
- Atomic writes (temp file → rename)
- Loaded by project ID or all at once
- Contains adapter configuration (`config.tool`)

#### App Session Tracking (TUI Layer)
```rust
// crates/ai-commander/src/tui/app.rs
pub struct App {
    pub project: Option<String>,           // Current project name
    pub project_path: Option<String>,      // Current project path
    pub sessions: HashMap<String, String>, // project_name → tmux_session_name
    pub store: StateStore,                 // Access to persistence
    pub tmux: Option<TmuxOrchestrator>,   // Tmux interface
    pub registry: AdapterRegistry,         // Adapter configurations
}
```

**Key characteristics:**
- `sessions` HashMap maps project name to tmux session name
  - Example: `"myproject" → "commander-myproject"`
- In-memory only (rebuilt on startup)
- Not persisted (reconstructed from tmux and project store)

### 1.2 Session Lifecycle

#### Creating a Session
```
/connect <path> -a <adapter> -n <name>
    ↓
1. Parse command → ConnectArgs::New { path, adapter, name }
2. Resolve adapter alias (cc → claude-code, mpm → mpm)
3. Validate project path exists
4. Create Project instance
5. Save to StateStore (~/.commander/projects/proj-{id}.json)
6. Call connect(name) to start tmux session
    ↓
7. Create tmux session: commander-{name}
8. Send launch command to tmux
9. Add to sessions map: name → commander-{name}
10. Set app.project = Some(name)
```

#### Connecting to Existing Session
```
/connect <name>
    ↓
Fallback chain:
1. Load all projects from StateStore
2. Find project by name or ID
3. Check tmux session exists: commander-{name}
    ↓ If exists:
    - Add to sessions map
    - Set app.project = Some(name)
    - Detect adapter from screen content
    ↓ If not exists:
    - Try to start project (get adapter from project.config)
    - Create tmux session
    - Send launch command
    ↓ If project not found:
4. Try tmux session directly (unregistered)
    - Try: commander-{name}, {name}, {base_name}
    - If found: Connect without project path
    - Detect adapter from screen content
```

#### Session State Persistence

**Where session metadata is stored:**

| Metadata | Tmux | Project JSON | App Memory |
|----------|------|--------------|------------|
| Session name | ✅ Physical | ❌ | ✅ HashMap |
| Project name | ❌ | ✅ `name` field | ✅ `Option<String>` |
| Project path | ❌ | ✅ `path` field | ✅ `Option<String>` |
| Adapter type | ❌ | ✅ `config.tool` | ❌ (detected on demand) |
| Session ID | ❌ | ✅ `sessions: HashMap<SessionId, ToolSession>` | ❌ |
| Created timestamp | ✅ | ✅ `created_at` | ❌ |
| Running/stopped | ✅ (query tmux) | ❌ | ❌ (query on demand) |

**Key insight:** Project name and tmux session name are decoupled:
- Project name: User-visible identifier (stored in Project.name)
- Tmux session name: Physical tmux session (`commander-{name}`)
- Mapping: Maintained in `App.sessions` HashMap (in-memory only)

---

## 2. `/connect` Command Implementation

### 2.1 Command Parsing
```rust
// crates/ai-commander/src/tui/connection.rs
pub(super) enum ConnectArgs {
    Existing(String),                      // /connect <name>
    New { path: String, adapter: String, name: String }  // /connect <path> -a <adapter> -n <name>
}

impl App {
    pub(super) fn parse_connect_args(&self, arg: &str) -> Result<ConnectArgs, String> {
        // Single argument: existing project
        if parts.len() == 1 {
            return Ok(ConnectArgs::Existing(parts[0].to_string()));
        }

        // Multiple arguments with -a and -n flags
        if parts.iter().any(|&p| p == "-a" || p == "-n") {
            // Parse flags...
            return Ok(ConnectArgs::New { path, adapter, name });
        }

        Err("use '/connect <name>' or '/connect <path> -a <adapter> -n <name>'")
    }
}
```

### 2.2 Connection Logic

#### connect() - Existing project/session
```rust
pub fn connect(&mut self, name: &str) -> Result<(), String> {
    let base_name = name.strip_prefix("commander-").unwrap_or(name);

    // Load all projects
    let projects = self.store.load_all_projects()?;

    // Try 1: Find registered project
    if let Some(project) = projects.values().find(|p| p.name == base_name || p.id == base_name) {
        validate_project_path(&project.path)?;

        let session_name = format!("commander-{}", project.name);

        // Check if tmux session exists
        if tmux.session_exists(&session_name) {
            // Connect to existing session
            self.sessions.insert(project.name, session_name);
            self.project = Some(project.name);
            self.project_path = Some(project.path);
            return Ok(());
        }

        // Try to start the project
        let tool_id = project.config.get("tool")?.as_str()?;
        let adapter = self.registry.get(tool_id)?;
        let (cmd, args) = adapter.launch_command(&project.path);

        tmux.create_session_in_dir(&session_name, &project.path)?;
        tmux.send_line(&session_name, &cmd)?;

        self.sessions.insert(project.name, session_name);
        self.project = Some(project.name);
        return Ok(());
    }

    // Try 2: Check for tmux session directly (unregistered)
    for session_name in [
        format!("commander-{}", base_name),
        name.to_string(),
        base_name.to_string(),
    ] {
        if tmux.session_exists(&session_name) {
            // Connect without project registration
            self.sessions.insert(display_name, session_name);
            self.project = Some(display_name);
            self.project_path = None;  // No project path for unregistered
            return Ok(());
        }
    }

    Err(format!("No project or session found: {}", name))
}
```

#### connect_new() - Create and connect
```rust
pub fn connect_new(&mut self, path: &str, adapter: &str, name: &str) -> Result<(), String> {
    // Resolve adapter alias
    let tool_id = self.registry.resolve(adapter)?;

    // Validate path
    validate_project_path(path)?;

    // Check project doesn't already exist
    let projects = self.store.load_all_projects()?;
    if projects.values().any(|p| p.name == name) {
        return Err(format!("Project '{}' already exists", name));
    }

    // Create and save project
    let mut project = Project::new(path, name);
    project.config.insert("tool", json!(tool_id));
    self.store.save_project(&project)?;

    // Connect to it
    self.connect(name)
}
```

### 2.3 Adapter Detection

**Adapters are detected dynamically** from screen content:
```rust
// In connect() after connecting:
let adapter = tmux.capture_output(&session_name, None, Some(50))
    .map(|output| commander_core::detect_adapter(&output))
    .unwrap_or(commander_core::Adapter::Unknown);

// commander_core::detect_adapter checks screen content patterns:
// - "Claude Code" → Adapter::Claude
// - "MPM" → Adapter::Mpm
// - Shell prompts → Adapter::Shell
// - etc.
```

**Adapter type is stored** in `project.config.tool` but not actively maintained in App state.

---

## 3. Requirements for Alias System

### 3.1 Conceptual Model

**Desired behavior:**
```
User creates project: /connect ~/code/myapp -a cc -n myapp
                      ↓
Physical tmux session: commander-myapp
Project name: myapp
Alias (optional): None

User creates alias: /alias myapp -> prod
                    ↓
Physical tmux session: commander-myapp (unchanged)
Project name: myapp (unchanged)
Alias: prod → myapp

User connects via alias: /connect prod
                         ↓
Resolves: prod → myapp
Connects to: commander-myapp
Displays as: prod (or myapp with alias indicator)
```

### 3.2 Storage Requirements

#### Where to store alias mapping?

**Option 1: Extend Project model**
```rust
pub struct Project {
    // Existing fields...
    pub name: String,           // Primary name
    pub aliases: Vec<String>,   // ["prod", "staging", "dev"]
}
```

**Pros:**
- Aliases persisted with project
- No separate storage needed
- Natural ownership (aliases belong to projects)

**Cons:**
- Requires bi-directional lookup (alias → project)
- Must scan all projects to resolve alias

**Option 2: Separate alias registry**
```rust
// New file: ~/.commander/aliases.json
{
    "prod": "myapp",
    "staging": "myapp-stage",
    "dev": "myapp-dev"
}
```

**Pros:**
- Fast alias resolution (direct lookup)
- Single source of truth
- Easy to list all aliases

**Cons:**
- Separate persistence layer
- Potential for orphaned aliases (project deleted but alias remains)
- Requires validation against project store

**Recommended: Option 1 (extend Project model)**
- More robust (no orphaned aliases)
- Better data integrity
- Simpler consistency model

### 3.3 Implementation Points

#### 1. Project Model Changes
```rust
// crates/commander-models/src/project.rs
pub struct Project {
    // ... existing fields ...

    /// Aliases for this project (e.g., ["prod", "staging"])
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl Project {
    /// Add an alias to this project
    pub fn add_alias(&mut self, alias: String) -> Result<(), String> {
        if self.aliases.contains(&alias) {
            return Err("Alias already exists".to_string());
        }
        self.aliases.push(alias);
        Ok(())
    }

    /// Remove an alias from this project
    pub fn remove_alias(&mut self, alias: &str) -> bool {
        if let Some(pos) = self.aliases.iter().position(|a| a == alias) {
            self.aliases.remove(pos);
            true
        } else {
            false
        }
    }

    /// Check if this project matches a name or alias
    pub fn matches(&self, name_or_alias: &str) -> bool {
        self.name == name_or_alias || self.aliases.contains(&name_or_alias.to_string())
    }
}
```

#### 2. StateStore Helper Methods
```rust
// crates/commander-persistence/src/state_store.rs
impl StateStore {
    /// Find a project by name or alias
    pub fn find_project_by_name_or_alias(&self, name_or_alias: &str) -> Result<Option<Project>> {
        let projects = self.load_all_projects()?;

        Ok(projects.into_values().find(|p| p.matches(name_or_alias)))
    }

    /// Check if an alias is already in use by any project
    pub fn alias_exists(&self, alias: &str) -> Result<bool> {
        let projects = self.load_all_projects()?;

        Ok(projects.values().any(|p| {
            p.name == alias || p.aliases.contains(&alias.to_string())
        }))
    }
}
```

#### 3. Connection Logic Updates
```rust
// crates/ai-commander/src/tui/connection.rs
impl App {
    pub fn connect(&mut self, name: &str) -> Result<(), String> {
        let base_name = name.strip_prefix("commander-").unwrap_or(name);

        let projects = self.store.load_all_projects()?;

        // CHANGE: Find by name, alias, or ID
        if let Some(project) = projects.values().find(|p| {
            p.name == base_name
            || p.id.as_str() == base_name
            || p.aliases.contains(&base_name.to_string())  // NEW
        }) {
            // Rest of connection logic unchanged
            // Use project.name for tmux session: commander-{project.name}
        }

        // Fallback to tmux session direct connection...
    }
}
```

#### 4. New `/alias` Command
```rust
// crates/ai-commander/src/tui/commands.rs
pub fn handle_alias(&mut self, arg: &str) {
    let parts: Vec<&str> = arg.split_whitespace().collect();

    match parts.as_slice() {
        // List all aliases
        [] => {
            let projects = self.store.load_all_projects().unwrap_or_default();
            let mut aliases = Vec::new();

            for project in projects.values() {
                for alias in &project.aliases {
                    aliases.push((alias.clone(), project.name.clone()));
                }
            }

            if aliases.is_empty() {
                self.messages.push(Message::system("No aliases defined"));
            } else {
                for (alias, project_name) in aliases {
                    self.messages.push(Message::system(
                        format!("  {} → {}", alias, project_name)
                    ));
                }
            }
        }

        // Show aliases for project
        [project_name] => {
            match self.store.load_all_projects() {
                Ok(projects) => {
                    if let Some(project) = projects.values().find(|p| p.matches(project_name)) {
                        if project.aliases.is_empty() {
                            self.messages.push(Message::system(
                                format!("No aliases for '{}'", project.name)
                            ));
                        } else {
                            self.messages.push(Message::system(
                                format!("Aliases for '{}':", project.name)
                            ));
                            for alias in &project.aliases {
                                self.messages.push(Message::system(format!("  {}", alias)));
                            }
                        }
                    } else {
                        self.messages.push(Message::system(
                            format!("Project not found: {}", project_name)
                        ));
                    }
                }
                Err(e) => {
                    self.messages.push(Message::system(format!("Error: {}", e)));
                }
            }
        }

        // Add alias: /alias <project> <alias>
        [project_name, alias] => {
            // Check alias not already in use
            match self.store.alias_exists(alias) {
                Ok(true) => {
                    self.messages.push(Message::system(
                        format!("Alias '{}' already in use", alias)
                    ));
                    return;
                }
                Err(e) => {
                    self.messages.push(Message::system(format!("Error: {}", e)));
                    return;
                }
                Ok(false) => {}
            }

            // Load project
            match self.store.find_project_by_name_or_alias(project_name) {
                Ok(Some(mut project)) => {
                    // Add alias
                    if let Err(e) = project.add_alias(alias.to_string()) {
                        self.messages.push(Message::system(format!("Error: {}", e)));
                        return;
                    }

                    // Save project
                    if let Err(e) = self.store.save_project(&project) {
                        self.messages.push(Message::system(format!("Failed to save: {}", e)));
                        return;
                    }

                    self.messages.push(Message::system(
                        format!("Added alias '{}' for '{}'", alias, project.name)
                    ));
                }
                Ok(None) => {
                    self.messages.push(Message::system(
                        format!("Project not found: {}", project_name)
                    ));
                }
                Err(e) => {
                    self.messages.push(Message::system(format!("Error: {}", e)));
                }
            }
        }

        // Invalid syntax
        _ => {
            self.messages.push(Message::system(
                "Usage: /alias [project] [alias]"
            ));
        }
    }
}

// Add to command dispatch
"/alias" => self.handle_alias(&arg),
```

#### 5. Remove Alias Command
```rust
// /unalias <alias>
pub fn handle_unalias(&mut self, alias: &str) {
    if alias.is_empty() {
        self.messages.push(Message::system("Usage: /unalias <alias>"));
        return;
    }

    match self.store.load_all_projects() {
        Ok(projects) => {
            let mut found = false;

            for mut project in projects.into_values() {
                if project.remove_alias(alias) {
                    found = true;

                    if let Err(e) = self.store.save_project(&project) {
                        self.messages.push(Message::system(
                            format!("Failed to save: {}", e)
                        ));
                        return;
                    }

                    self.messages.push(Message::system(
                        format!("Removed alias '{}' from '{}'", alias, project.name)
                    ));
                    break;
                }
            }

            if !found {
                self.messages.push(Message::system(
                    format!("Alias not found: {}", alias)
                ));
            }
        }
        Err(e) => {
            self.messages.push(Message::system(format!("Error: {}", e)));
        }
    }
}
```

### 3.4 Edge Cases and Constraints

#### Tmux Session Name Collision
**Problem:** Multiple projects with different names but same tmux session?
**Solution:** Tmux session is always based on primary project name:
```
Project: myapp, aliases: [prod, staging]
Tmux session: commander-myapp (always)
```

**Users can connect via any alias, but tmux session name is deterministic.**

#### Alias Collision Detection
**Problem:** User tries to create alias that conflicts with existing project name or alias.
**Solution:** Validate before adding:
```rust
pub fn add_alias_validated(&mut self, alias: String) -> Result<(), String> {
    // Check against project names
    if self.store.load_all_projects()?
        .values()
        .any(|p| p.name == alias) {
        return Err(format!("Alias conflicts with project name: {}", alias));
    }

    // Check against existing aliases
    if self.store.alias_exists(&alias)? {
        return Err(format!("Alias already in use: {}", alias));
    }

    self.add_alias(alias)
}
```

#### Session Detection with Aliases
**Problem:** Is session running?
**Solution:** Always use project.name for tmux session lookup:
```rust
// When checking if session is running:
let session_name = format!("commander-{}", project.name);  // NOT alias
tmux.session_exists(&session_name)
```

#### Display Behavior
**Options:**
1. **Show alias**: Display connected as "prod" (confusing - doesn't match tmux session)
2. **Show both**: Display "myapp (alias: prod)" (verbose but clear)
3. **Show primary**: Display "myapp" (ignores how user connected)

**Recommended:** Option 2 (show both)
```rust
let display = if let Some(alias) = connected_via_alias {
    format!("{} (alias: {})", project.name, alias)
} else {
    project.name.clone()
};
```

#### Adapter Detection
**No change needed** - adapter detection happens from tmux screen content, independent of alias.

---

## 4. Recommended Implementation Approach

### Phase 1: Data Model
1. Add `aliases: Vec<String>` to Project struct
2. Implement `add_alias()`, `remove_alias()`, `matches()` methods
3. Add StateStore helper: `find_project_by_name_or_alias()`

### Phase 2: Connection Logic
1. Update `connect()` to check aliases in project lookup
2. Preserve connection metadata (connected via name or alias)

### Phase 3: Commands
1. Implement `/alias` command (list, show, add)
2. Implement `/unalias <alias>` command
3. Add to command completion

### Phase 4: Display
1. Show alias in connection message: "Connected to 'myapp' (alias: prod)"
2. Add alias info to `/status` output
3. Show aliases in `/sessions` list

### Phase 5: Validation
1. Test alias collision detection
2. Test alias resolution in all connection flows
3. Test alias persistence across restarts
4. Test unregistered session fallback (aliases don't apply)

---

## 5. Alternative Approaches Considered

### Approach A: Alias at tmux layer
**Idea:** Rename tmux session to alias (commander-prod instead of commander-myapp)

**Rejected because:**
- Breaks existing tooling that expects commander-{project.name}
- Requires tmux session renaming (complex, error-prone)
- Loses primary name (can't tell project.name from tmux session)
- Session naming becomes non-deterministic

### Approach B: Separate alias registry file
**Idea:** Store aliases in ~/.commander/aliases.json

**Rejected because:**
- Potential for orphaned aliases
- Requires synchronization with project store
- Additional persistence layer
- More complex consistency model

### Approach C: App-level alias mapping (in-memory only)
**Idea:** Store aliases only in App.sessions HashMap

**Rejected because:**
- Not persistent across restarts
- Lost when app crashes
- No way to list all aliases
- Inconsistent with project lifecycle

---

## 6. Summary

**Session management is three-layered:**
1. **Tmux layer**: Physical sessions (commander-{name})
2. **Project layer**: Persistent metadata (~/.commander/projects/)
3. **TUI layer**: In-memory tracking (HashMap)

**For alias system:**
- Store aliases in Project model (persistence)
- Resolve aliases in connection logic (lookup)
- Display alias context in messages (UX)
- Tmux session name always uses primary project.name (deterministic)

**Implementation is straightforward:**
- Add `aliases` field to Project
- Update connection lookup to check aliases
- Add `/alias` and `/unalias` commands
- Validate collision (alias vs. names and other aliases)

**Key constraint:**
- Tmux session name must be deterministic (based on project.name)
- Aliases are pure routing (name resolution → project lookup)
- No physical tmux session renaming

---

**Files to modify:**
1. `crates/commander-models/src/project.rs` - Add aliases field and methods
2. `crates/commander-persistence/src/state_store.rs` - Add helper methods
3. `crates/ai-commander/src/tui/connection.rs` - Update connect() logic
4. `crates/ai-commander/src/tui/commands.rs` - Add /alias and /unalias
5. `crates/ai-commander/src/tui/completion.rs` - Add command completion

**Testing strategy:**
- Unit tests: Project.add_alias(), Project.remove_alias(), Project.matches()
- Integration tests: alias resolution in connect()
- E2E tests: /alias command, collision detection, persistence
