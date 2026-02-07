//! Event handling for the TUI.

use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

#[cfg(feature = "agents")]
use tokio::runtime::Runtime as TokioRuntime;

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

/// Register SIGHUP handler for hot-restart.
fn setup_signal_handler() -> Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&flag))?;
    Ok(flag)
}

/// Re-exec the current process to pick up a new binary.
/// This replaces the current process entirely, preserving the terminal session.
fn restart_self() -> ! {
    use std::os::unix::process::CommandExt;

    let args: Vec<String> = std::env::args().collect();
    let err = std::process::Command::new(&args[0])
        .args(&args[1..])
        .exec();

    // exec() only returns on error
    eprintln!("Failed to restart: {}", err);
    std::process::exit(1);
}

/// Run the TUI event loop.
pub fn run(state_dir: &std::path::Path, connect_to: Option<String>) -> Result<()> {
    // Load config and check for first-run onboarding
    commander_core::load_config();

    if commander_core::needs_onboarding() {
        if let Err(e) = commander_core::run_onboarding() {
            eprintln!("Onboarding failed: {}", e);
        }
        // Reload config after onboarding
        commander_core::load_config();
    }

    // Setup SIGHUP handler for hot-restart
    let restart_flag = match setup_signal_handler() {
        Ok(flag) => Some(flag),
        Err(e) => {
            eprintln!("Warning: Failed to setup signal handler: {}", e);
            None
        }
    };

    // Setup terminal
    let mut terminal = setup_terminal()?;

    // Create app
    let mut app = App::new(state_dir);

    // Initialize tokio runtime for async operations (agents feature)
    #[cfg(feature = "agents")]
    let _runtime = init_agents_runtime(&mut app);

    // Auto-connect if project specified
    if let Some(project) = connect_to {
        if let Err(e) = app.connect(&project) {
            app.messages.push(super::app::Message::system(format!("Failed to connect: {}", e)));
        }
    }

    // Run event loop
    let result = run_loop(&mut terminal, &mut app, restart_flag.as_ref());

    // Restore terminal before potentially restarting
    restore_terminal(&mut terminal)?;

    // Check if restart was requested
    if restart_flag.as_ref().is_some_and(|f| f.load(Ordering::Relaxed)) {
        restart_self();
    }

    result
}

/// Initialize the tokio runtime and agent orchestrator.
///
/// Returns the runtime which must be kept alive for the duration of the TUI.
#[cfg(feature = "agents")]
fn init_agents_runtime(app: &mut App) -> Option<TokioRuntime> {
    // Create a tokio runtime for async operations
    let runtime = match TokioRuntime::new() {
        Ok(rt) => rt,
        Err(e) => {
            app.messages.push(super::app::Message::system(
                format!("Failed to create async runtime: {}", e),
            ));
            return None;
        }
    };

    // Store the runtime handle for later async operations
    app.set_runtime_handle(Arc::new(runtime.handle().clone()));

    // Initialize the agent orchestrator
    // This happens synchronously during startup for better UX
    app.init_orchestrator_sync();

    Some(runtime)
}

/// Main event loop.
fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    restart_flag: Option<&Arc<AtomicBool>>,
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

                // Handle F3 to show sessions view
                if key.code == KeyCode::F(3) {
                    if app.view_mode == ViewMode::Sessions {
                        app.view_mode = ViewMode::Normal;
                    } else if app.tmux.is_some() {
                        app.show_sessions();
                    } else {
                        app.messages.push(super::app::Message::system("Tmux not available"));
                    }
                    continue;
                }

                // Handle keys based on view mode
                match app.view_mode {
                    ViewMode::Sessions => {
                        // In sessions mode, handle selection and actions
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => app.session_select_up(),
                            KeyCode::Down | KeyCode::Char('j') => app.session_select_down(),
                            KeyCode::Enter => app.connect_selected_session(),
                            KeyCode::Char('d') => app.delete_selected_session(),
                            KeyCode::Esc | KeyCode::Char('q') => {
                                app.view_mode = ViewMode::Normal;
                            }
                            _ => {}
                        }
                    }
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
                            KeyCode::Tab => app.complete_command(),
                            KeyCode::Char(c) => {
                                app.reset_completions();
                                app.enter_char(c);
                            }
                            KeyCode::Backspace => {
                                app.reset_completions();
                                app.delete_char();
                            }
                            KeyCode::Left => app.move_cursor_left(),
                            KeyCode::Right => app.move_cursor_right(),
                            KeyCode::Up => app.history_prev(),
                            KeyCode::Down => app.history_next(),
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

        // Check session status for "waiting for input" notifications
        app.check_session_status();

        // Full scan of all sessions every 5 minutes
        app.scan_all_sessions();

        // Check if should quit
        if app.should_quit {
            break;
        }

        // Check if restart was requested via SIGHUP
        if restart_flag.is_some_and(|f| f.load(Ordering::Relaxed)) {
            app.messages.push(super::app::Message::system("Restart requested, reloading..."));
            break;
        }
    }

    Ok(())
}
