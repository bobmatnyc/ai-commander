//! TUI rendering using ratatui.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::app::{App, ClickAction, InputMode, MessageDirection, SessionInfo, ViewMode};

/// Draw the TUI.
pub fn draw(frame: &mut Frame, app: &mut App) {
    // Clear clickable items before each render cycle
    app.clear_clickable_items();

    match app.view_mode {
        ViewMode::Normal => draw_normal(frame, app),
        ViewMode::Inspect => draw_inspect(frame, app),
        ViewMode::Sessions => draw_sessions(frame, app),
    }
}

/// Draw normal chat mode.
fn draw_normal(frame: &mut Frame, app: &mut App) {
    // Check if we need extra space for command hint
    let hint_height = if app.get_command_hint().is_some() { 1 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                       // Header
            Constraint::Min(5),                          // Output area
            Constraint::Length(1),                       // Status/Progress bar
            Constraint::Length(5 + hint_height),         // Input area + hint (increased for wrapping)
            Constraint::Length(1),                       // Footer
        ])
        .split(frame.area());

    draw_header(frame, app, chunks[0]);
    draw_output(frame, app, chunks[1]);
    draw_status(frame, app, chunks[2]);
    draw_input(frame, app, chunks[3]);
    draw_footer(frame, app, chunks[4]);

    // Store output area rect for click detection
    app.output_area = Some(chunks[1]);
}

/// Draw inspect mode (live tmux view).
fn draw_inspect(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Header
            Constraint::Min(10),    // Tmux content
            Constraint::Length(1),  // Footer
        ])
        .split(frame.area());

    // Header with magenta background to indicate inspect mode
    let project_name = app.project.as_deref().unwrap_or("none");
    let header_text = format!(" Commander - [{}] INSPECT MODE                    F2 to exit ", project_name);
    let header = Paragraph::new(header_text)
        .style(Style::default().bg(Color::Magenta).fg(Color::White).add_modifier(Modifier::BOLD));
    frame.render_widget(header, chunks[0]);

    // Tmux content area
    let session_name = app.project.as_ref()
        .map(|p| format!("commander-{}", p))
        .unwrap_or_else(|| "none".to_string());

    let inner_height = chunks[1].height.saturating_sub(2) as usize;
    let lines: Vec<&str> = app.inspect_content.lines().collect();
    let total_lines = lines.len();

    // Calculate visible range based on scroll (scroll_offset is from bottom)
    let end_idx = total_lines.saturating_sub(app.inspect_scroll);
    let start_idx = end_idx.saturating_sub(inner_height);

    let visible_content: String = lines.get(start_idx..end_idx)
        .map(|slice| slice.join("\n"))
        .unwrap_or_default();

    let title = if app.inspect_scroll > 0 {
        format!(" tmux: {} [scroll: {}] ", session_name, app.inspect_scroll)
    } else {
        format!(" tmux: {} ", session_name)
    };

    let tmux_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(title);

    let content = Paragraph::new(visible_content)
        .block(tmux_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(content, chunks[1]);

    // Footer
    let footer = Paragraph::new(" Live tmux view | Auto-refresh 100ms | Up/Down scroll | F2/Esc/q return to chat ")
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(footer, chunks[2]);
}

/// Draw sessions list view.
fn draw_sessions(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),   // Header
            Constraint::Min(10),     // Session list
            Constraint::Length(1),   // Footer
        ])
        .split(frame.area());

    // Header with cyan background for sessions mode
    let header = Paragraph::new(" Commander - Sessions                                     F3 to exit ")
        .style(Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD));
    frame.render_widget(header, chunks[0]);

    // Session list
    let items: Vec<ListItem> = app.session_list.iter().enumerate().map(|(i, s)| {
        format_session_item(i, s, app.session_selected)
    }).collect();

    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Available Sessions "));
    frame.render_widget(list, chunks[1]);

    // Footer
    let footer = Paragraph::new(" Up/Down select | Enter connect | d delete | F3/Esc back ")
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(footer, chunks[2]);
}

/// Format a session list item.
/// Uses [Claude], [Shell], or [?] based on detected adapter type.
fn format_session_item(index: usize, session: &SessionInfo, selected: usize) -> ListItem<'static> {
    let marker = if index == selected { ">" } else { " " };

    // Type indicator based on detected adapter
    let type_indicator = session.adapter.indicator();

    // Status indicator
    let status = if session.is_connected {
        "connected"
    } else {
        ""
    };

    let style = if index == selected {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if session.is_connected {
        Style::default().fg(Color::Green)
    } else {
        // Color by adapter type
        match session.adapter {
            commander_core::Adapter::Claude => Style::default().fg(Color::Cyan),
            commander_core::Adapter::Shell => Style::default(),
            commander_core::Adapter::Unknown => Style::default().fg(Color::DarkGray),
        }
    };

    let text = if status.is_empty() {
        format!("  {} {} {}", marker, type_indicator, session.name)
    } else {
        format!("  {} {} {:<30} {}", marker, type_indicator, session.name, status)
    };

    ListItem::new(text).style(style)
}

