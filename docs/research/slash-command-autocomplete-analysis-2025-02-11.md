# AI Commander Slash Command Autocomplete - Architecture Analysis

**Date**: 2025-02-11
**Context**: Analysis for implementing slash command autocomplete in TUI and CLI interfaces
**Status**: ✅ Autocomplete already implemented in TUI | ⚠️ CLI uses rustyline with basic completion

---

## Executive Summary

The AI Commander project has **two distinct interfaces** with different completion implementations:

1. **TUI (Terminal User Interface)** - Built with **ratatui + crossterm**
   - ✅ **Already has slash command autocomplete implemented**
   - Location: `crates/ai-commander/src/tui/completion.rs`
   - Completion is custom-built and works on Tab key press

2. **REPL/CLI** - Built with **rustyline**
   - ✅ **Already has basic completion with CommandCompleter trait**
   - Location: `crates/ai-commander/src/repl.rs` (lines 172-217)
   - Uses rustyline's built-in `Completer` trait

**Key Finding**: Both interfaces already support autocomplete, but the implementations can be enhanced with additional features.

---

## Architecture Overview

### 1. TUI Architecture

**Entry Point**: `crates/ai-commander/src/main.rs` → `run_tui()` → `tui::run()`

**Key Components**:
```
crates/ai-commander/src/
├── tui/
│   ├── mod.rs              # Module entry point
│   ├── app.rs              # App state (lines 176-179: completion fields)
│   ├── events.rs           # Event loop (line 216: Tab key triggers completion)
│   ├── input.rs            # Input handling (submit, history)
│   ├── completion.rs       # ⭐ Tab completion logic
│   ├── ui.rs               # Rendering
│   └── commands.rs         # Command parsing and execution
```

**Terminal Library Stack**:
- **ratatui** (v0.29) - TUI framework
- **crossterm** - Low-level terminal manipulation
- **No rustyline** - Custom input handling

**Completion Flow**:
```
User presses Tab
  ↓
events.rs: KeyCode::Tab detected (line 216)
  ↓
app.complete_command() called
  ↓
completion.rs: Filters COMMANDS array
  ↓
Cycles through matches on repeated Tab
  ↓
Updates app.input with completion
```

---

### 2. REPL/CLI Architecture

**Entry Point**: `crates/ai-commander/src/main.rs` → `run_repl()` → `Repl::run()`

**Key Components**:
```
crates/ai-commander/src/
├── repl.rs                 # ⭐ REPL implementation
│   ├── CommandCompleter   # Lines 172-217: Completion trait
│   ├── Repl struct        # Lines 484-501: REPL state
│   └── ReplCommand enum   # Lines 219-254: Command parsing
```

**Terminal Library Stack**:
- **rustyline** (v14.0) - Readline-style input with built-in completion
- Provides: history, editing, completion, hints

**Completion Flow**:
```
User types "/" and Tab
  ↓
rustyline calls CommandCompleter::complete()
  ↓
Filters COMMANDS array by prefix
  ↓
Returns Vec<Pair> with matching commands
  ↓
rustyline displays completions in list
```

---

## Current Implementation Details

### TUI Completion (completion.rs)

**Available Commands**:
```rust
pub const COMMANDS: &[&str] = &[
    "/clear", "/connect", "/disconnect", "/help", "/inspect",
    "/list", "/quit", "/rename", "/send", "/sessions", "/status",
    "/stop", "/telegram",
];
```

**Completion Behavior**:
1. Only completes if input starts with `/`
2. Builds completions list on first Tab press
3. Cycles through matches on subsequent Tab presses
4. Resets completions when input changes (non-Tab key)
5. Updates `app.input` and `app.cursor_pos` directly

**State Management**:
```rust
// App struct fields (app.rs lines 176-179)
pub(super) completions: Vec<String>,       // Cached matches
pub(super) completion_index: Option<usize>, // Current position
```

