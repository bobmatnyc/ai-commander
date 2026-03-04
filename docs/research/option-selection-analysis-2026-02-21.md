# Option Selection Analysis for Claude Code and Claude MPM

**Date:** 2026-02-21
**Issue:** "We got hung up on those choices" - users cannot select options when Claude presents them
**Status:** Missing functionality - no option selection UI implemented

---

## Executive Summary

**Problem Statement:** Users encounter situations where Claude Code or Claude MPM presents multiple options/choices (e.g., "Which approach would you prefer? A) Option 1 B) Option 2"), but the TUI lacks any mechanism for users to select from these options. This results in users being "stuck" with no way to respond to Claude's questions.

**Root Cause:** The current TUI input system is designed for **free-text input only** - there is no pattern detection, option parsing, or interactive selection UI for handling Claude's multiple-choice questions.

**Impact:** High - blocks user workflow when Claude presents choices, forces users to type out full descriptions instead of selecting options, creates poor UX for decision points.

---

## Current Architecture

### 1. Input System (`crates/ai-commander/src/tui/input.rs`)

**Current capabilities:**
- Character input (`enter_char`)
- Cursor movement (left/right)
- Text deletion (`delete_char`)
- Command history (up/down arrows)
- Free-text submission (`submit`)

**Key observation:** The `submit()` function processes input as:
1. **Commands** (starting with `/`)
2. **Routing syntax** (starting with `@`)
3. **Filesystem commands** (parsed by `filesystem::parse_command`)
4. **Free text** to connected project

**Missing:** No detection or handling of option/choice contexts from Claude's output.

```rust
// Lines 46-107 of input.rs
pub fn submit(&mut self) {
    let input = std::mem::take(&mut self.input);
    // ... history management ...

    if let Some(cmd) = input.strip_prefix('/') {
        self.handle_command(cmd);
    } else if input.starts_with('@') {
        self.handle_route(&input);
    } else if self.project.is_some() {
        // Just sends input directly - no option parsing
        if let Err(e) = self.send_message(&input) {
            self.messages.push(Message::system(format!("Error: {}", e)));
        }
    }
}
```

### 2. Adapter System (`crates/commander-adapters/`)

**Adapter responsibilities** (from `traits.rs`):
- Launch runtime commands
- Analyze output for state detection (Idle, Working, Error)
- Pattern matching for runtime state
- Message formatting

**Key limitation:** Adapters detect **state** (idle/working/error) but do NOT detect **output semantics** like "Claude is asking a question with options."

**Pattern types** (from `patterns.rs`):
- **Idle patterns**: `> `, `PM ready`, `[IDLE]`
- **Error patterns**: `Error:`, `exception`, `failed`
- **Working patterns**: `thinking`, `processing`, `delegating`

**Missing pattern category:** No "question" or "choice" patterns.

### 3. Output Display (`crates/ai-commander/src/tui/ui.rs`)

**Current output rendering:**
- Scrollable message list
- Direction indicators (Sent/Received/System)
- Syntax highlighting for code blocks
- Clickable links (limited to session connections)

**Key observation:** Messages are rendered as plain text or formatted text - no special handling for option lists.

```rust
// From ui.rs draw_output function
fn draw_output(frame: &mut Frame, app: &mut App, area: Rect) {
    // ... scrolling logic ...
    // Renders messages as List items
    // No parsing for "A)", "B)", "1)", "2)" patterns
}
```

### 4. Message Flow

```
User types input → submit() → send_message() → tmux/adapter
                                                    ↓
Claude processes ← receives input ← adapter sends
     ↓
Claude outputs text (potentially with options)
     ↓
Adapter reads output → analyzes state → returns to TUI
     ↓
TUI renders output → user sees options → STUCK (no selection UI)
```

---

## When Claude Presents Options

### Typical Option Formats

Claude Code and Claude MPM may present options in several formats:

1. **Lettered choices:**
   ```
   Which approach would you prefer?
   A) Fix the bug by updating the authentication middleware
   B) Create a new token refresh endpoint
   C) Roll back the recent auth changes
   ```

2. **Numbered choices:**
   ```
   Select an option:
   1) Continue with current approach
   2) Try alternative solution
   3) Ask for more information
   ```

3. **Natural language questions:**
   ```
   Would you like me to:
   - Update the database schema automatically
   - Generate a migration script for manual review
   - Show you the proposed changes first
   ```

4. **Binary choices:**
   ```
   Should I proceed? (yes/no)
   ```

### Detection Challenges

**Problem:** No pattern recognition for option formats in current codebase.

Search results:
- No patterns matching `A)`, `B)`, `1)`, `2)` in `patterns.rs`
- No parsing for list items like `- Option` or `* Choice`
- No detection of question markers like `?` followed by option lists

