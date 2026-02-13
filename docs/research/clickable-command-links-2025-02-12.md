# Research: Clickable Command Links in TUI and Telegram

**Date:** 2025-02-12
**Goal:** Implement links like "/list" that automatically execute "/connect {session_id}" when clicked

---

## Executive Summary

This research investigates how to add clickable command links (e.g., session names in `/list` output that auto-execute `/connect {session_id}` when clicked) to both the TUI and Telegram interfaces.

**Key Findings:**
- **TUI (ratatui):** No native clickable links. Options include OSC 8 hyperlinks (terminal-dependent) or mouse event handling (requires significant refactoring).
- **Telegram (teloxide):** Full support for inline keyboard buttons with callback queries - recommended approach.
- **Neither interface currently has clickable elements.**

---

## 1. TUI Implementation Analysis

### 1.1 Current Architecture

**Terminal Library:** `ratatui` (v0.29) with `crossterm` (v0.28) backend

**Key Files:**
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/mod.rs` - TUI module entry point
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/ui.rs` - Rendering logic (uses ratatui widgets)
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/sessions.rs` - Session list management
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/commands.rs` - Command handling (/list, /connect)
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/events.rs` - Event loop (keyboard-only currently)

**Current Rendering:**
- Output rendered using `ratatui::widgets::Paragraph` with `Text` and `Line` types
- Session lists use `ratatui::widgets::List` with `ListItem`
- No mouse event handling currently enabled
- Styling done via `ratatui::style::Style` (colors, bold, etc.)

### 1.2 Current Session List Display (/list command)

**Location:** `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/commands.rs` (lines 94-124)

```rust
// Current /list command output format:
// "Sessions:"
// "  [Claude] commander-myproject (connected) - Waiting for input"
// "  [Shell] other-session - Active"

for session in &sessions {
    let indicator = adapter.indicator();  // [Claude], [Shell], or [?]
    let activity = self.get_session_activity(&session.name, &adapter);
    self.messages.push(Message::system(format!(
        "  {} {}{} - {}",
        indicator, session.name, connected_marker, activity
    )));
}
```

### 1.3 Options for Clickable Links in TUI

#### Option A: OSC 8 Hyperlinks (Terminal Escape Sequences)

**Concept:** Use ANSI escape sequence `\x1b]8;;URI\x07text\x1b]8;;\x07` to create hyperlinks that open in default handler.

**Pros:**
- Standard mechanism supported by many modern terminals (iTerm2, Kitty, Windows Terminal, Alacritty)
- No mouse handling needed - terminals handle clicks natively

**Cons:**
- Not universally supported (older terminals, basic SSH sessions)
- Requires URI scheme handler registration (`commander://connect/session-name`)
- Would need separate process to handle URI scheme callbacks
- ratatui doesn't have built-in support (need custom spans)

**Implementation complexity:** Medium-High
- Define custom URI scheme
- Register system-wide handler
- Parse URI and dispatch commands

#### Option B: Mouse Event Handling with Click Detection

**Concept:** Enable mouse capture in crossterm, detect clicks on specific regions, and execute commands.

**Pros:**
- Works in any terminal with mouse support
- Full control over click behavior
- Can have visual hover effects

**Cons:**
- Requires significant refactoring of event loop
- Need to track clickable regions during rendering
- Mouse capture interferes with text selection
- More complex state management

**Implementation complexity:** High
- Add `crossterm::event::EnableMouseCapture` to terminal setup
- Track bounding boxes of clickable items during rendering
- Handle `Event::Mouse` in event loop
- Map click coordinates to items

**Required changes:**
1. Modify `setup_terminal()` in `events.rs` to enable mouse
2. Create clickable region tracking during `draw_output()`
3. Add `Event::Mouse` handling in `run_loop()`
4. Store clickable items with their screen coordinates

#### Option C: Selection-Based Navigation (Current Session View Pattern)

**Concept:** Instead of clickable links, use the existing session picker pattern (F3) with keyboard navigation.

**Pros:**
- Already implemented (`ViewMode::Sessions`)
- Keyboard-driven, works everywhere
- Clean UI with selection highlighting

**Cons:**
- Not "clickable links" as requested
- Requires switching to sessions view

**Implementation:** Already exists at `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/ui.rs` (lines 103-134)

### 1.4 Recommended TUI Approach

**Recommended: Option B (Mouse Events) with Option C fallback**

Rationale:
- Mouse events provide the most intuitive "clickable" experience
- The existing sessions view provides keyboard fallback
- OSC 8 has too many compatibility issues

**Implementation Plan:**

1. **Enable mouse capture** in `events.rs`:
```rust
use crossterm::event::{EnableMouseCapture, DisableMouseCapture, MouseEvent, MouseEventKind};

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;  // Add mouse
    // ...
}

fn restore_terminal(...) {
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    // ...
}
```

2. **Track clickable regions** during rendering:
```rust
// Add to App state:
pub struct ClickableItem {
    pub rect: Rect,
    pub action: ClickAction,
}

pub enum ClickAction {
    Connect(String),      // Session name
    ViewSession(String),  // Open in sessions view
}

// During draw_output(), track message positions
```

