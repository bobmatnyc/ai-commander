# Plain Shell Adapter Architecture Research

**Date:** 2026-02-01
**Researcher:** Claude Opus 4.5
**Topic:** Building a plain shell adapter for ai-commander

---

## Executive Summary

This research analyzes the ai-commander architecture to understand how to build a "plain shell adapter" that can connect to arbitrary shell sessions (not running specific AI tools like Claude Code or MPM). The current architecture is well-designed for extension, with clear separation between:

1. **RuntimeAdapter trait** - Defines how to detect state from output patterns
2. **TmuxOrchestrator** - Handles session lifecycle and I/O
3. **ToolSession model** - Tracks session state in persistence layer

A plain shell adapter requires minimal changes: implement a new `RuntimeAdapter` and optionally add a new session detection mechanism for non-commander tmux sessions.

---

## Current Architecture Summary

### Component Hierarchy

```
┌─────────────────────────────────────────────────────────────────────┐
│                        commander-cli (TUI/REPL)                      │
│  - App manages UI state, project connections                         │
│  - Uses TmuxOrchestrator for session I/O                            │
│  - Uses AdapterRegistry to get RuntimeAdapter instances              │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    ▼                               ▼
┌──────────────────────────────┐    ┌──────────────────────────────┐
│     commander-adapters       │    │      commander-tmux          │
│  - RuntimeAdapter trait      │    │  - TmuxOrchestrator          │
│  - ClaudeCodeAdapter         │    │  - TmuxSession/TmuxPane      │
│  - MpmAdapter                │    │  - Session CRUD              │
│  - AdapterRegistry           │    │  - capture_output/send_keys  │
│  - Pattern matching          │    │  - send_line                 │
└──────────────────────────────┘    └──────────────────────────────┘
                    │
                    ▼
┌──────────────────────────────┐
│      commander-models        │
│  - Project (state, config)   │
│  - ToolSession               │
│  - ProjectState enum         │
└──────────────────────────────┘
```

### Key Files

| File | Purpose |
|------|---------|
| `crates/commander-adapters/src/traits.rs` | `RuntimeAdapter` trait definition |
| `crates/commander-adapters/src/registry.rs` | `AdapterRegistry` for adapter lookup |
| `crates/commander-adapters/src/claude_code.rs` | Example adapter implementation |
| `crates/commander-adapters/src/patterns.rs` | Pattern matching utilities |
| `crates/commander-tmux/src/orchestrator.rs` | Tmux session management |
| `crates/commander-cli/src/tui/app.rs` | TUI app with session connection logic |
| `crates/commander-models/src/project.rs` | `ToolSession` and `Project` models |

---

## Trait/Interface Requirements

### RuntimeAdapter Trait (Required Interface)

```rust
// From: crates/commander-adapters/src/traits.rs

pub trait RuntimeAdapter: Send + Sync {
    /// Returns metadata about this adapter
    fn info(&self) -> &AdapterInfo;

    /// Returns command to launch this runtime (for new sessions)
    fn launch_command(&self, project_path: &str) -> (String, Vec<String>);

    /// Analyzes output to determine runtime state
    fn analyze_output(&self, output: &str) -> OutputAnalysis;

    /// Checks if output indicates runtime is idle (default impl provided)
    fn is_idle(&self, output: &str) -> bool;

    /// Checks if output indicates an error (default impl provided)
    fn is_error(&self, output: &str) -> bool;

    /// Formats a message before sending (default: pass-through)
    fn format_message(&self, message: &str) -> String;

    /// Returns regex patterns for idle detection
    fn idle_patterns(&self) -> &[&str];

    /// Returns regex patterns for error detection
    fn error_patterns(&self) -> &[&str];
}
```

### AdapterInfo (Metadata Struct)

```rust
pub struct AdapterInfo {
    pub id: String,              // e.g., "shell", "bash", "plain"
    pub name: String,            // e.g., "Plain Shell"
    pub description: String,     // Human-readable description
    pub command: String,         // e.g., "bash", "zsh", "/bin/sh"
    pub default_args: Vec<String>, // e.g., ["-l"] for login shell
}
```

### OutputAnalysis (Return Type)

```rust
pub struct OutputAnalysis {
    pub state: RuntimeState,      // Starting, Idle, Working, Error, Stopped
    pub confidence: f32,          // 0.0 - 1.0
    pub errors: Vec<String>,      // Extracted error messages
    pub data: HashMap<String, String>, // Additional extracted data
}
```

### RuntimeState Enum

```rust
pub enum RuntimeState {
    Starting,  // Session is initializing
    Idle,      // Ready for input (e.g., at shell prompt)
    Working,   // Command is running
    Error,     // Error detected
    Stopped,   // Session terminated
}
```

---

## How Sessions Are Currently Detected and Connected

### Session Discovery Flow

1. **TUI App** calls `tmux.list_sessions()` to get all tmux sessions
2. Sessions with `commander-` prefix are considered managed
3. External sessions (without prefix) shown but marked as "cannot connect"

