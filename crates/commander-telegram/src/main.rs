//! Commander Telegram Bot binary.
//!
//! Start the bot with:
//! ```bash
//! TELEGRAM_BOT_TOKEN=xxx cargo run -p commander-telegram
//! ```

use clap::Parser;
use commander_core::config;
use commander_telegram::TelegramBot;
use tracing_subscriber::EnvFilter;

/// Commander Telegram Bot - interact with Claude Code from Telegram
#[derive(Parser, Debug)]
#[command(name = "commander-telegram")]
#[command(about = "Telegram bot for Commander - interact with Claude Code remotely")]
struct Args {
    /// Use webhook mode with ngrok (default: polling mode)
    #[arg(short, long)]
    webhook: bool,

    /// Webhook port (default: 8443)
    #[arg(short, long, default_value = "8443")]
    port: u16,

    /// Verbose logging (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Load environment variables from config directory first
    let env_path = config::env_file();
    if env_path.exists() {
        let _ = dotenvy::from_path(&env_path);
    }
    // Also try local .env.local or .env for backwards compatibility
    let _ = dotenvy::from_filename(".env.local")
        .or_else(|_| dotenvy::dotenv());

    // Initialize logging based on verbosity
    let filter = match args.verbose {
        0 => "commander_telegram=info,teloxide=warn",
        1 => "commander_telegram=debug,teloxide=info",
        2 => "commander_telegram=trace,teloxide=debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // Get state directory
    let state_dir = config::state_dir();

    // Ensure all directories exist
    if let Err(e) = config::ensure_all_dirs() {
        tracing::warn!(error = %e, "Failed to create all directories");
    }

    // Create the bot
    let mut bot = TelegramBot::new(&state_dir)?;

    // Get bot info
    match bot.get_me().await {
        Ok(username) => {
            tracing::info!(username = %username, "Bot initialized successfully");
            println!("\n[robot] Commander Telegram Bot");
            println!("   Bot: @{}", username);
            println!("   Mode: {}", if args.webhook { "webhook" } else { "polling" });
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to get bot info");
            return Err(e.into());
        }
    }

    println!("\n[phone] Open Telegram and send /start to begin");
    println!("   Press Ctrl+C to stop\n");

    // Start the bot
    if args.webhook {
        std::env::set_var("TELEGRAM_WEBHOOK_PORT", args.port.to_string());
        bot.start().await?;
    } else {
        bot.start_polling().await?;
    }

    Ok(())
}