3. **Handle mouse events** in event loop:
```rust
if let Event::Mouse(mouse) = event::read()? {
    if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
        for item in &app.clickable_items {
            if item.rect.contains(Position { x: mouse.column, y: mouse.row }) {
                match &item.action {
                    ClickAction::Connect(session) => app.connect(session),
                    // ...
                }
            }
        }
    }
}
```

---

## 2. Telegram Implementation Analysis

### 2.1 Current Architecture

**Bot Library:** `teloxide` (v0.13) with webhooks-axum and macros features

**Key Files:**
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/lib.rs` - Module exports
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs` - Main bot implementation
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs` - Command handlers
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs` - Session state management

**Current Session List Display (/list and /sessions commands):**

Location: `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`

`/list` command (lines 534-607):
```rust
// Current format (HTML text only):
// <b>Tmux Sessions:</b>
// 🤖 <code>commander-myproject</code>
// 📟 <code>other-session</code>
// Use <code>/connect &lt;name&gt;</code> to attach
```

`/sessions` command (lines 695-731):
```rust
// Similar text-only format
```

### 2.2 Telegram Inline Keyboard Support

Telegram natively supports inline keyboards with callback buttons. teloxide provides full support.

**Example from teloxide:**
```rust
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

let keyboard = InlineKeyboardMarkup::new(vec![
    vec![InlineKeyboardButton::callback("Connect to myproject", "connect:myproject")],
    vec![InlineKeyboardButton::callback("Connect to other", "connect:other")],
]);

bot.send_message(chat_id, "Select a session:")
    .reply_markup(keyboard)
    .await?;
```

**Callback handling:**
```rust
// Add to handler setup:
Update::filter_callback_query()
    .endpoint(|bot: Bot, q: CallbackQuery, state: Arc<TelegramState>| async move {
        if let Some(data) = q.data {
            if let Some(session) = data.strip_prefix("connect:") {
                // Handle connection
                handle_connect(bot, q.message.unwrap(), state, session.to_string()).await
            }
        }
        Ok(())
    })
```

### 2.3 Current Limitations

- No inline keyboard buttons currently implemented
- All session lists are plain text with manual /connect instructions
- No callback query handler registered

### 2.4 Recommended Telegram Approach

**Recommended: Add inline keyboard buttons to /list and /sessions commands**

**Implementation Plan:**

1. **Modify `/list` handler** to include inline buttons:
```rust
pub async fn handle_list(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let sessions = state.list_tmux_sessions();

    // Build keyboard with session buttons
    let buttons: Vec<Vec<InlineKeyboardButton>> = sessions.iter()
        .map(|(name, _)| {
            let display = name.strip_prefix("commander-").unwrap_or(name);
            vec![InlineKeyboardButton::callback(
                format!("Connect to {}", display),
                format!("connect:{}", name)
            )]
        })
        .collect();

    let keyboard = InlineKeyboardMarkup::new(buttons);

    bot.send_message(msg.chat.id, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(keyboard)  // Add buttons
        .await?;

    Ok(())
}
```

2. **Add callback query handler** in `bot.rs`:
```rust
// In start_polling():
let handler = dptree::entry()
    // ... existing branches ...
    .branch(
        Update::filter_callback_query()
            .endpoint(handle_callback_query)
    );

// New handler:
async fn handle_callback_query(
    bot: Bot,
    q: CallbackQuery,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    if let Some(data) = &q.data {
        if let Some(session) = data.strip_prefix("connect:") {
            // Acknowledge callback
            bot.answer_callback_query(&q.id).await?;

            // Get chat_id from message
            if let Some(msg) = &q.message {
                // Perform connection
                let chat_id = msg.chat().id;
                match state.connect(chat_id, session).await {
                    Ok((name, tool_id)) => {
                        bot.send_message(chat_id, format!("Connected to {}", name)).await?;
                    }
                    Err(e) => {
                        bot.send_message(chat_id, format!("Error: {}", e)).await?;
                    }
                }
            }
        }
    }
    Ok(())
}
```

---

## 3. Command System Overview

### 3.1 /connect Command Flow

**TUI:**
1. User types `/connect myproject`
2. `handle_command()` in `commands.rs` parses input
3. Calls `App::connect()` in `connection.rs`
4. Fallback chain: registered project -> tmux session

**Telegram:**
1. User sends `/connect myproject`
2. `handle_connect()` in `handlers.rs` parses args
3. Calls `TelegramState::connect()` in `state.rs`
4. Similar fallback logic

### 3.2 Command Interface

Both interfaces use the same underlying mechanisms:
- TmuxOrchestrator for session management
- StateStore for project persistence
- AdapterRegistry for adapter lookup

---

## 4. Current Patterns to Follow

### 4.1 TUI Formatting

```rust
// Session list item styling (from ui.rs):
let style = if index == selected {
    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
} else if session.is_connected {
    Style::default().fg(Color::Green)
} else {
    match session.adapter {
        Adapter::Claude => Style::default().fg(Color::Cyan),
        Adapter::Shell => Style::default(),
        Adapter::Unknown => Style::default().fg(Color::DarkGray),
    }
};
```

