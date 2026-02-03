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

    #[command(description = "Stop session (commits changes, ends tmux): /stop [session]")]
    Stop(String),

    #[command(description = "Send message to session (for messages starting with /): /send <message>")]
    Send(String),

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
        I can help you interact with AI coding sessions from anywhere.\n\n\
        <b>Getting Started:</b>\n\
        1. Use /list to see available projects\n\
        2. Use /connect &lt;project&gt; to connect\n\
        3. Send messages to interact with your session\n\
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
                    Ok((connected_name, tool_id)) => {
                        let adapter_name = adapter_display_name(&tool_id);
                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "Paired and connected to <b>{}</b>!\n\n\
                                You can now send messages to interact with {}.",
                                connected_name, adapter_name
                            ),
                        )
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                        info!(
                            chat_id = %chat_id,
                            project = %connected_name,
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
                Ok((connected_name, tool_id)) => {
                    let adapter_name = adapter_display_name(&tool_id);
                    bot.send_message(
                        msg.chat.id,
                        format!("✅ Connected to <b>{}</b>\n\nYou can now send messages to interact with {}.", connected_name, adapter_name),
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
                    Ok((connected_name, tool_id)) => {
                        let adapter_name = adapter_display_name(&tool_id);
                        bot.send_message(
                            msg.chat.id,
                            format!("✅ Created and connected to <b>{}</b>\n\nYou can now send messages to interact with {}.", connected_name, adapter_name),
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

/// Map tool_id to display name.
fn adapter_display_name(tool_id: &str) -> &str {
    match tool_id {
        "claude-code" | "cc" => "Claude Code",
        "mpm" => "Claude MPM",
        "aider" => "Aider",
        _ => tool_id,
    }
}

/// Handle the /status command.
pub async fn handle_status(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let status = if let Some((project_name, project_path, tool_id, is_waiting, pending_query, screen_preview)) =
        state.get_session_status(msg.chat.id).await
    {
        let adapter_name = adapter_display_name(&tool_id);

        // Build activity section
        let activity = if is_waiting {
            if let Some(query) = pending_query {
                // Truncate long queries
                let truncated = if query.len() > 50 {
                    format!("{}...", &query[..47])
                } else {
                    query
                };
                format!(
                    "🔄 Activity: Processing command...\n📝 Query: \"{}\"",
                    html_escape(&truncated)
                )
            } else {
                "🔄 Activity: Processing...".to_string()
            }
        } else {
            "💤 Activity: Idle (ready for commands)".to_string()
        };

        // Build screen preview section
        let screen_section = if let Some(preview) = screen_preview {
            if preview.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\n📺 Screen:\n<pre>{}</pre>",
                    html_escape(&preview)
                )
            }
        } else {
            String::new()
        };

        format!(
            "📊 <b>Status</b>\n\n\
            ✅ Connection: Connected\n\
            📁 Project: {}\n\
            📍 Path: <code>{}</code>\n\
            🔧 Adapter: {}\n\n\
            {}{}",
            html_escape(&project_name),
            html_escape(&project_path),
            adapter_name,
            activity,
            screen_section
        )
    } else {
        "📊 <b>Status</b>\n\n❌ Connection: Not connected\n\nUse /connect &lt;project&gt; to connect to a project.".to_string()
    };

    bot.send_message(msg.chat.id, status)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    Ok(())
}

/// Escape HTML special characters for Telegram HTML mode.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Handle the /list command.
pub async fn handle_list(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let projects = state.list_projects();
    let tmux_sessions = state.list_tmux_sessions();

    if projects.is_empty() && tmux_sessions.is_empty() {
        bot.send_message(
            msg.chat.id,
            "No projects or sessions found.\n\nCreate a project using the Commander CLI or start a tmux session.",
        )
        .await?;
        return Ok(());
    }

    let current_project = state
        .get_session_info(msg.chat.id)
        .await
        .map(|(name, _)| name);

    let mut text = String::new();

    // Show projects if any
    if !projects.is_empty() {
        text.push_str("<b>📁 Projects:</b>\n\n");
        for (name, path) in &projects {
            let marker = if current_project.as_ref() == Some(name) {
                "✅"
            } else {
                "📁"
            };
            text.push_str(&format!("{} <b>{}</b>\n   <code>{}</code>\n\n", marker, name, path));
        }
    }

    // Show tmux sessions if any
    if !tmux_sessions.is_empty() {
        if !text.is_empty() {
            text.push_str("\n");
        }
        text.push_str("<b>📟 Tmux Sessions:</b>\n\n");

        // Get current session for highlighting
        let current_session = state
            .get_session_info(msg.chat.id)
            .await
            .map(|(_, path)| format!("commander-{}", path.rsplit('/').next().unwrap_or("")));

        for (name, is_commander) in &tmux_sessions {
            let marker = if current_session.as_ref().map(|s| s == name).unwrap_or(false) {
                "✅"
            } else if *is_commander {
                "🤖"
            } else {
                "📟"
            };
            text.push_str(&format!("{} <code>{}</code>\n", marker, name));
        }
    }

    // Add usage hints
    text.push_str("\n<b>Commands:</b>\n");
    if !projects.is_empty() {
        text.push_str("• <code>/connect &lt;name&gt;</code> - Connect to project\n");
    }
    if !tmux_sessions.is_empty() {
        text.push_str("• <code>/session &lt;name&gt;</code> - Attach to tmux session\n");
    }

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

    // Check for @alias prefix to route to specific project
    if let Some(rest) = text.strip_prefix('@') {
        if let Some((alias, message)) = rest.split_once(' ') {
            let alias = alias.trim();
            let message = message.trim();

            if !alias.is_empty() && !message.is_empty() {
                // Try to connect to the specified project
                match state.connect(msg.chat.id, alias).await {
                    Ok((project_name, _tool_id)) => {
                        // Successfully connected, now send the message
                        bot.send_message(msg.chat.id, format!("➡️ Routing to {}", project_name))
                            .await?;

                        // Send the actual message
                        match state.send_message(msg.chat.id, message, Some(msg.id)).await {
                            Ok(()) => {
                                debug!(
                                    chat_id = %msg.chat.id,
                                    alias = %alias,
                                    message = %message,
                                    "Message routed via @alias"
                                );
                                return Ok(());
                            }
                            Err(e) => {
                                bot.send_message(msg.chat.id, format!("❌ Failed to send: {}", e))
                                    .await?;
                                return Ok(());
                            }
                        }
                    }
                    Err(e) => {
                        bot.send_message(
                            msg.chat.id,
                            format!("❌ Could not connect to '{}': {}", alias, e),
                        )
                        .await?;
                        return Ok(());
                    }
                }
            }
        }
    }

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

    // Send message to the project with message ID for reply threading
    match state.send_message(msg.chat.id, text, Some(msg.id)).await {
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
        .map(|(_, path)| format!("commander-{}", path.rsplit('/').next().unwrap_or("")));

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

/// Handle the /stop command - stop a session with optional git commit.
pub async fn handle_stop(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    session_arg: String,
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

    let session_arg = session_arg.trim();

    // Determine which session to stop
    let (session_name, project_path, is_connected_session) = if session_arg.is_empty() {
        // Use connected session
        match state.get_session_info(msg.chat.id).await {
            Some((name, path)) => {
                let tmux_session = format!("commander-{}", name);
                (tmux_session, path, true)
            }
            None => {
                bot.send_message(
                    msg.chat.id,
                    "Not connected to any session.\n\n\
                    <b>Usage:</b> <code>/stop [session]</code>\n\n\
                    Use /sessions to list available sessions.",
                )
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
                return Ok(());
            }
        }
    } else {
        // Use specified session
        let tmux_session = if session_arg.starts_with("commander-") {
            session_arg.to_string()
        } else {
            format!("commander-{}", session_arg)
        };

        // Try to find project path from store
        let project_path = state
            .store()
            .load_all_projects()
            .ok()
            .and_then(|projects| {
                let search_name = session_arg.strip_prefix("commander-").unwrap_or(session_arg);
                projects
                    .values()
                    .find(|p| p.name == search_name)
                    .map(|p| p.path.clone())
            })
            .unwrap_or_else(|| "unknown".to_string());

        // Check if this is the connected session
        let is_connected = state
            .get_session_info(msg.chat.id)
            .await
            .map(|(name, _)| format!("commander-{}", name) == tmux_session)
            .unwrap_or(false);

        (tmux_session, project_path, is_connected)
    };

    // Check if tmux session exists
    let tmux = match state.tmux() {
        Some(t) => t,
        None => {
            bot.send_message(msg.chat.id, "tmux not available.")
                .await?;
            return Ok(());
        }
    };

    if !tmux.session_exists(&session_name) {
        bot.send_message(
            msg.chat.id,
            format!(
                "Session '{}' not found.\n\nUse /sessions to list available sessions.",
                session_name
            ),
        )
        .await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, format!("Stopping session {}...", session_name))
        .await?;

    // Check for git changes and commit if needed
    let mut commit_message = None;
    if project_path != "unknown" && std::path::Path::new(&project_path).exists() {
        match check_and_commit_changes(&project_path, &session_name).await {
            Ok(Some(msg)) => commit_message = Some(msg),
            Ok(None) => {} // No changes to commit
            Err(e) => {
                info!(error = %e, "Git commit check failed (non-fatal)");
            }
        }
    }

    // Destroy the tmux session
    if let Err(e) = tmux.destroy_session(&session_name) {
        bot.send_message(msg.chat.id, format!("Failed to destroy session: {}", e))
            .await?;
        return Ok(());
    }

    // Disconnect if this was the connected session
    if is_connected_session {
        let _ = state.disconnect(msg.chat.id).await;
    }

    // Build response
    let response = if let Some(commit_msg) = commit_message {
        format!(
            "Session <code>{}</code> stopped.\n\n\
            Git changes committed:\n<pre>{}</pre>",
            html_escape(&session_name),
            html_escape(&commit_msg)
        )
    } else {
        format!(
            "Session <code>{}</code> stopped.\n\n\
            No uncommitted changes found.",
            html_escape(&session_name)
        )
    };

    bot.send_message(msg.chat.id, response)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    info!(
        chat_id = %msg.chat.id,
        session = %session_name,
        "Session stopped"
    );

    Ok(())
}

/// Check for git changes and commit them if present.
/// Returns the commit message if a commit was made, None if no changes.
async fn check_and_commit_changes(
    project_path: &str,
    session_name: &str,
) -> std::result::Result<Option<String>, String> {
    use std::process::Command;

    // Check for uncommitted changes
    let status_output = Command::new("git")
        .args(["-C", project_path, "status", "--porcelain"])
        .output()
        .map_err(|e| format!("Failed to run git status: {}", e))?;

    if !status_output.status.success() {
        return Err("git status failed (not a git repo?)".to_string());
    }

    let changes = String::from_utf8_lossy(&status_output.stdout);
    if changes.trim().is_empty() {
        return Ok(None); // No changes
    }

    // Stage all changes
    let add_output = Command::new("git")
        .args(["-C", project_path, "add", "-A"])
        .output()
        .map_err(|e| format!("Failed to run git add: {}", e))?;

    if !add_output.status.success() {
        return Err(format!(
            "git add failed: {}",
            String::from_utf8_lossy(&add_output.stderr)
        ));
    }

    // Create commit message
    let friendly_name = session_name
        .strip_prefix("commander-")
        .unwrap_or(session_name);
    let commit_msg = format!("WIP: Auto-commit from Commander session '{}'", friendly_name);

    // Commit
    let commit_output = Command::new("git")
        .args(["-C", project_path, "commit", "-m", &commit_msg])
        .output()
        .map_err(|e| format!("Failed to run git commit: {}", e))?;

    if !commit_output.status.success() {
        let stderr = String::from_utf8_lossy(&commit_output.stderr);
        // "nothing to commit" is not really an error
        if stderr.contains("nothing to commit") {
            return Ok(None);
        }
        return Err(format!("git commit failed: {}", stderr));
    }

    Ok(Some(commit_msg))
}

/// Handle the /send command - explicitly send a message to the session.
pub async fn handle_send(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    message: String,
) -> ResponseResult<()> {
    let message = message.trim();

    if message.is_empty() {
        bot.send_message(
            msg.chat.id,
            "Please provide a message to send.\n\n\
            <b>Usage:</b> <code>/send &lt;message&gt;</code>\n\n\
            Use this to send messages that start with / to the session\n\
            (e.g., <code>/send /help</code> sends \"/help\" to Claude Code).",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

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

    // Send the message
    match state.send_message(msg.chat.id, message, Some(msg.id)).await {
        Ok(()) => {
            debug!(chat_id = %msg.chat.id, message = %message, "Message sent via /send");
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("Failed to send: {}", e))
                .await?;
            error!(chat_id = %msg.chat.id, error = %e, "Failed to send message via /send");
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
        Command::Stop(session) => handle_stop(bot, msg, state, session).await,
        Command::Send(message) => handle_send(bot, msg, state, message).await,
        Command::Status => handle_status(bot, msg, state).await,
        Command::List => handle_list(bot, msg, state).await,
    }
}
