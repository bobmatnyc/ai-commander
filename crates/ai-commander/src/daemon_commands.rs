//! Daemon management commands for the CLI.

use commander_daemon::DaemonService;

use crate::cli::DaemonCommands;
use crate::commands::Result;

/// Execute daemon management commands.
pub async fn execute(command: DaemonCommands) -> Result<()> {
    match command {
        DaemonCommands::Start { foreground } => {
            println!("Starting daemon service...");

            let service = DaemonService::new().await
                .map_err(|e| format!("Failed to create daemon service: {}", e))?;

            if foreground {
                println!("Running daemon in foreground mode. Press Ctrl+C to stop.");
                service.run().await
                    .map_err(|e| format!("Daemon service error: {}", e))?;
            } else {
                service.daemonize().await
                    .map_err(|e| format!("Failed to start daemon: {}", e))?;
                println!("Daemon service started successfully");
            }

            Ok(())
        }

        DaemonCommands::Stop => {
            println!("Stopping daemon service...");
            DaemonService::stop().await
                .map_err(|e| format!("Failed to stop daemon: {}", e))?;
            println!("Daemon service stopped successfully");
            Ok(())
        }

        DaemonCommands::Status => {
            let status = DaemonService::status().await
                .map_err(|e| format!("Failed to get daemon status: {}", e))?;

            println!("{}", serde_json::to_string_pretty(&status)
                .map_err(|e| format!("Failed to format status: {}", e))?);

            Ok(())
        }

        DaemonCommands::Restart => {
            println!("Restarting daemon service...");
            let service = DaemonService::new().await
                .map_err(|e| format!("Failed to create daemon service: {}", e))?;

            service.restart().await
                .map_err(|e| format!("Failed to restart daemon: {}", e))?;

            println!("Daemon service restarted successfully");
            Ok(())
        }
    }
}

/// Generate a pairing code.
pub async fn generate_pairing_code(session_id: Option<String>) -> Result<()> {
    println!("Generating pairing code...");

    let code = DaemonService::generate_pairing_code(session_id.as_deref()).await
        .map_err(|e| format!("Failed to generate pairing code: {}", e))?;

    println!("Pairing code: {}", code);
    println!("This code is valid for 5 minutes.");

    Ok(())
}