---

## Gap Analysis

### What's Missing

| Component | Current State | Missing Functionality |
|-----------|--------------|----------------------|
| **Output parsing** | Detects idle/working/error states | Does NOT detect questions or options |
| **Input system** | Free-text entry, commands, routing | No structured option selection (1/2/3, A/B/C) |
| **UI rendering** | Plain text, code blocks, links | No interactive option lists |
| **Adapter layer** | State detection only | No semantic analysis of output content |
| **Message types** | Sent/Received/System | No "Question" or "Choice" message type |

### Specific Implementation Gaps

1. **No option context tracking:**
   - App state doesn't track "Claude is waiting for option selection"
   - No storage for "what options are currently available"

2. **No option parser:**
   - Can't detect patterns like:
     - `A)`, `B)`, `C)` (lettered options)
     - `1)`, `2)`, `3)` (numbered options)
     - `- Option` (markdown list items)
     - `yes/no` questions

3. **No selection UI:**
   - Can't render highlighted/selectable option list
   - Can't use arrow keys to navigate options
   - Can't use number/letter keys to quick-select

4. **No option submission:**
   - Can't translate selection back to Claude's expected format
   - Unclear if Claude expects "A", "Option A", or full text

---

## Example Failure Scenario

**User experience:**

```
> User: "Should we refactor the auth module or create a new one?"

Claude: Let me think about this...

Claude: I see two approaches:
A) Refactor the existing auth module - safer but more time consuming
B) Create a new auth module - faster but requires more testing

Which would you prefer?

> User: [types something in input box]
```

**What happens:**
- User sees options A and B
- User types "A" and presses Enter
- Input system sends literal text "A" to Claude
- Claude may or may not understand that "A" means "Option A"
- No visual feedback that an option was selected
- No validation that "A" is a valid choice
- No way to see option details before selecting

**What SHOULD happen:**
1. TUI detects Claude presented options A and B
2. TUI enters "option selection mode"
3. User sees highlighted option list:
   ```
   > A) Refactor existing auth module [SELECTED]
     B) Create new auth module

   Press Enter to confirm, or type message: _
   ```
4. User can arrow up/down to change selection, or press A/B keys
5. User presses Enter
6. TUI sends "Option A: Refactor existing auth module" back to Claude
7. Visual feedback shows selection was sent

---

## Related Code Patterns

### Existing clickable UI

The codebase has **one example** of interactive elements: clickable session links.

```rust
// From app.rs
pub struct ClickableItem {
    pub rect: Rect,
    pub action: ClickAction,
}

pub enum ClickAction {
    Connect(String), // Only one action type currently
}
```

**Insight:** Infrastructure exists for clickable regions, but only used for session connections. Could be extended for option selection.

### Input modes

```rust
// From app.rs
pub enum InputMode {
    Normal,    // Typing input
    Scrolling, // Scrolling output
}
```

**Insight:** Input mode system exists but doesn't include "SelectingOption" mode.

---

## Recommended Implementation Approach

### Phase 1: Detection (Backend)

**Add pattern recognition for option formats:**

```rust
// New patterns in patterns.rs
pub mod option_patterns {
    use super::*;

    pub fn option_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| {
            vec![
                // Lettered options: A) B) C)
                Pattern::new("lettered", r"(?m)^[A-Z]\)", 0.9),
                // Numbered options: 1) 2) 3)
                Pattern::new("numbered", r"(?m)^\d+\)", 0.9),
                // Markdown list items
                Pattern::new("markdown_list", r"(?m)^[\-\*]\s+", 0.8),
                // Yes/no questions
                Pattern::new("yes_no", r"(?i)\(yes/no\)", 0.95),
            ]
        })
    }
}
```

**Parse detected options:**

```rust
pub struct DetectedOption {
    pub marker: String,      // "A", "1", "-"
    pub text: String,        // Full option text
    pub line_number: usize,  // Where it appeared in output
}

pub fn parse_options(output: &str) -> Option<Vec<DetectedOption>> {
    // Parse option list from Claude's output
    // Return None if no options detected
}
```

### Phase 2: State Management (App)

**Add option context to App state:**

```rust
// In app.rs
pub struct App {
    // ... existing fields ...

    /// Detected options from Claude's last response
    pub pending_options: Option<Vec<DetectedOption>>,
    /// Currently selected option index
    pub selected_option: usize,
    /// Whether in option selection mode
    pub in_option_mode: bool,
}
```

**Add new input mode:**

```rust
pub enum InputMode {
    Normal,
    Scrolling,
    SelectingOption, // NEW: selecting from Claude's options
}
```

