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

/// Polling interval for notification checking (less frequent).
const NOTIFICATION_POLL_INTERVAL_MS: u64 = 2000;

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

        // Initialize agent orchestrator (if agents feature enabled)
        #[cfg(feature = "agents")]
        {
            if let Err(e) = self.state.init_orchestrator().await {
                warn!(error = %e, "Could not initialize orchestrator");
            }
        }

        let bot = self.bot.clone();
        let state = Arc::clone(&self.state);

        // Start the output polling task
        let poll_state = Arc::clone(&self.state);
        let poll_bot = bot.clone();
        tokio::spawn(async move {
            poll_output_loop(poll_bot, poll_state).await;
        });

        // Start the notification polling task
        let notify_state = Arc::clone(&self.state);
        let notify_bot = bot.clone();
        tokio::spawn(async move {
            poll_notifications_loop(notify_bot, notify_state).await;
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
                        info!(chat_id = %msg.chat.id, "Command matched: {:?}", cmd);
                        async move { handle_command(bot, msg, cmd, state).await }
                    }),
            )
            .branch(
                Update::filter_message()
                    .filter(|msg: Message| {
                        // Handle unrecognized commands (start with / but didn't parse)
                        let is_cmd = msg.text()
                            .map(|t| t.starts_with('/'))
                            .unwrap_or(false);
                        if is_cmd {
                            info!(text = ?msg.text(), "Command didn't parse, falling through to unknown handler");
                        }
                        is_cmd
                    })
                    .endpoint(move |bot: Bot, msg: Message| {
                        async move {
                            if let Some(text) = msg.text() {
                                info!(cmd = %text, "Unrecognized command - sending response");
                                bot.send_message(
                                    msg.chat.id,
                                    format!("Unknown command: {}\n\nUse /help to see available commands.", text.split_whitespace().next().unwrap_or(text)),
                                ).await?;
                            }
                            Ok(())
                        }
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
                        info!(chat_id = %msg.chat.id, text = ?msg.text(), "Regular message received");
                        async move { handle_message(bot, msg, state).await }
                    }),
            );

        info!("Bot is running! Send /start to begin.");

        Dispatcher::builder(bot, handler)
            .default_handler(|upd| async move {
                warn!("Unhandled update: {:?}", upd);
            })
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
    use teloxide::types::{ChatId, ChatAction, ReplyParameters};

    let mut poll_interval = interval(Duration::from_millis(POLL_INTERVAL_MS));

    loop {
        poll_interval.tick().await;

        // Get all chat IDs that are waiting for responses
        let waiting_ids = state.get_waiting_chat_ids().await;

        for chat_id in waiting_ids {
            // Refresh typing indicator to show processing is ongoing
            let _ = bot.send_chat_action(ChatId(chat_id), ChatAction::Typing).await;

            // Poll for output from this session
            match state.poll_output(ChatId(chat_id)).await {
                Ok(Some((response, message_id))) => {
                    // Send the response back to the user, as a reply if we have a message ID
                    let send_result = if let Some(msg_id) = message_id {
                        bot.send_message(ChatId(chat_id), &response)
                            .reply_parameters(ReplyParameters::new(msg_id))
                            .await
                    } else {
                        bot.send_message(ChatId(chat_id), &response).await
                    };

                    if let Err(e) = send_result {
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

/// Background task to poll for cross-channel notifications and broadcast to authorized users.
async fn poll_notifications_loop(bot: Bot, state: Arc<TelegramState>) {
    use teloxide::types::ChatId;
    use crate::notifications::{get_unread_notifications, mark_notifications_read};

    let mut poll_interval = interval(Duration::from_millis(NOTIFICATION_POLL_INTERVAL_MS));

    loop {
        poll_interval.tick().await;

        // Get unread notifications for the telegram channel
        let notifications = get_unread_notifications("telegram");
        if notifications.is_empty() {
            continue;
        }

        // Get all authorized chat IDs
        let authorized_chats = state.get_authorized_chat_ids().await;
        if authorized_chats.is_empty() {
            // No authorized users yet, mark as read anyway to avoid backlog
            let ids: Vec<_> = notifications.iter().map(|n| n.id.clone()).collect();
            if let Err(e) = mark_notifications_read("telegram", &ids) {
                warn!(error = %e, "Failed to mark notifications as read");
            }
            continue;
        }

        // Send each notification to all authorized chats
        // Note: Notifications already have clean, conversational formatting from
        // notify_session_ready/notify_session_resumed/notify_sessions_waiting.
        // No LLM summarization needed - it only introduces preamble bleeding.
        let mut sent_ids = Vec::new();
        for notification in &notifications {
            for &chat_id in &authorized_chats {
                if let Err(e) = bot.send_message(ChatId(chat_id), &notification.message).await {
                    warn!(chat_id = %chat_id, error = %e, "Failed to send notification");
                } else {
                    info!(chat_id = %chat_id, notification_id = %notification.id, "Notification sent");
                }
            }
            sent_ids.push(notification.id.clone());
        }

        // Mark notifications as read
        if let Err(e) = mark_notifications_read("telegram", &sent_ids) {
            warn!(error = %e, "Failed to mark notifications as read");
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests would go here but require mocking the Telegram API
}