/// Draw the header bar.
fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let header_text = match (&app.project, &app.project_path) {
        (Some(name), Some(path)) => format!(" Commander - [{}] {} ", name, path),
        (Some(name), None) => format!(" Commander - [{}] connected ", name),
        (None, _) => " Commander - disconnected ".to_string(),
    };

    let header = Paragraph::new(header_text)
        .style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD));

    frame.render_widget(header, area);
}

/// Draw the scrollable output area.
fn draw_output(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = if app.scroll_offset > 0 {
        format!(" Output [scroll: {}] ", app.scroll_offset)
    } else {
        " Output ".to_string()
    };

    // Build lines from messages and track session names for clickable regions
    let mut session_line_info: Vec<(usize, String)> = Vec::new(); // (line_index, session_name)

    let lines: Vec<Line> = app
        .messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            let style = match msg.direction {
                MessageDirection::Sent => Style::default().fg(Color::Cyan),
                MessageDirection::Received => Style::default().fg(Color::Green),
                MessageDirection::System => Style::default().fg(Color::Yellow),
            };

            let prefix = match msg.direction {
                MessageDirection::Sent => format!("[{}] > ", msg.project),
                MessageDirection::Received => format!("[{}] ", msg.project),
                MessageDirection::System => String::new(),
            };

            let content = format!("{}{}", prefix, msg.content);

            // Detect session names in /list output (format: "  [Claude|Shell|?] session-name ...")
            if msg.direction == MessageDirection::System {
                if let Some(session_name) = extract_clickable_session(&msg.content) {
                    session_line_info.push((idx, session_name));
                }
            }

            Line::from(vec![Span::styled(content, style)])
        })
        .collect();

    // Calculate scroll - estimate wrapped line count
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;

    // Build cumulative line positions for click tracking
    let mut cumulative_lines: Vec<usize> = Vec::with_capacity(lines.len() + 1);
    cumulative_lines.push(0);
    let mut total = 0usize;
    for line in &lines {
        let line_len: usize = line.spans.iter().map(|s| s.content.len()).sum();
        let wrapped_count = if inner_width > 0 {
            ((line_len + inner_width - 1) / inner_width).max(1)
        } else {
            1
        };
        total += wrapped_count;
        cumulative_lines.push(total);
    }
    let total_wrapped_lines = total;

    // Calculate scroll offset to show latest content, adjusted by user scroll
    let scroll_offset = if total_wrapped_lines > inner_height {
        (total_wrapped_lines - inner_height).saturating_sub(app.scroll_offset)
    } else {
        0
    };

    // Register clickable items for visible session lines
    let inner_area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    for (line_idx, session_name) in session_line_info {
        // Get the screen row for this line (after wrapping and scroll)
        let line_start = cumulative_lines.get(line_idx).copied().unwrap_or(0);

        // Calculate visible row (0-indexed from top of inner area)
        if line_start >= scroll_offset {
            let visible_row = line_start - scroll_offset;
            if visible_row < inner_height {
                // Create clickable region for this session line
                let click_rect = Rect {
                    x: inner_area.x,
                    y: inner_area.y + visible_row as u16,
                    width: inner_area.width,
                    height: 1,
                };
                app.add_clickable_item(click_rect, ClickAction::Connect(session_name));
            }
        }
    }

    let text = Text::from(lines);

    let output = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset as u16, 0));

    frame.render_widget(output, area);
}

/// Extract session name from a /list output line.
///
/// Format: "  [Claude|Shell|?] session-name (connected)? - activity"
fn extract_clickable_session(content: &str) -> Option<String> {
    let trimmed = content.trim_start();

    // Check for session indicator pattern
    let after_indicator = if trimmed.starts_with("[Claude]") {
        trimmed.strip_prefix("[Claude]")
    } else if trimmed.starts_with("[Shell]") {
        trimmed.strip_prefix("[Shell]")
    } else if trimmed.starts_with("[?]") {
        trimmed.strip_prefix("[?]")
    } else {
        None
    }?;

    // Session name follows, separated by space
    let session_part = after_indicator.trim_start();

    // Extract session name (up to space, "(connected)", or " - ")
    let session_name = session_part
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()?;

    if session_name.is_empty() {
        return None;
    }

    Some(session_name.to_string())
}

