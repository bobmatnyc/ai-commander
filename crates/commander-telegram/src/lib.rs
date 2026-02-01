//! Telegram bot interface for Commander.
//!
//! This crate provides a Telegram bot that allows users to interact with
//! Claude Code sessions remotely via Telegram messages.
//!
//! # Features
//!
//! - Connect to Commander projects from Telegram
//! - Send messages to Claude Code and receive responses
//! - Automatic response summarization via OpenRouter
//! - ngrok integration for webhook tunneling
//!
//! # Environment Variables
//!
//! Required:
//! - `TELEGRAM_BOT_TOKEN`: Bot token from @BotFather
//! - `NGROK_AUTHTOKEN`: ngrok authentication token (for webhook mode)
//!
//! Optional:
//! - `OPENROUTER_API_KEY`: For response summarization
//! - `OPENROUTER_MODEL`: Model to use (default: anthropic/claude-sonnet-4)
//! - `TELEGRAM_WEBHOOK_PORT`: Webhook port (default: 8443)
//!
//! # Example
//!
//! ```no_run
//! use commander_telegram::TelegramBot;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize with state directory
//!     let state_dir = Path::new("/path/to/state");
//!     let mut bot = TelegramBot::new(state_dir)?;
//!
//!     // Start in polling mode (simpler, no ngrok needed)
//!     bot.start_polling().await?;
//!
//!     // Or start with webhook (requires ngrok)
//!     // bot.start().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Commands
//!
//! - `/start` - Welcome message and help
//! - `/help` - Show available commands
//! - `/connect <project>` - Connect to a project
//! - `/disconnect` - Disconnect from current project
//! - `/status` - Show connection status
//! - `/list` - List available projects

pub mod bot;
pub mod error;
pub mod handlers;
pub mod ngrok;
pub mod pairing;
pub mod session;
pub mod state;

pub use bot::TelegramBot;
pub use error::{Result, TelegramError};
pub use ngrok::NgrokTunnel;
pub use pairing::{consume_pairing, create_pairing, generate_code};
pub use session::UserSession;
pub use state::{create_shared_state, TelegramState};
