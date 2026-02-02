//! Command handlers for the Telegram bot.

use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;
use tracing::{debug, error, info};

use crate::error::TelegramError;
use crate::state::TelegramState;

/// Bot commands that can be invoked with /.
#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
pub enum Command {
    #[command(description = "Start the bot and get help")]
    Start,

    #[command(description = "Show help message")]
    Help,

    #[command(description = "Pair with CLI using code: /pair <CODE>")]
    Pair(String),

    #[command(description = "Connect to a project: /connect <name> or /connect <path> -a <adapter> -n <name>")]
    Connect(String),

    #[command(description = "Attach to tmux session: /session <session_name>")]
    Session(String),

    #[command(description = "List tmux sessions")]
    Sessions,

    #[command(description = "Disconnect from current project")]
    Disconnect,

    #[command(description = "Show current connection status")]
    Status,

    #[command(description = "List available projects")]
    List,
}

/// Handle the /start command.
pub async fn handle_start(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let welcome = format!(
        "Welcome to Commander Bot! 🚀\n\n\
        I can help you interact with Claude Code sessions from anywhere.\n\n\
        <b>Getting Started:</b>\n\
        1. Use /list to see available projects\n\
        2. Use /connect &lt;project&gt; to connect\n\
        3. Send messages to interact with Claude Code\n\
        4. Use /disconnect when done\n\n\
        <b>Status:</b>\n\
        - tmux: {}\n\
        - Summarization: {}\n\n\
        Type /help for all commands.",
        if state.has_tmux() { "✅ available" } else { "❌ not available" },
        if state.has_summarization() { "✅ enabled" } else { "⚠️ disabled (set OPENROUTER_API_KEY)" }
    );

    bot.send_message(msg.chat.id, welcome)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    info!(chat_id = %msg.chat.id, user = ?msg.from.as_ref().map(|u| &u.username), "User started bot");
    Ok(())
}

/// Handle the /help command.
pub async fn handle_help(bot: Bot, msg: Message) -> ResponseResult<()> {
    let help_text = Command::descriptions().to_string();
    bot.send_message(msg.chat.id, help_text).await?;
    Ok(())
}