```rust
// From tui/app.rs
pub fn refresh_session_list(&mut self) {
    if let Some(tmux) = &self.tmux {
        if let Ok(sessions) = tmux.list_sessions() {
            self.session_list = sessions.iter().map(|s| {
                let is_commander = s.name.starts_with("commander-");
                // ...
            }).collect();
        }
    }
}
```

### Connection Flow

1. **Project-based**: `/connect <name>` looks up project by name
2. **Creates tmux session**: `commander-{name}` naming convention
3. **Launches adapter command**: Uses `adapter.launch_command(project.path)`
4. **Sends launch command to tmux**: Via `tmux.send_line(session, cmd)`

```rust
// From tui/app.rs - connect()
let (cmd, cmd_args) = adapter.launch_command(&project.path);
let full_cmd = format!("{} {}", cmd, cmd_args.join(" "));
tmux.create_session_in_dir(&session_name, Some(&project.path))?;
tmux.send_line(&session_name, None, &full_cmd)?;
```

### Output Polling

1. **Poll loop**: `poll_output()` captures tmux output periodically
2. **Adapter analysis**: `adapter.analyze_output()` determines state
3. **Idle detection**: Pattern matching for shell prompts
4. **Response collection**: Buffers new lines until idle

---

## "Not Running a Framework" Error Path

Currently, there's **no explicit error** for "not running a framework". The system either:

1. **Finds a project** with a configured `tool` in `project.config` and uses that adapter
2. **Defaults to claude-code** if no tool specified
3. **Fails to connect** if tmux session doesn't exist and project not found

The relevant code path:

```rust
// From tui/app.rs
let tool_id = project.config.get("tool")
    .and_then(|v| v.as_str())
    .unwrap_or("claude-code");  // Default fallback

if let Some(adapter) = self.registry.get(tool_id) {
    // Connect with adapter...
}
```

For a plain shell adapter, we'd add a `"shell"` tool option.

---

## Existing Abstractions We Can Extend

### 1. Pattern Module (Most Extensible)

```rust
// crates/commander-adapters/src/patterns.rs

pub mod shell {
    use super::*;

    pub fn idle_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| vec![
            Pattern::new("dollar_prompt", r"^\$\s*$", 0.9),
            Pattern::new("hash_prompt", r"^#\s*$", 0.9),
            Pattern::new("percent_prompt", r"^%\s*$", 0.9),
            Pattern::new("chevron_prompt", r"^>\s*$", 0.8),
            Pattern::new("custom_prompt", r"^[\w\-@:~]+[$#%>]\s*$", 0.85),
            Pattern::new("ps1_pattern", r"\]\$\s*$", 0.8),
        ])
    }

    pub fn error_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| vec![
            Pattern::new("command_not_found", r"command not found", 0.95),
            Pattern::new("permission_denied", r"(?i)permission denied", 0.95),
            Pattern::new("no_such_file", r"No such file or directory", 0.95),
            Pattern::new("exit_code", r"exit code [1-9]", 0.8),
            Pattern::new("error_prefix", r"(?i)^error:", 0.9),
        ])
    }

    pub fn working_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| vec![
            Pattern::new("output_stream", r".+", 0.5), // Any output = working
        ])
    }
}
```

### 2. AdapterRegistry (Plug-and-Play)

```rust
// Easy registration:
impl AdapterRegistry {
    pub fn new() -> Self {
        let mut adapters = HashMap::new();

        // Existing
        adapters.insert("claude-code", Arc::new(ClaudeCodeAdapter::new()));
        adapters.insert("mpm", Arc::new(MpmAdapter::new()));

        // NEW: Plain shell adapter
        adapters.insert("shell", Arc::new(ShellAdapter::new()));

        Self { adapters }
    }
}
```

### 3. ToolSession Model (Already Generic)

```rust
// Already supports arbitrary runtimes:
pub struct ToolSession {
    pub runtime: Option<String>,      // Can be "shell", "bash", etc.
    pub tmux_target: Option<String>,  // Generic tmux target
    pub output_buffer: Vec<String>,   // Works for any output
}
```

---

## Recommended Approach for Plain Shell Adapter

### Option A: Simple Shell Adapter (Recommended)

Create a new adapter for generic shells with configurable prompt patterns.

**Implementation Steps:**

1. **Create `shell.rs` adapter**
```rust
// crates/commander-adapters/src/shell.rs

pub struct ShellAdapter {
    info: AdapterInfo,
    shell_type: ShellType, // Bash, Zsh, Sh, Fish, etc.
}

pub enum ShellType {
    Bash,
    Zsh,
    Sh,
    Fish,
    Custom(String), // Custom prompt pattern
}

impl RuntimeAdapter for ShellAdapter {
    fn info(&self) -> &AdapterInfo { &self.info }

    fn launch_command(&self, project_path: &str) -> (String, Vec<String>) {
        match self.shell_type {
            ShellType::Bash => ("bash", vec!["-l"]),
            ShellType::Zsh => ("zsh", vec!["-l"]),
            ShellType::Sh => ("sh", vec![]),
            ShellType::Fish => ("fish", vec!["-l"]),
            ShellType::Custom(_) => ("bash", vec!["-l"]), // Default
        }
    }

    fn analyze_output(&self, output: &str) -> OutputAnalysis {
        // Use shell patterns module
    }

    fn idle_patterns(&self) -> &[&str] {
        &[r"^\$\s*$", r"^#\s*$", r"^%\s*$", r"^>\s*$"]
    }

    fn error_patterns(&self) -> &[&str] {
        &[r"command not found", r"(?i)permission denied", r"(?i)error:"]
    }
}
```

