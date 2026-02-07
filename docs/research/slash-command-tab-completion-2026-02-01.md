# Slash Command Tab Completion Research

**Date:** 2026-02-01
**Status:** Research Complete
**Scope:** Add tab autocomplete for slash commands in Commander CLI

---

## Executive Summary

The Commander CLI uses `rustyline 14.0` for REPL input handling and `ratatui`/`crossterm` for the TUI. Tab completion can be added to the REPL via rustyline's `Completer` trait. The TUI requires a custom implementation using crossterm key events.

**Key Findings:**
1. Slash commands are defined statically in `COMMAND_HELP` array in `repl.rs`
2. The REPL uses `rustyline::DefaultEditor` which supports completion via the `Helper` trait
3. The TUI handles input manually via `crossterm` events - no built-in completion
4. Both interfaces share the same command vocabulary

**Recommended Approach:**
- REPL: Implement `Completer` + `Helper` traits for rustyline
- TUI: Add inline completion with Tab key handling

---

## 1. Current Slash Commands

All slash commands are defined in `/Users/masa/Projects/ai-commander/crates/commander-cli/src/repl.rs`:

### Commands from `COMMAND_HELP` Static Array (lines 45-149):

| Command | Aliases | Description |
|---------|---------|-------------|
| `/list` | `ls`, `l` | List all projects |
| `/status` | `s` | Show project status |
| `/connect` | `c` | Connect to a project (starts if needed) |
| `/disconnect` | `dc` | Disconnect from current project |
| `/send` | - | Send message to connected project |
| `/sessions` | - | List all tmux sessions |
| `/stop` | - | Stop session (commits changes, ends tmux) |
| `/help` | `h`, `?` | Show help |
| `/quit` | `q`, `exit` | Exit the REPL |

### Commands from `ReplCommand` Enum (lines 152-178):

The command parsing is implemented in `ReplCommand::parse()` (lines 191-231):

```rust
match cmd.as_str() {
    "list" | "ls" | "l" => ReplCommand::List,
    "status" | "s" => ReplCommand::Status(arg),
    "connect" | "c" => Self::parse_connect(arg),
    "disconnect" | "dc" => ReplCommand::Disconnect,
    "send" => ...,
    "sessions" => ReplCommand::Sessions,
    "stop" => ReplCommand::Stop(arg),
    "help" | "h" | "?" => ReplCommand::Help(arg),
    "quit" | "q" | "exit" => ReplCommand::Quit,
    _ => ReplCommand::Unknown(cmd),
}
```

### TUI Additional Commands (from `/Users/masa/Projects/ai-commander/crates/commander-cli/src/tui/app.rs`, lines 981-1114):

| Command | Description |
|---------|-------------|
| `/inspect` | Toggle inspect mode (live tmux) |
| `/clear` | Clear output |

---

## 2. Input Library Analysis

### REPL: rustyline 14.0

**Location:** `/Users/masa/Projects/ai-commander/crates/commander-cli/src/repl.rs`

**Current Setup (lines 371-372):**
```rust
pub struct Repl {
    editor: DefaultEditor,  // rustyline::DefaultEditor
    // ...
}
```

**Initialization (lines 372):**
```rust
let mut editor = DefaultEditor::new()?;
```

**Input Loop (lines 423-425):**
```rust
match self.editor.readline(&prompt) {
    Ok(line) => {
        self.editor.add_history_entry(&line)?;
```

### TUI: ratatui + crossterm

**Location:** `/Users/masa/Projects/ai-commander/crates/commander-cli/src/tui/`

**Key Handling (events.rs, lines 131-156):**
```rust
match key.code {
    KeyCode::Enter => app.submit(),
    KeyCode::Char(c) => app.enter_char(c),
    KeyCode::Backspace => app.delete_char(),
    KeyCode::Left => app.move_cursor_left(),
    KeyCode::Right => app.move_cursor_right(),
    KeyCode::Up => app.history_prev(),
    KeyCode::Down => app.history_next(),
    // ...
}
```

**Input State (app.rs, lines 126-128):**
```rust
pub input: String,
pub cursor_pos: usize,
```

**No Tab handling currently exists.**

---

## 3. Rustyline Completion API

### Required Traits

To add completion to rustyline, implement these traits:

