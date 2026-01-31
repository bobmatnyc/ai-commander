//! TUI rendering using ratatui.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame,
};

use super::app::{App, InputMode, MessageDirection};

/// Draw the TUI.
pub fn draw(frame: &mut Frame, app: &App) {
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

    let prompt = if app.project.is_some() { "> " } else { "commander> " };
    let input_text = format!("{}{}", prompt, app.input);

    let input = Paragraph::new(input_text)
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title(" Input "));

    frame.render_widget(input, area);

    // Set cursor position
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