2. **Add shell patterns module**
```rust
// Add to crates/commander-adapters/src/patterns.rs
pub mod shell { ... }
```

3. **Register in AdapterRegistry**
```rust
let shell = Arc::new(ShellAdapter::new(ShellType::Bash));
adapters.insert(shell.info().id.clone(), shell);
```

4. **Update resolve() for aliases**
```rust
pub fn resolve(&self, alias: &str) -> Option<&'static str> {
    match alias {
        "sh" | "shell" | "bash" => Some("shell"),
        // ... existing
    }
}
```

### Option B: External Session Attachment

Allow connecting to existing non-commander tmux sessions.

**Additional Changes:**

1. **Remove `commander-` prefix requirement** in `connect_selected_session()`
2. **Add session attachment without launching** - skip the `launch_command` step
3. **Auto-detect shell type** from session environment

```rust
// New method in App
pub fn attach_external_session(&mut self, session_name: &str) -> Result<(), String> {
    // 1. Verify session exists
    let tmux = self.tmux.as_ref().ok_or("Tmux not available")?;
    if !tmux.session_exists(session_name) {
        return Err(format!("Session '{}' not found", session_name));
    }

    // 2. Create tracking without launching
    self.sessions.insert(session_name.to_string(), session_name.to_string());
    self.project = Some(session_name.to_string());

    // 3. Use shell adapter for analysis
    // (adapter chosen based on session content analysis or default)

    Ok(())
}
```

---

## Files Requiring Modification

### Minimal Implementation (Option A)

| File | Change |
|------|--------|
| `crates/commander-adapters/src/shell.rs` | **NEW** - Shell adapter implementation |
| `crates/commander-adapters/src/patterns.rs` | Add `pub mod shell` with patterns |
| `crates/commander-adapters/src/lib.rs` | Add `pub mod shell; pub use shell::ShellAdapter;` |
| `crates/commander-adapters/src/registry.rs` | Register ShellAdapter, add "shell" alias |

### Extended Implementation (Option B)

| File | Additional Change |
|------|-------------------|
| `crates/commander-cli/src/tui/app.rs` | Add `attach_external_session()` method |
| `crates/commander-cli/src/repl.rs` | Add `/attach` command |

### Optional Enhancements

| File | Enhancement |
|------|-------------|
| `crates/commander-models/src/project.rs` | Add `SessionType` enum (Tool vs Shell) |
| `crates/commander-adapters/src/traits.rs` | Add `detect_shell_type(output: &str)` helper |

---

## Example Usage After Implementation

### TUI Commands

```bash
# Connect to project with shell adapter
/connect /path/to/project -a shell -n myproject

# Or with alias
/connect /path/to/project -a sh -n myproject

# Attach to existing tmux session (Option B)
/attach my-existing-session
```

### Project Config

```json
{
  "tool": "shell",
  "shell_type": "zsh"
}
```

### Programmatic

```rust
let registry = AdapterRegistry::new();
let shell = registry.get("shell").unwrap();

// Check if shell is idle
let output = tmux.capture_output("session", None, Some(10))?;
if shell.is_idle(&output) {
    tmux.send_line("session", None, "ls -la")?;
}
```

---

## Implementation Recommendations

### Priority Order

1. **First**: Implement `ShellAdapter` with common shell prompt patterns
2. **Second**: Add to registry with "shell", "sh", "bash" aliases
3. **Third**: Test with existing tmux sessions running bash/zsh
4. **Fourth**: (Optional) Add external session attachment

### Testing Strategy

1. **Unit tests**: Pattern matching for various shell prompts
2. **Integration tests**: Create tmux session, verify idle detection
3. **Manual tests**: Connect to real shell sessions via TUI

### Future Enhancements

- Auto-detect shell type from `$SHELL` or prompt analysis
- Custom prompt pattern configuration per-project
- Exit code detection and reporting
- Shell completion suggestions (fish-style)
- Command history tracking

---

## Conclusion

The ai-commander architecture is well-suited for a plain shell adapter:

1. **RuntimeAdapter trait** provides clear interface - just implement pattern matching
2. **AdapterRegistry** offers plug-and-play registration
3. **TmuxOrchestrator** handles all session I/O generically
4. **ToolSession model** already supports arbitrary runtimes

The recommended approach is to start with a simple `ShellAdapter` implementation using common shell prompt patterns, then optionally extend to support attaching to external sessions.

**Estimated effort**: 2-4 hours for minimal implementation, 4-8 hours with external session attachment.
