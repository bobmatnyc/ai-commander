//! Commander CLI entry point.

use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

use ai_commander::agent_cli;
use ai_commander::cli::{Cli, Commands};
use ai_commander::commands;
use ai_commander::repl::Repl;
use ai_commander::tui;

fn main() {
    // Load .env.local if it exists (for OPENROUTER_API_KEY etc.)
    let _ = dotenvy::from_filename(".env.local");

    let cli = Cli::parse();

    // Initialize tracing
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(cli.log_level().to_string()));

    fmt().with_env_filter(filter).with_target(false).init();

    // Get state directory
    let state_dir = cli.state_dir();

    // Handle command or enter REPL
    let result = match cli.command {
        Some(Commands::Repl { project }) => run_repl(&state_dir, project),
        Some(Commands::Tui { project }) => run_tui(&state_dir, project),
        Some(Commands::Agent { command }) => agent_cli::execute(command),
        Some(cmd) => commands::execute(cmd, &state_dir),
        None => {
            // No command = enter REPL
            run_repl(&state_dir, None)
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run_repl(state_dir: &std::path::Path, connect_to: Option<String>) -> commands::Result<()> {
    let mut repl = Repl::new(state_dir)?;

    // Auto-connect if project specified
    if let Some(project) = connect_to {
        println!("Connecting to '{}'...", project);
        // Connection handled by REPL internally
    }

    repl.run()?;
    Ok(())
}

fn run_tui(state_dir: &std::path::Path, connect_to: Option<String>) -> commands::Result<()> {
    tui::run(state_dir, connect_to)?;
    Ok(())
}