```rust
// From rustyline::completion
pub trait Candidate {
    fn display(&self) -> &str;
    fn replacement(&self) -> &str;
}

pub trait Completer {
    type Candidate: Candidate;

    fn complete(
        &self,
        line: &str,        // Full input line
        pos: usize,        // Cursor position
        ctx: &Context<'_>, // Editor context
    ) -> Result<(usize, Vec<Self::Candidate>)>;
    // Returns: (start_position_of_word, completions)
}

// From rustyline
pub trait Helper: Completer + Hinter + Highlighter + Validator {}
```

### Built-in Types

```rust
// Simple candidate with display and replacement text
pub struct Pair {
    pub display: String,
    pub replacement: String,
}

impl Candidate for Pair { /* auto-implemented */ }
impl Candidate for String { /* String implements Candidate */ }
```

### Completion Types

From `rustyline::config::CompletionType`:
- `List` - Show all matches below input
- `Circular` - Cycle through matches on Tab

---

## 4. Implementation Plan

### 4.1 REPL Tab Completion

**File:** `/Users/masa/Projects/ai-commander/crates/commander-cli/src/repl.rs`

#### Step 1: Define Completer Struct

```rust
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

/// Command completer for slash commands.
struct CommandCompleter {
    commands: Vec<&'static str>,
}

impl CommandCompleter {
    fn new() -> Self {
        Self {
            commands: vec![
                // Primary commands
                "list", "status", "connect", "disconnect", "send",
                "sessions", "stop", "help", "quit",
                // Aliases
                "ls", "l", "s", "c", "dc", "h", "?", "q", "exit",
            ],
        }
    }
}
```

#### Step 2: Implement Completer Trait

```rust
impl Completer for CommandCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // Only complete if line starts with /
        if !line.starts_with('/') {
            return Ok((0, vec![]));
        }

        // Get the command part (after /)
        let cmd_start = 1;
        let cmd_part = &line[cmd_start..pos];

        // If there's a space, we're past the command
        if cmd_part.contains(' ') {
            return Ok((pos, vec![]));
        }

        // Find matching commands
        let matches: Vec<Pair> = self
            .commands
            .iter()
            .filter(|cmd| cmd.starts_with(cmd_part))
            .map(|cmd| Pair {
                display: format!("/{}", cmd),
                replacement: cmd.to_string(),
            })
            .collect();

        Ok((cmd_start, matches))
    }
}
```

#### Step 3: Implement Helper Trait

```rust
impl Hinter for CommandCompleter {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<String> {
        if !line.starts_with('/') || pos == 0 {
            return None;
        }

        let cmd_part = &line[1..pos];
        if cmd_part.contains(' ') {
            return None;
        }

        // Find first matching command for inline hint
        self.commands
            .iter()
            .find(|cmd| cmd.starts_with(cmd_part) && *cmd != cmd_part)
            .map(|cmd| cmd[cmd_part.len()..].to_string())
    }
}

impl Highlighter for CommandCompleter {}
impl Validator for CommandCompleter {}
impl Helper for CommandCompleter {}
```

#### Step 4: Update Repl::new()

Change from `DefaultEditor` to `Editor<CommandCompleter>`:

```rust
use rustyline::Editor;

pub struct Repl {
    editor: Editor<CommandCompleter>,
    // ... rest unchanged
}

impl Repl {
    pub fn new(state_dir: &Path) -> RlResult<Self> {
        let config = rustyline::Config::builder()
            .completion_type(rustyline::config::CompletionType::List)
            .build();

        let mut editor = Editor::with_config(config)?;
        editor.set_helper(Some(CommandCompleter::new()));

        // ... rest unchanged
    }
}
```

### 4.2 TUI Tab Completion

**Files:**
- `/Users/masa/Projects/ai-commander/crates/commander-cli/src/tui/app.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-cli/src/tui/events.rs`

#### Step 1: Add Completion State to App

In `app.rs`, add to `App` struct:

```rust
pub struct App {
    // ... existing fields ...

    /// Available completions for current input
    pub completions: Vec<String>,
    /// Current completion index (-1 = none selected)
    pub completion_index: Option<usize>,
}
```

#### Step 2: Add Completion Logic

In `app.rs`:

