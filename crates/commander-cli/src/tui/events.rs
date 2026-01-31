//! Event handling for the TUI.

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use super::app::{App, ViewMode};
use super::ui;

/// Result type for TUI operations.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Initialize the terminal for TUI mode.
fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to normal mode.
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Run the TUI event loop.
pub fn run(state_dir: &std::path::Path, connect_to: Option<String>) -> Result<()> {
    // Setup terminal
    let mut terminal = setup_terminal()?;

    // Create app
    let mut app = App::new(state_dir);

    // Auto-connect if project specified
    if let Some(project) = connect_to {
        if let Err(e) = app.connect(&project) {
            app.messages.push(super::app::Message::system(format!("Failed to connect: {}", e)));
        }
    }

    // Run event loop
    let result = run_loop(&mut terminal, &mut app);

    // Restore terminal
    restore_terminal(&mut terminal)?;

    result
}

/// Main event loop.
fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    let tick_rate = Duration::from_millis(100);

    loop {
        // Draw UI
        terminal.draw(|f| ui::draw(f, app))?;

        // Poll for events with timeout
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events (not release)
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Handle Ctrl+C to quit
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    app.should_quit = true;
                }

                // Handle F2 to toggle inspect mode
                if key.code == KeyCode::F(2) {
                    app.toggle_inspect_mode();
                    continue;
                }

                // Handle keys based on view mode
                match app.view_mode {
                    ViewMode::Inspect => {
                        // In inspect mode, handle scroll and exit
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => app.inspect_scroll_up(),
                            KeyCode::Down | KeyCode::Char('j') => app.inspect_scroll_down(),
                            KeyCode::PageUp => app.inspect_scroll_page_up(10),
                            KeyCode::PageDown => app.inspect_scroll_page_down(10),
                            KeyCode::Esc | KeyCode::Char('q') => app.toggle_inspect_mode(),
                            _ => {}
                        }
                    }
                    ViewMode::Normal => {
                        // Normal mode key handling
                        match key.code {
                            KeyCode::Enter => app.submit(),
                            KeyCode::Char(c) => app.enter_char(c),
                            KeyCode::Backspace => app.delete_char(),
                            KeyCode::Left => app.move_cursor_left(),
                            KeyCode::Right => app.move_cursor_right(),
                            KeyCode::Up => app.scroll_up(),
                            KeyCode::Down => app.scroll_down(),
                            KeyCode::PageUp => app.scroll_page_up(10),
                            KeyCode::PageDown => app.scroll_page_down(10),
                            KeyCode::Esc => {
                                if app.is_working {
                                    app.stop_working();
                                } else {
                                    app.should_quit = true;
                                }
                            }
                            KeyCode::Home => {
                                app.cursor_pos = 0;
                            }
                            KeyCode::End => {
                                app.cursor_pos = app.input.len();
                            }
                            _ => {}
                        }
                    }
                }

                // Handle Ctrl+L to clear
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
                    app.messages.clear();
                    app.messages.push(super::app::Message::system("Output cleared"));
                }
            }
        }

        // Poll for tmux output if working
        if app.is_working {
            app.poll_output();
        }

        // Auto-refresh inspect content
        if app.view_mode == ViewMode::Inspect {
            app.refresh_inspect_content();
        }

        // Check if should quit
        if app.should_quit {
            break;
        }
    }

    Ok(())
}