/// Draw the status/progress bar.
fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    if app.is_summarizing() {
        // Summarizing phase - show indeterminate spinner style
        let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let idx = ((app.progress * 10.0) as usize) % spinner.len();
        let label = format!(" {} Summarizing response... ", spinner[idx]);
        let status = Paragraph::new(label)
            .style(Style::default().bg(Color::Magenta).fg(Color::White));
        frame.render_widget(status, area);
    } else if app.is_working {
        // Receiving phase - show line count
        let line_count = app.response_buffer_len();
        let label = format!(" Receiving... ({} lines captured) ", line_count);
        let status = Paragraph::new(label)
            .style(Style::default().bg(Color::Yellow).fg(Color::Black));
        frame.render_widget(status, area);
    } else {
        // Show connection status
        let status_text = if let Some(project) = &app.project {
            format!(" Ready - {} ", project)
        } else {
            " No project connected ".to_string()
        };
        let status = Paragraph::new(status_text)
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));
        frame.render_widget(status, area);
    }
}

/// Draw the input area with text wrapping support and optional hint.
fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let input_style = match app.input_mode {
        InputMode::Normal => Style::default(),
        InputMode::Scrolling => Style::default().fg(Color::DarkGray),
    };

    let prompt = match &app.project {
        Some(name) => format!("[{}]> ", name),
        None => "commander> ".to_string(),
    };

    // Check if we need to show a hint
    let hint = app.get_command_hint();
    let input_area = if hint.is_some() {
        // Split area for input and hint
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),      // Input area (at least 3 lines)
                Constraint::Length(1),   // Hint area
            ])
            .split(area);

        // Render hint
        if let Some(hint_text) = hint {
            let hint_widget = Paragraph::new(format!("  {}", hint_text))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint_widget, chunks[1]);
        }

        chunks[0]
    } else {
        area
    };

    let input_text = format!("{}{}", prompt, app.input);

    let input = Paragraph::new(input_text.clone())
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title(" Input "))
        .wrap(Wrap { trim: false });

    frame.render_widget(input, input_area);

    // Calculate cursor position accounting for wrapping
    let inner_width = input_area.width.saturating_sub(2) as usize; // Account for borders
    let cursor_offset = prompt.len() + app.cursor_pos;

    if inner_width > 0 {
        let cursor_line = cursor_offset / inner_width;
        let cursor_col = cursor_offset % inner_width;

        let cursor_x = input_area.x + 1 + cursor_col as u16;
        let cursor_y = input_area.y + 1 + cursor_line as u16;

        // Only set cursor if it's within the visible area
        let max_y = input_area.y + input_area.height.saturating_sub(1);
        if cursor_y <= max_y {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    } else {
        frame.set_cursor_position((input_area.x + 1, input_area.y + 1));
    }
}

/// Draw the footer with keybindings.
fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let project_indicator = if let Some(p) = &app.project {
        p.as_str()
    } else {
        "no project"
    };

    let keys = if app.input_mode == InputMode::Scrolling {
        "j/k scroll | Enter: back to input | q: quit"
    } else {
        "↑/↓: history | PgUp/PgDn: scroll | /help | Ctrl+C: quit"
    };

    let footer_text = format!(" {} | {} ", project_indicator, keys);
    let footer = Paragraph::new(footer_text)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(footer, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_clickable_session_claude() {
        let result = extract_clickable_session("  [Claude] commander-myproject (connected) - Waiting for input");
        assert_eq!(result, Some("commander-myproject".to_string()));
    }

    #[test]
    fn test_extract_clickable_session_shell() {
        let result = extract_clickable_session("  [Shell] my-shell-session - Active");
        assert_eq!(result, Some("my-shell-session".to_string()));
    }

    #[test]
    fn test_extract_clickable_session_unknown() {
        let result = extract_clickable_session("  [?] unknown-session - Idle");
        assert_eq!(result, Some("unknown-session".to_string()));
    }

    #[test]
    fn test_extract_clickable_session_no_indicator() {
        let result = extract_clickable_session("Sessions:");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_clickable_session_not_session_line() {
        let result = extract_clickable_session("Welcome to Commander TUI");
        assert_eq!(result, None);
    }

    #[test]
    fn test_clickable_item_contains() {
        let item = super::super::app::ClickableItem {
            rect: Rect { x: 10, y: 5, width: 20, height: 1 },
            action: super::super::app::ClickAction::Connect("test".to_string()),
        };

        // Inside region
        assert!(item.contains(10, 5));
        assert!(item.contains(15, 5));
        assert!(item.contains(29, 5)); // x + width - 1

        // Outside region
        assert!(!item.contains(9, 5));   // Before x
        assert!(!item.contains(30, 5));  // After x + width
        assert!(!item.contains(15, 4));  // Before y
        assert!(!item.contains(15, 6));  // After y + height
    }
}
