//! TUI rendering using ratatui.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::app::{App, InputMode, MessageDirection, SessionInfo, ViewMode};

/// Draw the TUI.
pub fn draw(frame: &mut Frame, app: &App) {
    match app.view_mode {
        ViewMode::Normal => draw_normal(frame, app),
        ViewMode::Inspect => draw_inspect(frame, app),
        ViewMode::Sessions => draw_sessions(frame, app),
    }
}

/// Draw normal chat mode.
fn draw_normal(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Header
            Constraint::Min(5),     // Output area
            Constraint::Length(1),  // Status/Progress bar
            Constraint::Length(5),  // Input area (increased for wrapping)
            Constraint::Length(1),  // Footer
        ])
        .split(frame.area());

    draw_header(frame, app, chunks[0]);
    draw_output(frame, app, chunks[1]);
    draw_status(frame, app, chunks[2]);
    draw_input(frame, app, chunks[3]);
    draw_footer(frame, app, chunks[4]);
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
fn draw_output(frame: &mut Frame, app: &App, area: Rect) {
    let title = if app.scroll_offset > 0 {
        format!(" Output [scroll: {}] ", app.scroll_offset)
    } else {
        " Output ".to_string()
    };

    // Build lines from messages
    let lines: Vec<Line> = app
        .messages
        .iter()
        .map(|msg| {
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
            Line::from(vec![Span::styled(content, style)])
        })
        .collect();

    // Calculate scroll - estimate wrapped line count
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;

    // Estimate total lines after wrapping
    let total_wrapped_lines: usize = lines
        .iter()
        .map(|line| {
            let line_len: usize = line.spans.iter().map(|s| s.content.len()).sum();
            if inner_width > 0 {
                ((line_len + inner_width - 1) / inner_width).max(1)
            } else {
                1
            }
        })
        .sum();

    // Calculate scroll offset to show latest content, adjusted by user scroll
    let scroll_offset = if total_wrapped_lines > inner_height {
        (total_wrapped_lines - inner_height).saturating_sub(app.scroll_offset) as u16
    } else {
        0
    };

    let text = Text::from(lines);

    let output = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0));

    frame.render_widget(output, area);
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

/// Draw the input area with text wrapping support.
fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let input_style = match app.input_mode {
        InputMode::Normal => Style::default(),
        InputMode::Scrolling => Style::default().fg(Color::DarkGray),
    };

    let prompt = match &app.project {
        Some(name) => format!("[{}]> ", name),
        None => "commander> ".to_string(),
    };
    let input_text = format!("{}{}", prompt, app.input);

    let input = Paragraph::new(input_text.clone())
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title(" Input "))
        .wrap(Wrap { trim: false });

    frame.render_widget(input, area);

    // Calculate cursor position accounting for wrapping
    let inner_width = area.width.saturating_sub(2) as usize; // Account for borders
    let cursor_offset = prompt.len() + app.cursor_pos;

    if inner_width > 0 {
        let cursor_line = cursor_offset / inner_width;
        let cursor_col = cursor_offset % inner_width;

        let cursor_x = area.x + 1 + cursor_col as u16;
        let cursor_y = area.y + 1 + cursor_line as u16;

        // Only set cursor if it's within the visible area
        let max_y = area.y + area.height.saturating_sub(1);
        if cursor_y <= max_y {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    } else {
        frame.set_cursor_position((area.x + 1, area.y + 1));
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
