//! Commander daemon main entry point.
//!
//! This binary provides the central daemon service for ai-commander.
//! It manages sessions, handles IPC communication, and provides
//! lifecycle management for the entire system.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::info;

use commander_daemon::{DaemonService, Result};

#[derive(Parser)]
#[command(
    name = "commander-daemon",
    about = "Central daemon service for ai-commander",
    long_about = "Provides session management, IPC communication, and unified control for ai-commander system"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Custom configuration directory
    #[arg(long, global = true)]
    config_dir: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the daemon service
    Start {
        /// Run in foreground (don't daemonize)
        #[arg(short, long)]
        foreground: bool,
    },
    /// Stop the daemon service
    Stop,
    /// Show daemon status
    Status,
    /// Restart the daemon service
    Restart,
    /// Generate a pairing code for client connections
    Pair {
        /// Session ID to pair with (optional)
        #[arg(short, long)]
        session: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("commander_daemon={},commander={}", log_level, log_level))
        .init();

    // Load environment configuration
    let _ = dotenvy::from_filename(".env.local");
    if let Some(config_dir) = &cli.config_dir {
        let env_file = config_dir.join(".env.local");
        if env_file.exists() {
            let _ = dotenvy::from_path(&env_file);
        }
    } else {
        let env_file = commander_core::config::env_file();
        if env_file.exists() {
            let _ = dotenvy::from_path(&env_file);
        }
    }

    // Execute command
    match cli.command {
        Commands::Start { foreground } => {
            info!("Starting commander daemon service");
            let service = DaemonService::new().await?;
            if foreground {
                service.run().await
            } else {
                service.daemonize().await
            }
        }
        Commands::Stop => {
            info!("Stopping commander daemon service");
            DaemonService::stop().await
        }
        Commands::Status => {
            let status = DaemonService::status().await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
        Commands::Restart => {
            info!("Restarting commander daemon service");
            let service = DaemonService::new().await?;
            service.restart().await
        }
        Commands::Pair { session } => {
            info!("Generating pairing code");
            let code = DaemonService::generate_pairing_code(session.as_deref()).await?;
            println!("Pairing code: {}", code);
            Ok(())
        }
    }
}