**Code Quality**: ✅ Clean, simple, cyclic completion

---

### REPL Completion (repl.rs)

**Available Commands**:
```rust
const COMMANDS: &'static [&'static str] = &[
    "/clear", "/connect", "/disconnect", "/help", "/inspect",
    "/list", "/quit", "/send", "/sessions", "/status", "/stop",
    "/telegram",
];
```

**Completion Behavior**:
1. Only completes if input starts with `/`
2. Filters commands by prefix match
3. Returns all matches (rustyline shows as list)
4. User can select with Tab/arrow keys

**Implementation**:
```rust
impl Completer for CommandCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        if !line.starts_with('/') {
            return Ok((0, vec![]));
        }

        let prefix = &line[..pos];
        let matches: Vec<Pair> = Self::COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .map(|cmd| Pair {
                display: cmd.to_string(),
                replacement: cmd.to_string(),
            })
            .collect();

        Ok((0, matches))
    }
}
```

**rustyline Traits Implemented**:
- ✅ `Completer` - Tab completion
- ✅ `Hinter` - Empty (no inline hints)
- ✅ `Highlighter` - Empty (no syntax highlighting)
- ✅ `Validator` - Empty (no input validation)
- ✅ `Helper` - Marker trait combining all above

**Code Quality**: ✅ Follows rustyline conventions, functional

---

## Slash Commands Available

Both TUI and REPL support the same set of commands:

| Command | Aliases | Description |
|---------|---------|-------------|
| `/list` | `/ls`, `/l` | List all projects |
| `/status` | `/s` | Show project status |
| `/connect` | `/c` | Connect to project (starts if needed) |
| `/disconnect` | `/dc` | Disconnect from current project |
| `/send` | - | Send message to connected project |
| `/sessions` | - | List all tmux sessions |
| `/stop` | - | Stop session (commits changes, ends tmux) |
| `/help` | `/h`, `/?` | Show help |
| `/quit` | `/q`, `/exit` | Exit REPL/TUI |
| `/telegram` | - | Generate pairing code for Telegram bot |
| `/inspect` | - | Toggle live tmux view (TUI only) |
| `/rename` | - | Rename project (TUI only) |
| `/clear` | - | Clear output |

**Note**: TUI has `/inspect` and `/rename` which REPL doesn't have.

---

## Command Parsing Architecture

### REPL Command Parsing (repl.rs)

**Three Types of Input**:
1. **Slash commands**: `/connect myapp`
2. **Routing syntax**: `@project1 @project2 message`
3. **Plain text**: `hello world` (sent to connected project or chat)

**Parsing Flow**:
```rust
ReplCommand::parse(input: &str) -> ReplCommand {
    if input.starts_with('/') {
        // Parse slash command
    } else if input.starts_with('@') {
        // Parse routing syntax
    } else {
        // Parse conversational or treat as text
    }
}
```

**Conversational Commands** (Natural Language):
- `connect to myapp` → `/connect myapp`
- `list projects` → `/list`
- `status of myapp` → `/status myapp`
- `disconnect` → `/disconnect`
- `help` → `/help`
- `quit` → `/quit`

**Command Enum**:
```rust
pub enum ReplCommand {
    List,
    Status(Option<String>),
    Connect(ConnectTarget),  // New or Existing project
    Disconnect,
    Send(String),
    Route { targets: Vec<String>, message: String },
    Sessions,
    Stop(Option<String>),
    Help(Option<String>),
    Telegram,
    Quit,
    Unknown(String),
    Text(String),
}
```

---

### TUI Command Parsing (commands.rs)

**Similar Structure** to REPL but with TUI-specific commands:
- `/inspect` - Toggle live tmux view
- `/rename <project> <new-name>` - Rename project

**Routing** follows same `@alias message` syntax as REPL.

---

## Enhancement Opportunities

### 1. Autocomplete Context-Aware Suggestions

**Current**: Static command list
**Enhancement**: Dynamic suggestions based on state

