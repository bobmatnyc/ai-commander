//! Commander CLI entry point.

use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

use commander_cli::cli::{Cli, Commands};
use commander_cli::commands;
use commander_cli::repl::Repl;

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
