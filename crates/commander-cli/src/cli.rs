//! Command-line interface definition using clap.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Build version string with git hash and build date.
fn version_string() -> &'static str {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const GIT_HASH: &str = env!("GIT_HASH");
    const BUILD_DATE: &str = env!("BUILD_DATE");

    // Use a static string that combines all version info
    // Format: "0.1.0 (abc1234, 2026-01-29)"
    static VERSION_STRING: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION_STRING.get_or_init(|| format!("{} ({}, {})", VERSION, GIT_HASH, BUILD_DATE))
}

/// Commander - Multi-project AI orchestration system
#[derive(Parser, Debug)]
#[command(name = "commander")]
#[command(author, version = version_string(), about, long_about = None)]
pub struct Cli {
    /// Enable verbose output (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Path to state directory
    #[arg(short, long, env = "COMMANDER_STATE_DIR")]
    pub state_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start a new project instance
    Start {
        /// Path to the project directory
        #[arg(required = true)]
        path: PathBuf,

        /// Runtime adapter to use (default: claude-code)
        #[arg(short, long, default_value = "claude-code")]
        adapter: String,

        /// Project name (default: directory name)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Stop a running project instance
    Stop {
        /// Project ID or name
        #[arg(required = true)]
        project: String,

        /// Force stop without graceful shutdown
        #[arg(short, long)]
        force: bool,
    },

    /// List all projects
    List {
        /// Show only running projects
        #[arg(short, long)]
        running: bool,

        /// Output format (table, json, brief)
        #[arg(short, long, default_value = "table")]
        format: OutputFormat,
    },

    /// Show status of a project
    Status {
        /// Project ID or name (shows all if omitted)
        project: Option<String>,

        /// Show detailed status including events
        #[arg(short, long)]
        detailed: bool,
    },

    /// Send a message to a project
    Send {
        /// Project ID or name
        #[arg(required = true)]
        project: String,

        /// Message to send
        #[arg(required = true)]
        message: String,
    },

    /// Start interactive REPL mode
    Repl {
        /// Connect to specific project on start
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Show available runtime adapters
    Adapters,
}

/// Output format for list commands
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Brief,
}

impl Cli {
    /// Returns the state directory path, using default if not specified.
    pub fn state_dir(&self) -> PathBuf {
        self.state_dir.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".commander"))
                .unwrap_or_else(|| PathBuf::from(".commander"))
        })
    }

    /// Returns the log level based on verbosity.
    pub fn log_level(&self) -> tracing::Level {
        match self.verbose {
            0 => tracing::Level::WARN,
            1 => tracing::Level::INFO,
            2 => tracing::Level::DEBUG,
            _ => tracing::Level::TRACE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_parse_no_args() {
        // No args should work (enters REPL mode)
        let cli = Cli::parse_from(["commander"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_parse_start() {
        let cli = Cli::parse_from(["commander", "start", "/path/to/project"]);
        match cli.command {
            Some(Commands::Start { path, adapter, .. }) => {
                assert_eq!(path, PathBuf::from("/path/to/project"));
                assert_eq!(adapter, "claude-code");
            }
            _ => panic!("Expected Start command"),
        }
    }

    #[test]
    fn test_cli_parse_list() {
        let cli = Cli::parse_from(["commander", "list", "--running"]);
        match cli.command {
            Some(Commands::List { running, .. }) => {
                assert!(running);
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_cli_verbose() {
        let cli = Cli::parse_from(["commander", "-vvv"]);
        assert_eq!(cli.verbose, 3);
        assert_eq!(cli.log_level(), tracing::Level::TRACE);
    }

    #[test]
    fn test_cli_help() {
        // Verify help can be generated without panic
        Cli::command().debug_assert();
    }
}