/// Handle the /pair command - validate pairing code and connect.
pub async fn handle_pair(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    code: String,
) -> ResponseResult<()> {
    let code = code.trim().to_uppercase();

    if code.is_empty() {
        bot.send_message(
            msg.chat.id,
            "Please provide a pairing code.\n\n\
            <b>Usage:</b> <code>/pair CODE</code>\n\n\
            Get a code by running <code>/telegram</code> in the Commander CLI.",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    if code.len() != 6 {
        bot.send_message(
            msg.chat.id,
            "Invalid code format. Pairing codes are 6 characters.\n\n\
            Get a code by running <code>/telegram</code> in the Commander CLI.",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    let chat_id = msg.chat.id.0;

    match state.validate_pairing(&code, chat_id).await {
        Ok((project_name, _session_name)) => {
            // If project_name is empty, just authorize without auto-connect
            if project_name.is_empty() {
                bot.send_message(
                    msg.chat.id,
                    "Paired successfully!\n\n\
                    You are now authorized for this Commander instance.\n\
                    Use <code>/list</code> to see projects or <code>/connect &lt;name&gt;</code> to connect.",
                )
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
                info!(
                    chat_id = %chat_id,
                    "User paired (no auto-connect)"
                );
            } else {
                // Auto-connect to the session
                match state.connect_session(msg.chat.id, &project_name).await {
                    Ok(_) => {
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "Paired and connected to <b>{}</b>!\n\n\
                                You can now send messages to this session.",
                                project_name
                            ),
                        )
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                        info!(
                            chat_id = %chat_id,
                            project = %project_name,
                            "User paired and connected via code"
                        );
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "Paired successfully but connection failed: {}\n\n\
                                Use <code>/connect {}</code> to connect manually.",
                                e, project_name
                            ),
                        )
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                    }
                }
            }
        }
        Err(TelegramError::InvalidPairingCode) => {
            bot.send_message(
                msg.chat.id,
                "Invalid pairing code. Please check and try again.\n\n\
                Get a new code by running <code>/telegram</code> in the Commander CLI.",
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        }
        Err(TelegramError::PairingExpired) => {
            bot.send_message(
                msg.chat.id,
                "Pairing code expired. Codes are valid for 5 minutes.\n\n\
                Generate a new one with <code>/telegram</code> in the CLI.",
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("Error: {}", e))
                .await?;
        }
    }

    Ok(())
}

/// Parsed connect command arguments.
#[derive(Debug)]
enum ConnectArgs {
    /// Connect to existing project by name
    Existing(String),
    /// Create and connect to new project
    New { path: String, adapter: String, name: String },
}

/// Parse connect command arguments.
fn parse_connect_args(arg: &str) -> Result<ConnectArgs, String> {
    let parts: Vec<&str> = arg.split_whitespace().collect();

    if parts.is_empty() {
        return Err("connect requires arguments".to_string());
    }

    // Check if this has -a or -n flags (new project syntax)
    if parts.iter().any(|&p| p == "-a" || p == "-n") {
        let path = shellexpand::tilde(parts[0]).to_string();
        let mut adapter = None;
        let mut name = None;

        let mut i = 1;
        while i < parts.len() {
            match parts[i] {
                "-a" => {
                    if i + 1 < parts.len() {
                        adapter = Some(parts[i + 1].to_string());
                        i += 2;
                    } else {
                        return Err("-a requires an adapter (cc, mpm)".to_string());
                    }
                }
                "-n" => {
                    if i + 1 < parts.len() {
                        name = Some(parts[i + 1].to_string());
                        i += 2;
                    } else {
                        return Err("-n requires a project name".to_string());
                    }
                }
                _ => {
                    return Err(format!("unknown flag: {}", parts[i]));
                }
            }
        }

        match (adapter, name) {
            (Some(a), Some(n)) => Ok(ConnectArgs::New { path, adapter: a, name: n }),
            (None, _) => Err("missing -a <adapter> (cc, mpm)".to_string()),
            (_, None) => Err("missing -n <name>".to_string()),
        }
    } else if parts.len() == 1 {
        // Existing project by name
        Ok(ConnectArgs::Existing(parts[0].to_string()))
    } else {
        Err("use '/connect <name>' or '/connect <path> -a <adapter> -n <name>'".to_string())
    }
}

/// Handle the /connect command.
pub async fn handle_connect(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    args: String,
) -> ResponseResult<()> {
    // Check authorization first
    if !state.is_authorized(msg.chat.id.0).await {
        bot.send_message(
            msg.chat.id,
            "Not authorized. Use /pair <code> first.\n\n\
            Get a pairing code by running <code>/telegram</code> in the Commander CLI.",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    let args = args.trim();

    if args.is_empty() {
        bot.send_message(
            msg.chat.id,
            "Please specify a project.\n\n\
            <b>Connect to existing project:</b>\n<code>/connect &lt;name&gt;</code>\n\n\
            <b>Create new project:</b>\n<code>/connect &lt;path&gt; -a &lt;adapter&gt; -n &lt;name&gt;</code>\n\n\
            <b>Attach to tmux session:</b>\nIf <code>-n</code> matches an existing tmux session, attaches to it.\n\n\
            Adapters: <code>cc</code> (Claude Code), <code>mpm</code>\n\n\
            Use /list for projects, /sessions for tmux sessions.",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    // Parse the connect arguments
    let connect_args = match parse_connect_args(args) {
        Ok(args) => args,
        Err(e) => {
            bot.send_message(msg.chat.id, format!("❌ {}", e))
                .await?;
            return Ok(());
        }
    };

    // Check if already connected and disconnect first
    if let Some((current_project, _)) = state.get_session_info(msg.chat.id).await {
        let target_name = match &connect_args {
            ConnectArgs::Existing(name) => name.clone(),
            ConnectArgs::New { name, .. } => name.clone(),
        };
        if current_project == target_name {
            bot.send_message(
                msg.chat.id,
                format!("Already connected to <b>{}</b>", current_project),
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
            return Ok(());
        }
        let _ = state.disconnect(msg.chat.id).await;
    }

    match connect_args {
        ConnectArgs::Existing(project_name) => {
            bot.send_message(msg.chat.id, format!("Connecting to {}...", project_name))
                .await?;

            match state.connect(msg.chat.id, &project_name).await {
                Ok(connected_name) => {
                    bot.send_message(
                        msg.chat.id,
                        format!("✅ Connected to <b>{}</b>\n\nYou can now send messages to interact with Claude Code.", connected_name),
                    )
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await?;
                    info!(chat_id = %msg.chat.id, project = %connected_name, "User connected to project");
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("❌ Failed to connect: {}", e))
                        .await?;
                    error!(chat_id = %msg.chat.id, error = %e, "Connection failed");
                }
            }
        }
        ConnectArgs::New { path, adapter, name } => {
            // Check if name matches an existing tmux session - if so, attach to it instead
            let sessions = state.list_tmux_sessions();
            if sessions.iter().any(|(s, _)| s == &name) {
                bot.send_message(msg.chat.id, format!("Found existing session '{}', attaching...", name))
                    .await?;

                match state.attach_session(msg.chat.id, &name).await {
                    Ok(attached_name) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("✅ Attached to existing session <code>{}</code>\n\nYou can now send messages.", attached_name),
                        )
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                        info!(chat_id = %msg.chat.id, session = %attached_name, "User attached to existing session");
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, format!("❌ Failed to attach: {}", e))
                            .await?;
                        error!(chat_id = %msg.chat.id, error = %e, "Session attach failed");
                    }
                }
            } else {
                bot.send_message(msg.chat.id, format!("Creating project {} at {}...", name, path))
                    .await?;

                match state.connect_new(msg.chat.id, &path, &adapter, &name).await {
                    Ok(connected_name) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("✅ Created and connected to <b>{}</b>\n\nYou can now send messages to interact with Claude Code.", connected_name),
                        )
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                        info!(chat_id = %msg.chat.id, project = %connected_name, path = %path, "User created and connected to project");
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, format!("❌ Failed to create project: {}", e))
                            .await?;
                        error!(chat_id = %msg.chat.id, error = %e, "Project creation failed");
                    }
                }
            }
        }
    }

    Ok(())
}

