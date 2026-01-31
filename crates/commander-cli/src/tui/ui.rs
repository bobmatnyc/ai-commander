//! TUI rendering using ratatui.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
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
            Constraint::Length(3),  // Input area
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
fn format_session_item(index: usize, session: &SessionInfo, selected: usize) -> ListItem<'static> {
    let marker = if index == selected { ">" } else { " " };
    let status = if session.is_connected {
        "* connected".to_string()
    } else if session.is_commander {
        "o idle".to_string()
    } else {
        "(external)".to_string()
    };

    let style = if index == selected {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if session.is_connected {
        Style::default().fg(Color::Green)
    } else if !session.is_commander {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };

    ListItem::new(format!("  {} {:<30} {}", marker, session.name, status)).style(style)
}

/// Draw the header bar.
fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let project_name = app.project.as_deref().unwrap_or("none");
    let status = if app.project.is_some() { "connected" } else { "disconnected" };

    let header_text = format!(" Commander - [{}] {} ", project_name, status);

    let header = Paragraph::new(header_text)
        .style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD));

    frame.render_widget(header, area);
}

/// Draw the scrollable output area.
fn draw_output(frame: &mut Frame, app: &App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize; // Account for borders

    // Calculate visible range considering scroll offset
    let total_messages = app.messages.len();
    let end_idx = total_messages.saturating_sub(app.scroll_offset);
    let start_idx = end_idx.saturating_sub(inner_height);

    let items: Vec<ListItem> = app.messages[start_idx..end_idx]
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
            ListItem::new(Line::from(vec![Span::styled(content, style)]))
        })
        .collect();

    let title = if app.scroll_offset > 0 {
        format!(" Output [scroll: {}] ", app.scroll_offset)
    } else {
        " Output ".to_string()
    };

    let output = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title));

    frame.render_widget(output, area);
}

/// Draw the status/progress bar.
fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    if app.is_working {
        let label = " [working...] ";
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
            .ratio(app.progress)
            .label(label);
        frame.render_widget(gauge, area);
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

/// Draw the input area.
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

    let input = Paragraph::new(input_text)
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title(" Input "));

    frame.render_widget(input, area);

    // Set cursor position (prompt is now dynamic length)
    let cursor_x = area.x + prompt.len() as u16 + app.cursor_pos as u16 + 1;
    let cursor_y = area.y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));
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
        "Up/Down: scroll | Enter: send | /help | Ctrl+C: quit"
    };

    let footer_text = format!(" {} | {} ", project_indicator, keys);
    let footer = Paragraph::new(footer_text)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(footer, area);
}