**Examples**:
- `/connect` → Show project names from `StateStore`
- `/status` → Show project names
- `/stop` → Show running session names
- `/help` → Show all command names

**Implementation Approach**:

#### For TUI (completion.rs):
```rust
impl App {
    pub fn complete_command(&mut self) {
        if !self.input.starts_with('/') {
            return;
        }

        // Build completions based on command
        self.completions = if self.input.starts_with("/connect ") {
            // Load project names from state store
            self.get_project_completions()
        } else if self.input.starts_with("/status ") {
            self.get_project_completions()
        } else if self.input.starts_with("/stop ") {
            self.get_session_completions()
        } else {
            // Default: command name completion
            self.get_command_completions()
        };

        // ... rest of cycling logic
    }

    fn get_project_completions(&self) -> Vec<String> {
        let projects = self.store.load_all_projects().unwrap_or_default();
        let prefix = self.input.split_whitespace().nth(1).unwrap_or("");

        projects
            .values()
            .map(|p| format!("/connect {}", p.name))
            .filter(|cmd| cmd.starts_with(&self.input))
            .collect()
    }
}
```

#### For REPL (repl.rs):
```rust
impl Completer for CommandCompleter {
    fn complete(&self, line: &str, pos: usize, ctx: &Context) -> Result<(usize, Vec<Pair>)> {
        // Need access to Repl state - requires refactoring
        // Option 1: Make CommandCompleter hold Arc<Mutex<Repl>>
        // Option 2: Inject completion function via closure
        // Option 3: Use rustyline's FilenameCompleter pattern
    }
}
```

**Challenge for REPL**: `CommandCompleter` doesn't have access to `Repl` state (projects, sessions).

**Solution**: Refactor to hold state reference:
```rust
struct CommandCompleter {
    state_dir: PathBuf,  // For loading projects
}

impl CommandCompleter {
    fn load_projects(&self) -> Vec<String> {
        StateStore::new(&self.state_dir)
            .load_all_projects()
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default()
    }
}
```

---

### 2. Autocomplete for Arguments and Flags

**Current**: Only completes command names
**Enhancement**: Complete arguments after command name

**Examples**:
- `/connect ~/code/myapp -a [cc|mpm] -n myapp`
  - After `-a ` → suggest `cc`, `mpm`
  - After `-n ` → suggest project name based on directory

**Implementation**:
```rust
pub fn complete_command(&mut self) {
    let parts: Vec<&str> = self.input.split_whitespace().collect();

    match parts.as_slice() {
        ["/connect", .., "-a"] => {
            self.completions = vec!["cc", "mpm"]
                .into_iter()
                .map(|s| format!("{} {}", self.input, s))
                .collect();
        }
        // ... other cases
    }
}
```

---

### 3. Alias Completion for Routing

**Current**: No completion for `@alias` syntax
**Enhancement**: Complete session names after `@`

**Examples**:
- `@my[Tab]` → `@myapp`
- `@frontend @back[Tab]` → `@frontend @backend`

**Implementation**:
```rust
pub fn complete_command(&mut self) {
    if self.input.starts_with('@') {
        // Extract current word after last @
        let words: Vec<&str> = self.input.split_whitespace().collect();
        let last_word = words.last().unwrap_or(&"");

        if last_word.starts_with('@') {
            let prefix = &last_word[1..]; // Remove @
            self.completions = self.get_session_completions()
                .into_iter()
                .filter(|s| s.starts_with(prefix))
                .map(|s| format!("@{}", s))
                .collect();
        }
    }
}
```

---

### 4. Inline Help/Hints

**Current**: No inline hints
**Enhancement**: Show command syntax while typing

**For REPL**: Use rustyline's `Hinter` trait:
```rust
impl Hinter for CommandCompleter {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &Context) -> Option<String> {
        if line.starts_with("/connect ") && pos == line.len() {
            Some(" <path> -a <adapter> -n <name>".into())
        } else if line == "/status" {
            Some(" [project]".into())
        } else {
            None
        }
    }
}
```