/// Handle the /disconnect command.
pub async fn handle_disconnect(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    match state.disconnect(msg.chat.id).await {
        Ok(Some(project_name)) => {
            bot.send_message(
                msg.chat.id,
                format!("Disconnected from <b>{}</b>", project_name),
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
            info!(chat_id = %msg.chat.id, project = %project_name, "User disconnected");
        }
        Ok(None) => {
            bot.send_message(msg.chat.id, "Not connected to any project.")
                .await?;
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("❌ Error: {}", e))
                .await?;
        }
    }

    Ok(())
}

/// Handle the /status command.
pub async fn handle_status(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let status = if let Some((project_name, project_path)) = state.get_session_info(msg.chat.id).await
    {
        format!(
            "<b>Status: Connected</b>\n\n\
            📁 Project: {}\n\
            📍 Path: <code>{}</code>",
            project_name, project_path
        )
    } else {
        "<b>Status: Not connected</b>\n\nUse /connect &lt;project&gt; to connect to a project.".to_string()
    };

    bot.send_message(msg.chat.id, status)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    Ok(())
}

/// Handle the /list command.
pub async fn handle_list(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let projects = state.list_projects();

    if projects.is_empty() {
        bot.send_message(
            msg.chat.id,
            "No projects found.\n\nCreate a project using the Commander CLI first.",
        )
        .await?;
        return Ok(());
    }

    let current_project = state
        .get_session_info(msg.chat.id)
        .await
        .map(|(name, _)| name);

    let mut text = String::from("<b>Available Projects:</b>\n\n");
    for (name, path) in projects {
        let marker = if current_project.as_ref() == Some(&name) {
            "✅"
        } else {
            "📁"
        };
        text.push_str(&format!("{} <b>{}</b>\n   <code>{}</code>\n\n", marker, name, path));
    }
    text.push_str("Use <code>/connect &lt;name&gt;</code> to connect");

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    Ok(())
}