### Phase 3: UI Rendering (Frontend)

**Render selectable option list:**

```rust
// In ui.rs
fn draw_option_list(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(options) = &app.pending_options {
        let items: Vec<ListItem> = options.iter().enumerate().map(|(i, opt)| {
            let style = if i == app.selected_option {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let marker = format!("[{}]", if i == app.selected_option { ">" } else { " " });
            let text = format!("{} {} {}", marker, opt.marker, opt.text);

            ListItem::new(text).style(style)
        }).collect();

        let list = List::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(" Select an option (↑↓ arrows, Enter to confirm) "));

        frame.render_widget(list, area);
    }
}
```

### Phase 4: Input Handling

**Handle option navigation:**

```rust
// In input.rs
impl App {
    pub fn select_next_option(&mut self) {
        if let Some(options) = &self.pending_options {
            self.selected_option = (self.selected_option + 1) % options.len();
        }
    }

    pub fn select_prev_option(&mut self) {
        if let Some(options) = &self.pending_options {
            if self.selected_option == 0 {
                self.selected_option = options.len() - 1;
            } else {
                self.selected_option -= 1;
            }
        }
    }

    pub fn confirm_option(&mut self) {
        if let Some(options) = &self.pending_options {
            let selected = &options[self.selected_option];

            // Format response for Claude
            let response = format!("{} {}", selected.marker, selected.text);

            // Send to Claude
            let _ = self.send_message(&response);

            // Clear option mode
            self.pending_options = None;
            self.in_option_mode = false;
            self.input_mode = InputMode::Normal;
        }
    }
}
```

**Update event handling:**

```rust
// In events.rs
match app.input_mode {
    InputMode::SelectingOption => {
        match key.code {
            KeyCode::Up => app.select_prev_option(),
            KeyCode::Down => app.select_next_option(),
            KeyCode::Enter => app.confirm_option(),
            KeyCode::Esc => {
                app.in_option_mode = false;
                app.input_mode = InputMode::Normal;
            }
            // Letter/number quick-select
            KeyCode::Char(c) if c.is_alphanumeric() => {
                app.quick_select_option(c);
            }
            _ => {}
        }
    }
    // ... existing input modes ...
}
```

### Phase 5: Integration with Adapters

**No adapter changes needed** - adapters continue to just pass through output. Detection happens in TUI layer.

**Optional enhancement:** Add semantic analysis to adapters for better confidence scoring:

```rust
// In OutputAnalysis struct
pub struct OutputAnalysis {
    pub state: RuntimeState,
    pub confidence: f32,
    pub errors: Vec<String>,
    pub data: HashMap<String, String>,
    pub detected_options: Option<Vec<DetectedOption>>, // NEW
}
```

---

## Alternative Approaches

### Alternative 1: Inline Quick Response

**Instead of separate selection UI, show inline shortcuts:**

```
Claude: Which approach?
A) Refactor  B) Create new  C) Ask for details

> [A/B/C or type message]: _
```

**Pros:**
- Simpler implementation
- Less UI complexity
- Doesn't change existing input flow

**Cons:**
- Harder to see full option text
- No visual selection feedback
- Still requires pattern detection

### Alternative 2: Command-Based Selection

**Add a `/select` command:**

```
> /select A
> /select 2
> /select "Create new auth module"
```

**Pros:**
- Reuses existing command infrastructure
- No UI changes needed
- Clear and explicit

**Cons:**
- Extra typing required
- Not discoverable
- Doesn't feel natural

### Alternative 3: Auto-Detection with Confirmation

**Detect options, auto-format simple responses:**

```
> A [TAB]  →  expands to "A) Refactor the existing auth module"
```

**Pros:**
- Minimal UI changes
- Uses familiar autocomplete pattern
- Fast for experienced users

**Cons:**
- Requires TAB completion system
- May be too subtle for discoverability

---

## Recommended Approach

**Hybrid solution combining Phase 1-5 + Alternative 3:**

1. **Detection Layer:** Automatically detect when Claude presents options (Phase 1)
2. **Selection UI:** Show interactive option list with arrow key navigation (Phase 2-4)
3. **Quick input:** Allow typing option marker ("A", "1") + Enter as shortcut (Alternative 3)
4. **Fallback:** Allow typing full free-text response if user prefers

**User flow:**

```
Claude outputs options A, B, C
    ↓
TUI detects options automatically
    ↓
Shows highlighted selection UI
    ↓
User can:
  - Arrow keys + Enter (interactive)
  - Type "A" + Enter (quick select)
  - Type custom text + Enter (override)
  - Press Esc (cancel selection mode)
```

---

## Implementation Effort Estimate