**For TUI**: Show hint text below input field (requires UI change):
```rust
// In ui.rs rendering
let hint = app.get_current_hint();
if let Some(hint_text) = hint {
    let hint_widget = Paragraph::new(hint_text)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(hint_widget, hint_area);
}
```

---

### 5. Fuzzy Matching

**Current**: Prefix-only matching
**Enhancement**: Fuzzy search for typo tolerance

**Example**:
- `/cnt` matches `/connect`
- `/hlp` matches `/help`

**Implementation**:
```rust
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};

pub fn complete_command(&mut self) {
    let matcher = SkimMatcherV2::default();

    let mut scored: Vec<_> = COMMANDS
        .iter()
        .filter_map(|cmd| {
            matcher.fuzzy_match(cmd, &self.input)
                .map(|score| (cmd, score))
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by score desc

    self.completions = scored
        .into_iter()
        .map(|(cmd, _)| cmd.to_string())
        .collect();
}
```

**Dependency**: Would need to add `fuzzy-matcher` crate.

---

## Implementation Recommendations

### Priority 1: Context-Aware Completions

**What**: Complete project names for `/connect`, `/status`
**Why**: Most impactful UX improvement
**Difficulty**: Medium (need state access in REPL completer)
**Files**:
- `tui/completion.rs` - Add methods to query state
- `repl.rs` - Refactor `CommandCompleter` to hold state reference

**Estimated Effort**: 2-3 hours

---

### Priority 2: Argument/Flag Completion

**What**: Complete `-a cc|mpm` after `/connect`
**Why**: Reduces need to remember syntax
**Difficulty**: Low (simple string matching)
**Files**:
- `tui/completion.rs` - Extend `complete_command()`
- `repl.rs` - Extend `Completer::complete()`

**Estimated Effort**: 1-2 hours

---

### Priority 3: Alias Routing Completion

**What**: Complete session names after `@`
**Why**: Improves multi-session workflows
**Difficulty**: Medium (parsing @ syntax)
**Files**:
- `tui/completion.rs` - Detect @ prefix
- `repl.rs` - Handle @ in completer

**Estimated Effort**: 2 hours

---

### Priority 4: Inline Hints

**What**: Show command syntax hints
**Why**: Discovery and learning
**Difficulty**:
  - REPL: Low (rustyline trait)
  - TUI: Medium (UI layout changes)
**Files**:
- `repl.rs` - Implement `Hinter` trait
- `tui/ui.rs` - Add hint rendering area

**Estimated Effort**:
- REPL: 1 hour
- TUI: 2-3 hours

---

### Priority 5: Fuzzy Matching

**What**: Match commands by fuzzy search
**Why**: Typo tolerance
**Difficulty**: Low (external crate)
**Dependency**: Add `fuzzy-matcher` crate
**Files**:
- `tui/completion.rs` - Replace filter with fuzzy match
- `repl.rs` - Same

**Estimated Effort**: 1 hour (+ dependency review)

---

## Technical Debt and Considerations

### Consistency Between TUI and REPL

**Issue**: TUI has `/inspect` and `/rename` commands that REPL doesn't have.

**Options**:
1. Add TUI-only commands to REPL (noop or error message)
2. Keep separate command lists
3. Extract shared commands to common module

**Recommendation**: Option 3 - Create `commands::COMMON_COMMANDS` array.

---

### rustyline Completion Limitations

**Challenge**: `CommandCompleter` trait doesn't have access to application state.

**Solutions**:

**Option A: State in Completer**
```rust
struct CommandCompleter {
    state_dir: PathBuf,
}

impl CommandCompleter {
    fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self { state_dir: state_dir.into() }
    }

    fn load_projects(&self) -> Vec<String> {
        // Load on-demand
    }
}
```

