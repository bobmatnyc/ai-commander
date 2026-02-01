//! Main Telegram bot implementation.

use std::sync::Arc;
use std::time::Duration;

use teloxide::dispatching::UpdateFilterExt;
use teloxide::prelude::*;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{info, warn};

use crate::error::{Result, TelegramError};
use crate::handlers::{handle_command, handle_message, Command};
use crate::ngrok::NgrokTunnel;
use crate::state::{create_shared_state, TelegramState};

/// Default webhook port.
const DEFAULT_WEBHOOK_PORT: u16 = 8443;

/// Polling interval for output checking.
const POLL_INTERVAL_MS: u64 = 500;

/// The Telegram bot for Commander.
pub struct TelegramBot {
    /// The teloxide bot instance.
    bot: Bot,
    /// Shared state across handlers.
    state: Arc<TelegramState>,
    /// ngrok tunnel for webhook.
    ngrok: Option<NgrokTunnel>,
    /// Webhook port.
    webhook_port: u16,
    /// Shutdown signal sender.
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl TelegramBot {
    /// Create a new TelegramBot instance.
    ///
    /// Requires `TELEGRAM_BOT_TOKEN` environment variable to be set.
    pub fn new(state_dir: &std::path::Path) -> Result<Self> {
        let token = std::env::var("TELEGRAM_BOT_TOKEN")
            .map_err(|_| TelegramError::NoToken)?;

        let webhook_port = std::env::var("TELEGRAM_WEBHOOK_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_WEBHOOK_PORT);

        let bot = Bot::new(token);
        let state = create_shared_state(state_dir);

        Ok(Self {
            bot,
            state,
            ngrok: None,
            webhook_port,
            shutdown_tx: None,
        })
    }

    /// Create a TelegramBot with custom state (for testing).
    pub fn with_state(state: Arc<TelegramState>) -> Result<Self> {
        let token = std::env::var("TELEGRAM_BOT_TOKEN")
            .map_err(|_| TelegramError::NoToken)?;

        let webhook_port = std::env::var("TELEGRAM_WEBHOOK_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_WEBHOOK_PORT);

        let bot = Bot::new(token);

        Ok(Self {
            bot,
            state,
            ngrok: None,
            webhook_port,
            shutdown_tx: None,
        })
    }

    /// Get the bot's username.
    pub async fn get_me(&self) -> Result<String> {
        let me = self.bot.get_me().await
            .map_err(|e| TelegramError::BotStartFailed(e.to_string()))?;
        Ok(me.username().to_string())
    }

    /// Start the bot with ngrok webhook.
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Telegram bot with webhook...");

        // Start ngrok tunnel
        info!(port = self.webhook_port, "Starting ngrok tunnel");
        let ngrok = NgrokTunnel::start(self.webhook_port).await?;
        let webhook_url = format!("{}/webhook", ngrok.public_url());
        info!(url = %webhook_url, "Webhook URL ready");

        self.ngrok = Some(ngrok);

        // For webhook mode, we'd use axum with teloxide's webhook adapter
        // For now, fall back to polling mode with a note
        warn!("Webhook mode requires additional setup. Starting in polling mode instead.");
        self.start_polling().await
    }

    /// Start the bot in polling mode (simpler, no ngrok needed).
    pub async fn start_polling(&self) -> Result<()> {
        info!("Starting Telegram bot in polling mode...");

        let bot = self.bot.clone();
        let state = Arc::clone(&self.state);

        // Start the output polling task
        let poll_state = Arc::clone(&self.state);
        let poll_bot = bot.clone();
        tokio::spawn(async move {
            poll_output_loop(poll_bot, poll_state).await;
        });

        // Set up the command and message handlers
        let state_for_commands = Arc::clone(&state);
        let state_for_messages = Arc::clone(&state);

        let handler = dptree::entry()
            .branch(
                Update::filter_message()
                    .filter_command::<Command>()
                    .endpoint(move |bot: Bot, msg: Message, cmd: Command| {
                        let state = Arc::clone(&state_for_commands);
                        async move { handle_command(bot, msg, cmd, state).await }
                    }),
            )
            .branch(
                Update::filter_message()
                    .filter(|msg: Message| {
                        // Only handle non-command text messages
                        msg.text()
                            .map(|t| !t.starts_with('/'))
                            .unwrap_or(false)
                    })
                    .endpoint(move |bot: Bot, msg: Message| {
                        let state = Arc::clone(&state_for_messages);
                        async move { handle_message(bot, msg, state).await }
                    }),
            );

        info!("Bot is running! Send /start to begin.");

        Dispatcher::builder(bot, handler)
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;

        Ok(())
    }

    /// Stop the bot.
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping Telegram bot...");

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Stop ngrok
        if let Some(mut ngrok) = self.ngrok.take() {
            ngrok.stop()?;
        }

        info!("Bot stopped");
        Ok(())
    }

    /// Get the current webhook URL (if running with ngrok).
    pub fn webhook_url(&self) -> Option<String> {
        self.ngrok.as_ref().map(|n| format!("{}/webhook", n.public_url()))
    }
}

/// Background task to poll for output from connected sessions and send responses.
async fn poll_output_loop(bot: Bot, state: Arc<TelegramState>) {
    use teloxide::types::ChatId;

    let mut poll_interval = interval(Duration::from_millis(POLL_INTERVAL_MS));

    loop {
        poll_interval.tick().await;

        // Get all chat IDs that are waiting for responses
        let waiting_ids = state.get_waiting_chat_ids().await;

        for chat_id in waiting_ids {
            // Poll for output from this session
            match state.poll_output(ChatId(chat_id)).await {
                Ok(Some(response)) => {
                    // Send the response back to the user
                    if let Err(e) = bot.send_message(ChatId(chat_id), &response).await {
                        warn!(chat_id = %chat_id, error = %e, "Failed to send response");
                    } else {
                        info!(chat_id = %chat_id, "Response sent to user");
                    }
                }
                Ok(None) => {
                    // No response ready yet, continue polling
                }
                Err(e) => {
                    warn!(chat_id = %chat_id, error = %e, "Error polling output");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests would go here but require mocking the Telegram API
}