```rust
impl App {
    /// Get completions for current input.
    fn get_completions(&self) -> Vec<String> {
        let commands = vec![
            "list", "status", "connect", "disconnect", "send",
            "sessions", "stop", "help", "quit", "inspect", "clear",
            "ls", "l", "s", "c", "dc", "h", "?", "q", "exit",
        ];

        if !self.input.starts_with('/') {
            return vec![];
        }

        let cmd_part = &self.input[1..];
        if cmd_part.contains(' ') {
            return vec![];
        }

        commands
            .into_iter()
            .filter(|cmd| cmd.starts_with(cmd_part))
            .map(|cmd| format!("/{}", cmd))
            .collect()
    }

    /// Handle Tab key press for completion.
    pub fn complete(&mut self) {
        let completions = self.get_completions();

        if completions.is_empty() {
            return;
        }

        match self.completion_index {
            None => {
                // First Tab: apply first completion
                self.completion_index = Some(0);
                self.input = completions[0].clone();
                self.cursor_pos = self.input.len();
            }
            Some(idx) => {
                // Subsequent Tabs: cycle through completions
                let next_idx = (idx + 1) % completions.len();
                self.completion_index = Some(next_idx);
                self.input = completions[next_idx].clone();
                self.cursor_pos = self.input.len();
            }
        }
    }

    /// Reset completion state when input changes.
    pub fn reset_completions(&mut self) {
        self.completion_index = None;
    }
}
```

#### Step 3: Handle Tab Key in events.rs

In the `ViewMode::Normal` match block (around line 129):

```rust
match key.code {
    KeyCode::Tab => app.complete(),
    KeyCode::Enter => {
        app.reset_completions();
        app.submit();
    }
    KeyCode::Char(c) => {
        app.reset_completions();
        app.enter_char(c);
    }
    KeyCode::Backspace => {
        app.reset_completions();
        app.delete_char();
    }
    // ... rest unchanged
}
```

---

## 5. Key Files and Insertion Points

### REPL Changes

| File | Line | Change |
|------|------|--------|
| `repl.rs` | After line 14 (imports) | Add completion imports |
| `repl.rs` | After line 149 (after COMMAND_HELP) | Add CommandCompleter struct and trait impls |
| `repl.rs` | Line 356 | Change `DefaultEditor` to `Editor<CommandCompleter>` |
| `repl.rs` | Lines 371-372 | Update editor initialization |

### TUI Changes

| File | Line | Change |
|------|------|--------|
| `app.rs` | Lines 175-178 | Add completion fields to App struct |
| `app.rs` | Line 221 | Initialize completion fields |
| `app.rs` | After line 978 (before handle_command) | Add completion methods |
| `events.rs` | Line 131 | Add Tab key handling before Enter |

---

## 6. Testing Recommendations

### Manual Testing

1. **REPL Completion:**
   - Type `/` and press Tab - should show all commands
   - Type `/co` and press Tab - should complete to `/connect`
   - Type `/l` and press Tab - should show `/list`, `/ls`, `/l`

2. **TUI Completion:**
   - Type `/h` and press Tab - should complete to `/help`
   - Press Tab again - should cycle to `/h` (if only one match, stay)
   - Type a character - should reset completion state

### Unit Tests

Add to `repl.rs` tests:

```rust
#[test]
fn test_command_completer() {
    let completer = CommandCompleter::new();
    let ctx = Context::new(&History::new());

    // Test slash completion
    let (start, completions) = completer.complete("/co", 3, &ctx).unwrap();
    assert_eq!(start, 1);
    assert!(completions.iter().any(|p| p.replacement == "connect"));

    // Test no completion without slash
    let (_, completions) = completer.complete("connect", 7, &ctx).unwrap();
    assert!(completions.is_empty());
}
```

---

## 7. Future Enhancements

1. **Project Name Completion:** After `/connect `, complete project names from store
2. **Path Completion:** After `/connect ~/`, use rustyline's `FilenameCompleter`
3. **Adapter Completion:** After `-a `, complete adapter names (cc, mpm)
4. **Session Completion:** After `/stop `, complete session names
5. **Fuzzy Matching:** Use fuzzy search for better UX (requires `rustyline` feature flag)

---

## References

- [rustyline Completer trait](https://docs.rs/rustyline/latest/rustyline/completion/trait.Completer.html)
- [rustyline Helper trait](https://docs.rs/rustyline/5.0.0/rustyline/trait.Helper.html)
- [rustyline GitHub source](https://github.com/kkawakam/rustyline/blob/master/src/completion.rs)
- [Workspace Cargo.toml](/Users/masa/Projects/ai-commander/Cargo.toml) - rustyline 14.0
- [REPL source](/Users/masa/Projects/ai-commander/crates/commander-cli/src/repl.rs)
- [TUI app source](/Users/masa/Projects/ai-commander/crates/commander-cli/src/tui/app.rs)
- [TUI events source](/Users/masa/Projects/ai-commander/crates/commander-cli/src/tui/events.rs)