**Option B: Closure Completer**
```rust
use rustyline::completion::FilenameCompleter;

let project_completer = move |line: &str| -> Vec<String> {
    // Closure captures Repl state
    load_projects_from_store()
};
```

**Option C: Dynamic Helper**
```rust
struct DynamicHelper {
    projects: Arc<RwLock<Vec<String>>>,
}

impl Completer for DynamicHelper {
    fn complete(&self, line: &str, ...) -> Result<...> {
        let projects = self.projects.read().unwrap();
        // Use cached project list
    }
}

// In Repl::run(), periodically update:
helper.projects.write().unwrap().extend(new_projects);
```

**Recommendation**: Option A (simple) for now, Option C (cached) if performance matters.

---

### Performance Considerations

**Issue**: Loading projects from disk on every Tab press could be slow.

**Solution**: Cache project list and invalidate on changes:
```rust
pub struct App {
    // ...
    cached_projects: Vec<String>,
    cache_updated: Instant,
    cache_ttl: Duration,
}

impl App {
    fn get_project_completions(&mut self) -> Vec<String> {
        if self.cache_updated.elapsed() > self.cache_ttl {
            self.cached_projects = self.load_projects();
            self.cache_updated = Instant::now();
        }
        self.cached_projects.clone()
    }
}
```

---

## Testing Strategy

### Unit Tests

**TUI Completion**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_command_prefix() {
        let mut app = App::test_instance();
        app.input = "/con".to_string();

        app.complete_command();

        assert_eq!(app.completions, vec!["/connect"]);
        assert_eq!(app.completion_index, Some(0));
    }

    #[test]
    fn test_complete_command_cycle() {
        let mut app = App::test_instance();
        app.input = "/s".to_string();

        app.complete_command(); // First match
        assert_eq!(app.input, "/send");

        app.complete_command(); // Second match
        assert_eq!(app.input, "/sessions");

        app.complete_command(); // Third match
        assert_eq!(app.input, "/status");
    }
}
```

**REPL Completion**:
```rust
#[test]
fn test_completer_filters_prefix() {
    let completer = CommandCompleter::new("./test-state");
    let history = DefaultHistory::new();
    let ctx = Context::new(&history);

    let (pos, matches) = completer.complete("/con", 4, &ctx).unwrap();

    assert_eq!(pos, 0);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].replacement, "/connect");
}
```

---

### Integration Tests

**End-to-End Completion Flow**:
1. Start TUI
2. Type `/con`
3. Press Tab
4. Verify input changes to `/connect`

**Manual Testing Checklist**:
- [ ] Tab completes command names
- [ ] Tab cycles through multiple matches
- [ ] Typing after completion resets state
- [ ] Non-slash input doesn't trigger completion
- [ ] Empty input doesn't crash
- [ ] `/connect` completes to project names
- [ ] `/status` completes to project names
- [ ] `@` completes to session names

---

## Dependencies and Tools

### Current Dependencies

```toml
# From Cargo.toml workspace
rustyline = "14.0"        # REPL completion
ratatui = "0.29"          # TUI framework
crossterm = { version = "..." }  # Terminal backend
```

### Optional Dependencies for Enhancements

```toml
# For fuzzy matching
fuzzy-matcher = "0.3"
# OR
skim = "0.10"

# For better completion UI in REPL
rustyline-derive = "0.10"  # Macro support
```

---

## Alternative Approaches

### 1. Use Same Completion Engine for Both

**Idea**: Extract completion logic to `commander-core` crate.

**Pros**:
- Single source of truth
- Consistent behavior
- Easier maintenance

**Cons**:
- Abstracts over different terminal libraries
- Might not leverage library-specific features

**Implementation**:
```rust
// In commander-core
pub struct CompletionEngine {
    commands: Vec<String>,
    projects: Vec<String>,
    sessions: Vec<String>,
}