| Phase | Effort | Complexity | Dependencies |
|-------|--------|-----------|--------------|
| Pattern detection | 2-3 hours | Low | None |
| State management | 1-2 hours | Low | Pattern detection |
| UI rendering | 3-4 hours | Medium | State management |
| Input handling | 2-3 hours | Medium | UI rendering |
| Testing & polish | 2-3 hours | Low | All above |
| **Total** | **10-15 hours** | **Medium** | - |

**Complexity drivers:**
- Pattern detection: Regex patterns are straightforward
- State management: Simple new fields in App struct
- UI rendering: Moderate - need to handle layout changes
- Input handling: Moderate - integrate with existing event system
- Testing: Needs various option format tests

---

## Testing Strategy

### Unit Tests

1. **Option pattern detection:**
   ```rust
   #[test]
   fn test_detect_lettered_options() {
       let output = "A) Option one\nB) Option two";
       let options = parse_options(output);
       assert_eq!(options.len(), 2);
       assert_eq!(options[0].marker, "A");
   }
   ```

2. **Option parsing edge cases:**
   - Nested options
   - Malformed option lists
   - Mixed formats (letters + numbers)

### Integration Tests

1. **End-to-end flow:**
   - Mock Claude output with options
   - Simulate user selection
   - Verify correct message sent back

2. **UI state transitions:**
   - Normal → SelectingOption → Normal
   - Handle Esc to cancel selection

### Manual Testing Scenarios

1. **Common option formats:**
   - [ ] Lettered options (A/B/C)
   - [ ] Numbered options (1/2/3)
   - [ ] Markdown lists (- / *)
   - [ ] Yes/no questions

2. **Edge cases:**
   - [ ] Very long option text (wrapping)
   - [ ] Many options (>10, requires scrolling)
   - [ ] Options mixed with other output
   - [ ] Multiple option lists in one response

3. **User interactions:**
   - [ ] Arrow key navigation
   - [ ] Quick-select with letter/number
   - [ ] Free-text override
   - [ ] Cancel with Esc

---

## Related Issues and Future Enhancements

### Related Functionality

1. **Command completion** (already exists)
   - Could share UI patterns with option selection
   - Tab completion infrastructure useful for option expansion

2. **Clickable items** (already exists for sessions)
   - Could extend to make options clickable with mouse
   - Same `ClickableItem` / `ClickAction` pattern

### Future Enhancements

1. **Multi-select options:**
   - "Select all that apply: A, B, C"
   - Checkbox-style UI
   - Space to toggle, Enter to confirm

2. **Option details on hover:**
   - Show full option description
   - Preview what each option would do

3. **Option history:**
   - Show previous selections
   - Quick "select same as last time"

4. **Smart defaults:**
   - Pre-select recommended option
   - Highlight based on context

5. **Voice of Claude:**
   - Detect when Claude is uncertain
   - Show confidence levels per option

---

## References

### Files Analyzed

- `crates/ai-commander/src/tui/input.rs` - Input handling
- `crates/ai-commander/src/tui/ui.rs` - UI rendering
- `crates/ai-commander/src/tui/app.rs` - Application state
- `crates/ai-commander/src/tui/events.rs` - Event handling
- `crates/commander-adapters/src/traits.rs` - Adapter interface
- `crates/commander-adapters/src/patterns.rs` - Pattern matching
- `crates/commander-adapters/src/claude_code.rs` - Claude Code adapter
- `crates/commander-adapters/src/mpm.rs` - MPM adapter

### Pattern Examples Found

No existing patterns for option detection found in codebase.

### Related User Workflows

From `.claude-mpm/PM_INSTRUCTIONS.md`:
- "User choice: Always respect if user prefers manual configuration"
- Context suggests Claude MPM asks users for preferences/choices
- Current system requires typing full explanations instead of selecting

---

## Conclusion

**Summary:** The TUI currently has **no option selection functionality**. When Claude presents multiple choices, users must type full free-text responses because there is no detection, parsing, or UI for handling option lists.

**Recommendation:** Implement Phases 1-5 with hybrid approach. Start with pattern detection (Phase 1), then add basic selection UI (Phases 2-3), polish input handling (Phase 4), and iterate based on user feedback.

**Priority:** High - this is a significant UX gap that blocks natural conversational flow when Claude presents decisions.

**Next Steps:**
1. Validate this analysis with team
2. Get user feedback on preferred interaction model
3. Start with Phase 1 (detection) as proof of concept
4. Iterate on UI based on real usage patterns

---

**Research completed:** 2026-02-21
**Researcher:** Claude (AI Commander investigation)
**Document status:** Complete analysis with implementation recommendations