/// Handle regular text messages (forward to Claude Code).
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let Some(text) = msg.text() else {
        return Ok(());
    };

    // Check if connected
    if !state.has_session(msg.chat.id).await {
        bot.send_message(
            msg.chat.id,
            "Not connected to any project.\n\nUse /connect <project> to connect first.",
        )
        .await?;
        return Ok(());
    }

    // Send typing indicator
    bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
        .await?;

    // Send message to the project
    match state.send_message(msg.chat.id, text).await {
        Ok(()) => {
            debug!(chat_id = %msg.chat.id, message = %text, "Message sent to project");
            // Response will be polled and sent back by the polling task
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("❌ Error: {}", e))
                .await?;
            error!(chat_id = %msg.chat.id, error = %e, "Failed to send message");
        }
    }

    Ok(())
}

/// Handle the /sessions command - list tmux sessions.
pub async fn handle_sessions(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let sessions = state.list_tmux_sessions();

    if sessions.is_empty() {
        bot.send_message(msg.chat.id, "No tmux sessions found.")
            .await?;
        return Ok(());
    }

    let current_session = state
        .get_session_info(msg.chat.id)
        .await
        .map(|(_, path)| format!("commander-{}", path.split('/').last().unwrap_or("")));

    let mut text = String::from("<b>Tmux Sessions:</b>\n\n");
    for (name, is_commander) in sessions {
        let marker = if current_session.as_ref().map(|s| s == &name).unwrap_or(false) {
            "✅"
        } else if is_commander {
            "🤖"
        } else {
            "📟"
        };
        text.push_str(&format!("{} <code>{}</code>\n", marker, name));
    }
    text.push_str("\nUse <code>/session &lt;name&gt;</code> to attach");

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    Ok(())
}

/// Handle the /session command - attach to a tmux session.
pub async fn handle_session(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    session_name: String,
) -> ResponseResult<()> {
    // Check authorization first
    if !state.is_authorized(msg.chat.id.0).await {
        bot.send_message(
            msg.chat.id,
            "Not authorized. Use /pair <code> first.\n\n\
            Get a pairing code by running <code>/telegram</code> in the Commander CLI.",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    let session_name = session_name.trim();

    if session_name.is_empty() {
        bot.send_message(
            msg.chat.id,
            "Please specify a session name.\n\nUsage: <code>/session &lt;name&gt;</code>\n\nUse /sessions to list available sessions.",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    // Disconnect from current if connected
    if state.has_session(msg.chat.id).await {
        let _ = state.disconnect(msg.chat.id).await;
    }

    bot.send_message(msg.chat.id, format!("Attaching to session {}...", session_name))
        .await?;

    match state.attach_session(msg.chat.id, session_name).await {
        Ok(attached_name) => {
            bot.send_message(
                msg.chat.id,
                format!("✅ Attached to <code>{}</code>\n\nYou can now send messages.", attached_name),
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
            info!(chat_id = %msg.chat.id, session = %attached_name, "User attached to session");
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("❌ Failed to attach: {}", e))
                .await?;
            error!(chat_id = %msg.chat.id, error = %e, "Session attach failed");
        }
    }

    Ok(())
}

/// Dispatch commands to appropriate handlers.
pub async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    match cmd {
        Command::Start => handle_start(bot, msg, state).await,
        Command::Help => handle_help(bot, msg).await,
        Command::Pair(code) => handle_pair(bot, msg, state, code).await,
        Command::Connect(project) => handle_connect(bot, msg, state, project).await,
        Command::Session(session) => handle_session(bot, msg, state, session).await,
        Command::Sessions => handle_sessions(bot, msg, state).await,
        Command::Disconnect => handle_disconnect(bot, msg, state).await,
        Command::Status => handle_status(bot, msg, state).await,
        Command::List => handle_list(bot, msg, state).await,
    }
}