impl CompletionEngine {
    pub fn complete(&self, input: &str) -> Vec<String> {
        // Shared logic
    }
}

// In TUI and REPL
let engine = CompletionEngine::new();
let completions = engine.complete(&input);
```

---

### 2. Fish/Zsh-Style Multi-Column Completions

**Idea**: Show completions in grid layout (like fish shell).

**Pros**:
- More visual completions
- Fits more on screen

**Cons**:
- Complex rendering
- TUI only (rustyline doesn't support)

**Example**:
```
/con
┌─────────────┬─────────────┐
│ /connect    │ /clear      │
│ /continue   │ /config     │
└─────────────┴─────────────┘
```

---

### 3. Integrated Help on Tab

**Idea**: Show command help when Tab is pressed (like git).

**Example**:
```
> /connect
/connect <path> -a <adapter> -n <name>  Start new project
/connect <name>                          Connect to existing project
```

**Pros**: Discovery and learning
**Cons**: Takes more screen space

---

## Code Locations Reference

### TUI Implementation

| File | Lines | Purpose |
|------|-------|---------|
| `tui/completion.rs` | 1-56 | Tab completion logic |
| `tui/app.rs` | 176-179 | Completion state fields |
| `tui/events.rs` | 216 | Tab key handling |
| `tui/input.rs` | 218 | Reset completions on input |
| `tui/commands.rs` | - | Command execution |

### REPL Implementation

| File | Lines | Purpose |
|------|-------|---------|
| `repl.rs` | 172-217 | CommandCompleter trait |
| `repl.rs` | 484-501 | Repl struct |
| `repl.rs` | 219-254 | ReplCommand enum |
| `repl.rs` | 265-481 | Command parsing |

### Shared

| File | Purpose |
|------|---------|
| `commander-persistence` | StateStore for projects |
| `commander-tmux` | TmuxOrchestrator for sessions |
| `commander-adapters` | AdapterRegistry (cc, mpm) |

---

## Next Steps

### Immediate Actions

1. ✅ **Documented Current State**: This analysis
2. ⏭️ **Choose Priority**: Select from Priority 1-5 above
3. ⏭️ **Spike Implementation**: Prototype context-aware completion
4. ⏭️ **Review with Team**: Discuss UX and approach
5. ⏭️ **Full Implementation**: TDD + integration tests
6. ⏭️ **User Testing**: Validate with real workflows

### Long-Term Improvements

- **Priority 1**: Context-aware completions (projects, sessions)
- **Priority 2**: Argument/flag completion (`-a cc|mpm`)
- **Priority 3**: Alias routing completion (`@alias`)
- **Priority 4**: Inline hints (syntax help)
- **Priority 5**: Fuzzy matching (typo tolerance)

---

## Conclusion

**TUI Autocomplete**: ✅ **Already implemented** with basic prefix matching and cycling.
**REPL Autocomplete**: ✅ **Already implemented** with rustyline's built-in completion.

**Key Enhancements Needed**:
1. Context-aware suggestions (project names, session names)
2. Argument/flag completion
3. Alias routing completion
4. Inline syntax hints
5. Fuzzy matching for typo tolerance

**Most Impactful**: Priority 1 (context-aware) - completes actual project/session names, not just command names.

**Recommended First Step**: Implement context-aware project name completion for `/connect` and `/status` in TUI, then replicate for REPL.

---

## References

- **rustyline documentation**: https://docs.rs/rustyline/14.0.0/rustyline/
- **ratatui documentation**: https://docs.rs/ratatui/0.29.0/ratatui/
- **crossterm documentation**: https://docs.rs/crossterm/latest/crossterm/
- **AI Commander codebase**: `/Users/masa/Projects/ai-commander/crates/ai-commander/`

---

**Attachments**:
- `completion.rs` (TUI implementation)
- `repl.rs` (REPL implementation with CommandCompleter)
- `events.rs` (Tab key handling)
- `app.rs` (completion state)
