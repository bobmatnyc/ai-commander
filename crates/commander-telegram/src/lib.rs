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
//! - **AgentOrchestrator integration** (with `agents` feature): Routes messages
//!   through LLM for intelligent interpretation and generates human-readable
//!   notification summaries
//!
//! # Cargo Features
//!
//! - `agents` (default): Enables AgentOrchestrator integration for LLM-based
//!   message processing and notification summarization
//!
//! # Environment Variables
//!
//! Required:
//! - `TELEGRAM_BOT_TOKEN`: Bot token from @BotFather
//! - `NGROK_AUTHTOKEN`: ngrok authentication token (for webhook mode)
//!
//! Optional:
//! - `OPENROUTER_API_KEY`: For response summarization (and agents feature)
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
pub mod notifications;
pub mod pairing;
pub mod session;
pub mod state;

pub use bot::TelegramBot;
pub use error::{Result, TelegramError};
pub use ngrok::NgrokTunnel;
pub use notifications::{
    get_unread_notifications, mark_notifications_read, notify_session_ready,
    notify_session_resumed, notify_sessions_waiting, push_notification, Notification,
};
pub use pairing::{consume_pairing, create_pairing, generate_code};
pub use session::UserSession;
pub use state::{create_shared_state, TelegramState};