### 4.2 Telegram HTML Formatting

```rust
// From handlers.rs:
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// Session formatting:
format!("{} <code>{}</code>\n", marker, name)  // Monospace for session names
```

---

## 5. Implementation Recommendations

### 5.1 TUI Priority: Medium-High

**Suggested approach:**
1. Enable mouse capture in crossterm
2. Track clickable regions during List widget rendering
3. Add mouse event handling for clicks
4. Execute `/connect` on click

**Estimated effort:** 2-3 days
**Files to modify:**
- `crates/ai-commander/src/tui/events.rs` - Mouse setup and event handling
- `crates/ai-commander/src/tui/ui.rs` - Region tracking during render
- `crates/ai-commander/src/tui/app.rs` - Clickable item storage
- `crates/ai-commander/src/tui/commands.rs` - Update /list to track regions

### 5.2 Telegram Priority: High (Easier)

**Suggested approach:**
1. Add inline keyboard buttons to /list and /sessions
2. Register callback query handler
3. Parse callback data and execute connection

**Estimated effort:** 0.5-1 day
**Files to modify:**
- `crates/commander-telegram/src/handlers.rs` - Add keyboards to responses
- `crates/commander-telegram/src/bot.rs` - Register callback handler

---

## 6. Code Snippets for Implementation

### 6.1 Telegram Inline Keyboard (Complete Example)

```rust
// In handlers.rs - modify handle_sessions:
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

pub async fn handle_sessions(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let sessions = state.list_tmux_sessions();

    if sessions.is_empty() {
        bot.send_message(msg.chat.id, "No tmux sessions found.")
            .await?;
        return Ok(());
    }

    // Build text
    let mut text = String::from("<b>Tmux Sessions:</b>\n\n");
    for (name, is_commander) in &sessions {
        let marker = if *is_commander { "🤖" } else { "📟" };
        text.push_str(&format!("{} <code>{}</code>\n", marker, name));
    }
    text.push_str("\nClick a button to connect:");

    // Build inline keyboard
    let buttons: Vec<Vec<InlineKeyboardButton>> = sessions.iter()
        .map(|(name, _)| {
            let display = name.strip_prefix("commander-").unwrap_or(name);
            vec![InlineKeyboardButton::callback(
                format!("➡️ {}", display),
                format!("connect:{}", name)
            )]
        })
        .collect();

    let keyboard = InlineKeyboardMarkup::new(buttons);

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(keyboard)
        .await?;

    Ok(())
}

// In bot.rs - add callback handler:
async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let Some(data) = &q.data else {
        return Ok(());
    };

    // Acknowledge immediately
    bot.answer_callback_query(&q.id).await?;

    // Parse callback data
    if let Some(session) = data.strip_prefix("connect:") {
        if let Some(msg) = &q.message {
            let chat_id = msg.chat().id;

            match state.connect(chat_id, session).await {
                Ok((name, tool_id)) => {
                    let adapter = adapter_display_name(&tool_id);
                    bot.send_message(
                        chat_id,
                        format!("✅ Connected to <b>{}</b>\n\nSend messages to interact with {}.", name, adapter)
                    )
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await?;
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("❌ {}", e)).await?;
                }
            }
        }
    }

    Ok(())
}
```

### 6.2 TUI Mouse Handling (Skeleton)

```rust
// In events.rs:
use crossterm::event::{EnableMouseCapture, DisableMouseCapture, MouseEvent, MouseEventKind, MouseButton};

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    // ...
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    // ...
}

// In run_loop:
if event::poll(tick_rate)? {
    match event::read()? {
        Event::Key(key) => { /* existing handling */ }
        Event::Mouse(mouse) => {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                app.handle_click(mouse.column, mouse.row);
            }
        }
        _ => {}
    }
}

// In app.rs:
impl App {
    pub fn handle_click(&mut self, x: u16, y: u16) {
        for item in &self.clickable_items {
            if item.contains(x, y) {
                match &item.action {
                    ClickAction::Connect(session) => {
                        if let Err(e) = self.connect(session) {
                            self.messages.push(Message::system(format!("Error: {}", e)));
                        }
                    }
                }
                break;
            }
        }
    }
}
```

---

## 7. Summary

| Interface | Current State | Recommended Approach | Effort |
|-----------|---------------|---------------------|--------|
| **TUI** | No clickable elements, keyboard-only | Add mouse capture + click tracking | 2-3 days |
| **Telegram** | Text-only session lists | Inline keyboard buttons | 0.5-1 day |

**Start with Telegram** - it's simpler, provides immediate value, and validates the UX pattern before investing in the more complex TUI implementation.

---

## Files Referenced

**TUI:**
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/mod.rs`
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/ui.rs`
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/sessions.rs`
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/commands.rs`
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/events.rs`
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/app.rs`
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/tui/connection.rs`
- `/Users/masa/Projects/ai-commander/crates/ai-commander/Cargo.toml`

**Telegram:**
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/lib.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/bot.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs`
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/state.rs`

**Command System:**
- `/Users/masa/Projects/ai-commander/crates/ai-commander/src/repl.rs`
