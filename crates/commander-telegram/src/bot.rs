//! Main Telegram bot implementation.

use std::sync::Arc;
use std::time::Duration;

use commander_core::options::{DetectedOptions, OptionDetector, OptionFormat};
use teloxide::dispatching::UpdateFilterExt;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::error::{Result, TelegramError};
use crate::features::{apply_expandable_blockquotes, split_message, FeatureSet, EFFECT_ID_CONFETTI};
use crate::handlers::{handle_callback, handle_command, handle_message, Command};
use crate::ngrok::NgrokTunnel;
use crate::state::{create_shared_state, PollResult, TelegramState};

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

        // Configure HTTP client with timeouts for better connection stability
        // Use teloxide's net module to get the correct reqwest version
        let client = teloxide::net::default_reqwest_settings()
            .timeout(Duration::from_secs(120))          // Read timeout - long enough for getUpdates
            .connect_timeout(Duration::from_secs(30))   // Connect timeout
            .pool_idle_timeout(Duration::from_secs(90)) // Keep connections alive
            .pool_max_idle_per_host(2)                  // Limit connection pool
            .build()
            .map_err(|e| TelegramError::BotStartFailed(format!("Failed to create HTTP client: {}", e)))?;

        let bot = Bot::with_client(token, client);
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

        // Check for rebuild and restore sessions
        let (is_rebuild, is_first_start, start_count) = crate::version::check_rebuild();
        info!(
            is_rebuild = is_rebuild,
            is_first_start = is_first_start,
            start_count = start_count,
            "Bot version checked"
        );

        // Restore sessions from disk
        let (restored_count, total_count) = self.state.load_sessions().await;
        if total_count > 0 {
            info!(
                restored = restored_count,
                total = total_count,
                "Session restoration complete"
            );
        }

        // Send restart notification on any restart (not first start)
        if !is_first_start {
            let bot = self.bot.clone();
            let state = Arc::clone(&self.state);
            tokio::spawn(async move {
                send_restart_notification(bot, state).await;
            });
        }

        // Initialize agent orchestrator (if agents feature enabled)
        #[cfg(feature = "agents")]
        {
            if let Err(e) = self.state.init_orchestrator().await {
                warn!(error = %e, "Could not initialize orchestrator");
            }
        }

        // Cache bot identity once at startup — avoids repeated get_me() calls per message.
        match self.bot.get_me().await {
            Ok(me) => {
                info!(username = %me.username(), "Bot identity cached");
                self.state.set_bot_info(me).await;
            }
            Err(e) => {
                warn!(error = %e, "Failed to cache bot identity; falling back to 'commander'");
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
        let state_for_callbacks = Arc::clone(&state);

        let handler = dptree::entry()
            .branch(
                Update::filter_callback_query()
                    .endpoint(move |bot: Bot, q: teloxide::types::CallbackQuery| {
                        let state = Arc::clone(&state_for_callbacks);
                        async move { handle_callback(bot, q, state).await }
                    }),
            )
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

        // Build dispatcher with error handler
        Dispatcher::builder(bot, handler)
            .default_handler(|upd| async move {
                warn!("Unhandled update: {:?}", upd);
            })
            .error_handler(teloxide::error_handlers::LoggingErrorHandler::with_custom_text(
                "An error occurred in the dispatcher"
            ))
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

/// Create inline keyboard from detected options.
fn create_option_keyboard(options: &DetectedOptions) -> InlineKeyboardMarkup {
    let buttons: Vec<Vec<InlineKeyboardButton>> = options.options
        .iter()
        .map(|opt| {
            let button_text = match options.format {
                OptionFormat::Letters => format!("{}) {}", opt.key, opt.label),
                OptionFormat::Numbers => format!("{}. {}", opt.key, opt.label),
                OptionFormat::YesNo => opt.label.clone(),
            };

            // Callback data: "option:{key}"
            let callback_data = format!("option:{}", opt.key);

            vec![InlineKeyboardButton::callback(button_text, callback_data)]
        })
        .collect();

    InlineKeyboardMarkup::new(buttons)
}

/// Send a message, splitting at newline boundaries if it exceeds `max_len` chars.
///
/// Returns the `MessageId`s of all chunks sent (used for reply-routing all visible messages).
async fn send_long_message(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    text: &str,
    parse_mode: teloxide::types::ParseMode,
    thread_id: Option<teloxide::types::ThreadId>,
    reply_params: Option<teloxide::types::ReplyParameters>,
    reply_markup: Option<teloxide::types::InlineKeyboardMarkup>,
    disable_notification: bool,
    message_effect_id: Option<&str>,
    max_len: usize,
) -> Result<Vec<teloxide::types::MessageId>> {
    use teloxide::types::LinkPreviewOptions;

    let chunks = split_message(text, max_len);
    let chunk_count = chunks.len();
    let mut sent_ids = Vec::with_capacity(chunk_count);

    for (i, chunk) in chunks.into_iter().enumerate() {
        let is_last = i == chunk_count - 1;

        let mut req = bot.send_message(chat_id, chunk)
            .parse_mode(parse_mode)
            .link_preview_options(LinkPreviewOptions {
                is_disabled: true,
                url: None,
                prefer_small_media: false,
                prefer_large_media: false,
                show_above_text: false,
            });

        if let Some(tid) = thread_id {
            req = req.message_thread_id(tid);
        }

        // Reply parameters and keyboard only on the last chunk.
        if is_last {
            if let Some(ref rp) = reply_params {
                req = req.reply_parameters(rp.clone());
            }
            if let Some(ref kb) = reply_markup {
                req = req.reply_markup(kb.clone());
            }
            if let Some(effect_id) = message_effect_id {
                req = req.message_effect_id(teloxide::types::EffectId(effect_id.to_owned()));
            }
        }

        if disable_notification {
            req = req.disable_notification(true);
        }

        match req.await {
            Ok(sent) => {
                sent_ids.push(sent.id);
            }
            Err(e) => {
                warn!(chat_id = %chat_id.0, chunk = i, error = %e, "Failed to send message chunk");
            }
        }
    }

    Ok(sent_ids)
}

/// Add a reaction emoji to a message, logging warnings on failure (not fatal).
async fn add_reaction(bot: &Bot, chat_id: teloxide::types::ChatId, message_id: teloxide::types::MessageId, emoji: &str) {
    use teloxide::types::ReactionType;
    if let Err(e) = bot
        .set_message_reaction(chat_id, message_id)
        .reaction(vec![ReactionType::Emoji { emoji: emoji.to_owned() }])
        .await
    {
        warn!(chat_id = %chat_id.0, error = %e, "Failed to set message reaction (non-fatal)");
    }
}

/// Background task to poll for output from connected sessions and send responses.
async fn poll_output_loop(bot: Bot, state: Arc<TelegramState>) {
    use teloxide::types::{ChatAction, LinkPreviewOptions, MessageId, ReplyParameters};
    use std::collections::HashMap;

    let mut poll_interval = interval(Duration::from_millis(POLL_INTERVAL_MS));

    // Track progress message IDs per session key
    let mut progress_messages: HashMap<i64, MessageId> = HashMap::new();
    // Track summary message IDs per session key
    let mut summary_messages: HashMap<i64, MessageId> = HashMap::new();
    // Track last selector hash per session to avoid re-sending the same prompt every poll
    let mut last_selector_hashes: HashMap<i64, u64> = HashMap::new();
    // Track selector message IDs so we can delete them when the selector disappears
    let mut selector_messages: HashMap<i64, MessageId> = HashMap::new();

    // Shared link preview options for progress/notification messages (no previews on status msgs).
    let no_preview = LinkPreviewOptions {
        is_disabled: true,
        url: None,
        prefer_small_media: false,
        prefer_large_media: false,
        show_above_text: false,
    };

    loop {
        poll_interval.tick().await;

        // Get all sessions that are waiting for responses (includes topic sessions)
        let waiting_sessions = state.get_waiting_sessions().await;

        for (session_key, chat_id, thread_id) in waiting_sessions {
            // Refresh typing indicator to show processing is ongoing
            if let Some(tid) = thread_id {
                if let Err(e) = bot.send_chat_action(chat_id, ChatAction::Typing)
                    .message_thread_id(tid)
                    .await
                {
                    warn!(chat_id = %chat_id, thread_id = ?tid, error = %e, "Failed to send typing indicator");
                }
            } else if let Err(e) = bot.send_chat_action(chat_id, ChatAction::Typing).await {
                warn!(chat_id = %chat_id, error = %e, "Failed to send typing indicator");
            }

            // Poll for output from this session
            let poll_result = if let Some(tid) = thread_id {
                state.poll_topic_output(chat_id, tid).await
            } else {
                state.poll_output(chat_id).await
            };

            match poll_result {
                Ok(PollResult::Progress(progress_msg)) => {
                    if let Some(&msg_id) = progress_messages.get(&session_key) {
                        // Update existing progress message silently (ignore errors — may have been deleted).
                        let _ = bot.edit_message_text(chat_id, msg_id, &progress_msg).await;
                    } else {
                        // Send new progress message: silent + no link preview.
                        let mut req = bot.send_message(chat_id, &progress_msg)
                            .disable_notification(true)
                            .link_preview_options(no_preview.clone());
                        if let Some(tid) = thread_id {
                            req = req.message_thread_id(tid);
                        }
                        match req.await {
                            Ok(sent) => {
                                progress_messages.insert(session_key, sent.id);
                            }
                            Err(e) => {
                                warn!(chat_id = %chat_id.0, error = %e, "Failed to send progress message");
                            }
                        }
                    }
                }
                Ok(PollResult::IncrementalSummary(summary)) => {
                    if let Some(&msg_id) = summary_messages.get(&session_key) {
                        if let Err(e) = bot.edit_message_text(chat_id, msg_id, &summary).await {
                            warn!(chat_id = %chat_id.0, error = %e, "Failed to update incremental summary");
                        } else {
                            info!(chat_id = %chat_id.0, "Incremental summary updated");
                        }
                    } else {
                        let mut req = bot.send_message(chat_id, &summary)
                            .disable_notification(true)
                            .link_preview_options(no_preview.clone());
                        if let Some(tid) = thread_id {
                            req = req.message_thread_id(tid);
                        }
                        match req.await {
                            Ok(sent) => {
                                summary_messages.insert(session_key, sent.id);
                                info!(chat_id = %chat_id.0, "Incremental summary sent");
                            }
                            Err(e) => {
                                warn!(chat_id = %chat_id.0, error = %e, "Failed to send incremental summary");
                            }
                        }
                    }
                }
                Ok(PollResult::Summarizing) => {
                    let summarizing_msg = "🤖 Summarizing output...";
                    if let Some(&msg_id) = progress_messages.get(&session_key) {
                        let _ = bot.edit_message_text(chat_id, msg_id, summarizing_msg).await;
                    } else {
                        let mut req = bot.send_message(chat_id, summarizing_msg)
                            .disable_notification(true)
                            .link_preview_options(no_preview.clone());
                        if let Some(tid) = thread_id {
                            req = req.message_thread_id(tid);
                        }
                        match req.await {
                            Ok(sent) => {
                                progress_messages.insert(session_key, sent.id);
                            }
                            Err(e) => {
                                warn!(chat_id = %chat_id.0, error = %e, "Failed to send summarizing message");
                            }
                        }
                    }
                }
                Ok(PollResult::Complete(mut response, message_id, response_thread_id)) => {
                    // Delete progress and summary messages.
                    if let Some(prog_msg_id) = progress_messages.remove(&session_key) {
                        let _ = bot.delete_message(chat_id, prog_msg_id).await;
                    }
                    if let Some(sum_msg_id) = summary_messages.remove(&session_key) {
                        let _ = bot.delete_message(chat_id, sum_msg_id).await;
                    }
                    // Clear selector state when session completes
                    last_selector_hashes.remove(&session_key);
                    if let Some(sel_msg_id) = selector_messages.remove(&session_key) {
                        let _ = bot.delete_message(chat_id, sel_msg_id).await;
                    }

                    // Take at_session_name (if @-addressed). Used after send to record the reply map.
                    let at_session_name = state.take_at_session_name(session_key).await;

                    // Determine target thread (prefer response's thread_id).
                    let target_thread_id = response_thread_id.or(thread_id);

                    // Read session metadata needed for feature flags and reactions.
                    let (original_msg_id, is_private) =
                        state.get_session_reaction_meta(session_key).await;

                    let features = FeatureSet::for_context(None, is_private);

                    // Append deep link if response is truncated.
                    if response.contains("more characters)_") || response.contains("more lines)_") {
                        if let Some((name, _)) = state.get_session_info(chat_id).await {
                            let bot_username = state.bot_username().await;
                            let link = format!("https://t.me/{}?start=connect_{}", bot_username, name);
                            response.push_str(&format!("\n\n👉 <a href=\"{}\">Open full session</a>", link));
                        }
                    }

                    // Apply expandable blockquotes to long <pre> blocks.
                    if features.use_expandable_blockquotes {
                        response = apply_expandable_blockquotes(&response);
                    }

                    // Check for options in response.
                    let detected_options = OptionDetector::detect_options(&response);
                    let keyboard = detected_options.as_ref().map(|o| create_option_keyboard(o));

                    let reply_params = message_id.map(ReplyParameters::new);
                    let effect_id = if features.use_message_effects { Some(EFFECT_ID_CONFETTI) } else { None };

                    // Send (possibly split) final response.
                    let send_result = send_long_message(
                        &bot,
                        chat_id,
                        &response,
                        teloxide::types::ParseMode::Html,
                        target_thread_id,
                        reply_params,
                        keyboard,
                        false, // final response is not silent
                        effect_id,
                        features.max_message_length,
                    ).await;

                    match send_result {
                        Ok(sent_ids) => {
                            // If @-addressed, record ALL sent message IDs so the user can
                            // reply to any visible chunk and still be routed to the same session.
                            debug!(
                                chat_id = %chat_id.0,
                                sent_ids = ?sent_ids,
                                at_session = ?at_session_name,
                                "Attempted record_at_reply"
                            );
                            if let Some(session_name) = &at_session_name {
                                for msg_id in &sent_ids {
                                    state.record_at_reply(chat_id.0, *msg_id, session_name.clone()).await;
                                }
                            }

                            if detected_options.is_some() {
                                info!(chat_id = %chat_id.0, thread_id = ?target_thread_id, "Response with options sent to user");
                            } else {
                                info!(chat_id = %chat_id.0, thread_id = ?target_thread_id, "Response sent to user");
                            }

                            // Add success reaction to the original user message.
                            if features.use_reactions {
                                if let Some(orig_id) = original_msg_id {
                                    add_reaction(&bot, chat_id, orig_id, "👍").await;
                                }
                            }
                        }
                        Err(e) => {
                            warn!(chat_id = %chat_id.0, error = %e, "Failed to send response");
                        }
                    }
                }
                Ok(PollResult::SelectorDetected(selector)) => {
                    // Deduplicate: only send when question/options change — not on every poll.
                    use std::hash::{Hash, Hasher};
                    use std::collections::hash_map::DefaultHasher;
                    let mut hasher = DefaultHasher::new();
                    selector.question.hash(&mut hasher);
                    selector.options.hash(&mut hasher);
                    let selector_hash = hasher.finish();

                    if last_selector_hashes.get(&session_key) != Some(&selector_hash) {
                        last_selector_hashes.insert(session_key, selector_hash);

                        // Delete previous selector message if any
                        if let Some(old_msg_id) = selector_messages.remove(&session_key) {
                            let _ = bot.delete_message(chat_id, old_msg_id).await;
                        }

                        let mut text = String::new();
                        if !selector.question.is_empty() {
                            text.push_str(&format!(
                                "❓ <b>{}</b>\n\n",
                                teloxide::utils::html::escape(&selector.question)
                            ));
                        } else {
                            text.push_str("❓ <b>Choose an option:</b>\n\n");
                        }
                        for (i, opt) in selector.options.iter().enumerate() {
                            let marker = if i == selector.selected_index { "▶ " } else { "   " };
                            text.push_str(&format!(
                                "{}{}.  {}\n",
                                marker,
                                i + 1,
                                teloxide::utils::html::escape(opt)
                            ));
                        }
                        if selector.is_multi {
                            text.push_str("\n<i>Tap buttons to select, then confirm</i>");
                        } else {
                            text.push_str("\n<i>Tap a button or reply with a number to select</i>");
                        }

                        let keyboard_buttons: Vec<Vec<InlineKeyboardButton>> = selector
                            .options
                            .iter()
                            .enumerate()
                            .map(|(i, opt)| {
                                vec![InlineKeyboardButton::callback(
                                    format!("{}. {}", i + 1, opt),
                                    format!("select:{}:{}", i + 1, selector.selected_index),
                                )]
                            })
                            .collect();

                        let markup = InlineKeyboardMarkup::new(keyboard_buttons);

                        let mut req = bot
                            .send_message(chat_id, &text)
                            .parse_mode(teloxide::types::ParseMode::Html)
                            .reply_markup(markup);
                        if let Some(tid) = thread_id {
                            req = req.message_thread_id(tid);
                        }
                        match req.await {
                            Ok(sent) => { selector_messages.insert(session_key, sent.id); }
                            Err(e) => { warn!(chat_id = %chat_id.0, error = %e, "Failed to send selector message"); }
                        }
                    }
                }
                Ok(PollResult::NoOutput) => {
                    // No response ready yet, continue polling.
                }
                Err(e) => {
                    warn!(chat_id = %chat_id.0, error = %e, "Error polling output");

                    // Clean up progress message on error.
                    if let Some(prog_msg_id) = progress_messages.remove(&session_key) {
                        let _ = bot.delete_message(chat_id, prog_msg_id).await;
                    }
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
            // Build notification message with deep link if session is specified
            let mut message = notification.message.clone();
            if let Some(session) = &notification.session {
                let display_name = session.strip_prefix("commander-").unwrap_or(session);
                // Generate deep link for connecting to this session (uses cached identity).
                let bot_username = state.bot_username().await;
                let link = format!("https://t.me/{}?start=connect_{}", bot_username, display_name);

                // Choose link text based on notification context
                let link_text = if message.contains("resumed work") || message.contains("resumed") {
                    format!("Resume {}", display_name)
                } else if message.contains("paused") || message.contains("waiting") {
                    format!("Continue {}", display_name)
                } else if message.contains("ready") || message.contains("started") {
                    format!("Open {}", display_name)
                } else {
                    // Default for unknown contexts
                    format!("Connect to {}", display_name)
                };

                message.push_str(&format!("\n\n👉 <a href=\"{}\">{}</a>", link, link_text));
            }

            for &chat_id in &authorized_chats {
                // Skip notification if it's for the session the user is currently connected to
                if let Some(ref notification_session) = notification.session {
                    if let Some(current_session) = state.get_current_tmux_session(chat_id).await {
                        if &current_session == notification_session {
                            debug!(
                                chat_id = %chat_id,
                                session = %notification_session,
                                "Skipping notification - user already connected to this session"
                            );
                            continue;
                        }
                    }
                }

                let req = bot.send_message(ChatId(chat_id), &message)
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .link_preview_options(teloxide::types::LinkPreviewOptions {
                        is_disabled: true,
                        url: None,
                        prefer_small_media: false,
                        prefer_large_media: false,
                        show_above_text: false,
                    });
                if let Err(e) = req.await {
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

/// Send per-session restart notification to each restored session's user.
async fn send_restart_notification(bot: Bot, state: Arc<TelegramState>) {
    use teloxide::types::{ChatId, ParseMode, ThreadId};

    let sessions = state.get_session_summaries().await;
    for (chat_id, project_name, thread_id) in sessions {
        let msg = format!(
            "🔄 Bot restarted — reconnected to <b>{}</b>",
            teloxide::utils::html::escape(&project_name)
        );
        let mut req = bot
            .send_message(ChatId(chat_id), &msg)
            .parse_mode(ParseMode::Html);
        if let Some(tid) = thread_id {
            req = req.message_thread_id(ThreadId(teloxide::types::MessageId(tid)));
        }
        if let Err(e) = req.await {
            warn!(chat_id = %chat_id, error = %e, "Failed to send restart notification");
        } else {
            debug!(chat_id = %chat_id, project = %project_name, "Restart notification sent");
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests would go here but require mocking the Telegram API
}
