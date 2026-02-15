//! Command handlers for the Telegram bot.

use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, ThreadId};
use teloxide::utils::command::BotCommands;
use tracing::{debug, error, info, warn};

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

    #[command(description = "Connect to project, tmux session, or create new: /connect <name> or /connect <path> -a <adapter> -n <name>")]
    Connect(String),

    #[command(description = "List tmux sessions")]
    Sessions,

    #[command(description = "Disconnect from current project")]
    Disconnect,

    #[command(description = "Stop session (commits changes, ends tmux): /stop [session]")]
    Stop(String),

    #[command(description = "Send message directly to session (bypasses AI interpretation): /send <message>")]
    Send(String),

    #[command(description = "Show current connection status")]
    Status,

    #[command(description = "List available projects (alias for /sessions)")]
    List,

    #[command(description = "Enable group mode for this supergroup")]
    GroupMode,

    #[command(description = "Create topic for session: /topic <session>")]
    Topic(String),

    #[command(description = "List topics and their sessions")]
    Topics,
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
        if let Err(e) = bot
            .send_message(
                msg.chat.id,
                "Not authorized. Use <code>/pair &lt;code&gt;</code> first.\n\n\
                Get a pairing code by running <code>/telegram</code> in the Commander CLI.",
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await
        {
            error!(chat_id = %msg.chat.id, error = %e, "Failed to send authorization error message");
        }
        return Ok(());
    }

    let args = args.trim();

    if args.is_empty() {
        bot.send_message(
            msg.chat.id,
            "Please specify a target.\n\n\
            <b>Connect to registered project:</b>\n<code>/connect &lt;name&gt;</code>\n\n\
            <b>Connect to tmux session:</b>\n<code>/connect &lt;session-name&gt;</code>\n\n\
            <b>Create new project:</b>\n<code>/connect &lt;path&gt; -a &lt;adapter&gt; -n &lt;name&gt;</code>\n\n\
            The command automatically detects whether the name refers to a registered project or an existing tmux session.\n\n\
            Adapters: <code>cc</code> (Claude Code), <code>mpm</code>\n\n\
            Use /list for projects and /sessions for tmux sessions.",
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
            let _ = bot
                .send_message(msg.chat.id, format!("Connecting to {}...", project_name))
                .await;

            match state.connect(msg.chat.id, &project_name).await {
                Ok((connected_name, tool_id)) => {
                    let adapter_name = adapter_display_name(&tool_id);

                    // Get status info to include in connection message
                    let status_info = get_connection_status(&state, msg.chat.id, &connected_name).await;

                    bot.send_message(
                        msg.chat.id,
                        format!(
                            "✅ Connected to <b>{}</b>\n\n\
                            📊 Status:{}\n\n\
                            You can now send messages to interact with {}.",
                            connected_name,
                            status_info,
                            adapter_name
                        ),
                    )
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await?;
                    info!(chat_id = %msg.chat.id, project = %connected_name, "User connected to project");
                }
                Err(e) => {
                    if let Err(send_err) = bot
                        .send_message(msg.chat.id, format!("❌ Failed to connect: {}", e))
                        .await
                    {
                        error!(chat_id = %msg.chat.id, send_error = %send_err, "Failed to send connection error message");
                    }
                    error!(chat_id = %msg.chat.id, error = %e, "Connection failed");
                }
            }
        }
        ConnectArgs::New { path, adapter, name } => {
            // Check if name matches an existing tmux session - if so, use connect() which handles fallback
            let sessions = state.list_tmux_sessions();
            if sessions.iter().any(|(s, _)| s == &name) {
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        format!("Found existing session '{}', connecting...", name),
                    )
                    .await;

                match state.connect(msg.chat.id, &name).await {
                    Ok((connected_name, tool_id)) => {
                        let adapter_name = adapter_display_name(&tool_id);

                        // Get status info to include in connection message
                        let status_info = get_connection_status(&state, msg.chat.id, &connected_name).await;

                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "✅ Connected to existing session <b>{}</b>\n\n\
                                📊 Status:{}\n\n\
                                You can now send messages to interact with {}.",
                                connected_name,
                                status_info,
                                adapter_name
                            ),
                        )
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                        info!(chat_id = %msg.chat.id, session = %name, "User connected to existing session");
                    }
                    Err(e) => {
                        if let Err(send_err) = bot
                            .send_message(msg.chat.id, format!("❌ Failed to connect: {}", e))
                            .await
                        {
                            error!(chat_id = %msg.chat.id, send_error = %send_err, "Failed to send connection error message");
                        }
                        error!(chat_id = %msg.chat.id, error = %e, "Session connection failed");
                    }
                }
            } else {
                let _ = bot
                    .send_message(msg.chat.id, format!("Creating project {} at {}...", name, path))
                    .await;

                match state.connect_new(msg.chat.id, &path, &adapter, &name).await {
                    Ok((connected_name, tool_id)) => {
                        let adapter_name = adapter_display_name(&tool_id);

                        // Get status info to include in connection message
                        let status_info = get_connection_status(&state, msg.chat.id, &connected_name).await;

                        bot.send_message(
                            msg.chat.id,
                            format!(
                                "✅ Created and connected to <b>{}</b>\n\n\
                                📊 Status:{}\n\n\
                                You can now send messages to interact with {}.",
                                connected_name,
                                status_info,
                                adapter_name
                            ),
                        )
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                        info!(chat_id = %msg.chat.id, project = %connected_name, path = %path, "User created and connected to project");
                    }
                    Err(e) => {
                        if let Err(send_err) = bot
                            .send_message(msg.chat.id, format!("❌ Failed to create project: {}", e))
                            .await
                        {
                            error!(chat_id = %msg.chat.id, send_error = %send_err, "Failed to send project creation error message");
                        }
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
        "unknown" => "this session",
        _ => tool_id,
    }
}

/// Get concise status info for connection success message.
/// Returns a formatted string with git branch, state, and last activity.
async fn get_connection_status(state: &Arc<TelegramState>, chat_id: ChatId, _connected_name: &str) -> String {
    // Get detailed session status
    let status = match state.get_session_status(chat_id).await {
        Some((_, _, _, is_waiting, _, screen_preview)) => {
            // Determine state
            let state_emoji = if is_waiting {
                "🔄 running"
            } else {
                "💤 idle"
            };

            // Try to extract git branch from screen preview
            let branch_info = if let Some(ref preview) = screen_preview {
                extract_git_branch(preview)
                    .map(|branch| format!("\n• Branch: {} (with changes)", branch))
                    .unwrap_or_else(|| String::new())
            } else {
                String::new()
            };

            // Extract context from screen when idle
            let context_info = if !is_waiting {
                if let Some(ref preview) = screen_preview {
                    extract_conversation_context(preview)
                        .map(|ctx| format!("\n• Context: {}", ctx))
                        .unwrap_or_else(|| String::new())
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            format!("{}{}\n• State: {}{}", branch_info, if branch_info.is_empty() { "\n• Branch: unknown" } else { "" }, state_emoji, context_info)
        }
        None => {
            "\n• State: connecting...".to_string()
        }
    };

    status
}

/// Extract git branch from screen preview if visible.
fn extract_git_branch(screen: &str) -> Option<String> {
    // Look for common git branch indicators in terminal prompts
    // Examples: "(main)", "[master]", "on main", "◉ main", "(feature/new)"
    let patterns = [
        r"\(([a-zA-Z0-9_/-]+)\)",  // (branch)
        r"\[([a-zA-Z0-9_/-]+)\]",  // [branch]
        r"on ([a-zA-Z0-9_/-]+)",   // on branch
        r"◉ ([a-zA-Z0-9_/-]+)",    // ◉ branch
        r"➜ ([a-zA-Z0-9_/-]+)",    // ➜ branch
    ];

    for pattern in &patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(caps) = re.captures(screen) {
                if let Some(branch) = caps.get(1) {
                    let branch_name = branch.as_str();
                    // Filter out common false positives
                    if !branch_name.is_empty()
                        && branch_name.len() < 50
                        && !branch_name.chars().all(|c| c.is_numeric()) {
                        return Some(branch_name.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Extract conversation context from screen content when idle.
/// Attempts to find the last meaningful Claude response or status line.
fn extract_conversation_context(screen: &str) -> Option<String> {
    let lines: Vec<&str> = screen.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return None;
    }

    // Look for meaningful content patterns (in priority order)
    // 1. Lines starting with common status indicators
    let status_patterns = [
        ("✅", "completed"),
        ("🎉", "completed"),
        ("✓", "completed"),
        ("Fixed", "fix"),
        ("Created", "creation"),
        ("Updated", "update"),
        ("Added", "addition"),
        ("Working on", "in progress"),
        ("Implemented", "implementation"),
    ];

    // Check last few lines for status indicators
    for line in lines.iter().rev().take(10) {
        for (indicator, _category) in &status_patterns {
            if line.contains(indicator) {
                // Truncate to ~50-100 chars
                let truncated = if line.len() > 100 {
                    format!("{}...", &line[..97])
                } else {
                    line.to_string()
                };
                return Some(truncated);
            }
        }

        // 2. Lines that look like descriptions (contain key verbs/patterns)
        let action_words = ["fixed", "added", "updated", "created", "implemented", "working", "completed"];
        let lower = line.to_lowercase();
        if action_words.iter().any(|&word| lower.contains(word)) {
            let truncated = if line.len() > 100 {
                format!("{}...", &line[..97])
            } else {
                line.to_string()
            };
            return Some(truncated);
        }
    }

    // 3. Last substantial line (not prompt, not empty, has content)
    for line in lines.iter().rev().take(5) {
        // Skip lines that look like prompts or UI elements
        if line.starts_with('$')
            || line.starts_with('>')
            || line.starts_with('#')
            || line.starts_with('❯')
            || line.starts_with('➜')
            || line.starts_with('┃')
            || line.starts_with('│')
            || line.starts_with('├')
            || line.starts_with('└')
            || line.len() < 10
        {
            continue;
        }

        let truncated = if line.len() > 100 {
            format!("{}...", &line[..97])
        } else {
            line.to_string()
        };
        return Some(truncated);
    }

    None
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

        // Build activity section with LLM interpretation
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
            // Session is idle - try to interpret what it's showing
            if let Some(ref preview) = screen_preview {
                if !preview.is_empty() {
                    // Use LLM to interpret screen context
                    if let Some(interpretation) = commander_core::interpret_screen_context(preview, true) {
                        format!("💤 Activity: {}", html_escape(&interpretation))
                    } else {
                        "💤 Activity: Idle (ready for commands)".to_string()
                    }
                } else {
                    "💤 Activity: Idle (ready for commands)".to_string()
                }
            } else {
                "💤 Activity: Idle (ready for commands)".to_string()
            }
        };

        // Build screen preview section (only if LLM interpretation failed or is disabled)
        let screen_section = if let Some(preview) = screen_preview {
            if preview.is_empty() {
                String::new()
            } else {
                // Only show raw screen if LLM interpretation is not available
                if !commander_core::is_summarization_available() {
                    format!(
                        "\n\n📺 Screen:\n<pre>{}</pre>",
                        html_escape(&preview)
                    )
                } else {
                    // LLM available - interpretation is in activity section
                    String::new()
                }
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

/// Handle the /list command - alias for /sessions.
pub async fn handle_list(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    // /list is now an alias for /sessions
    handle_sessions(bot, msg, state).await
}

/// Handle regular text messages (forward to Claude Code).
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    // Extract text and thread_id early to avoid borrow issues
    let text = match msg.text() {
        Some(t) => t.to_string(),
        None => return Ok(()),
    };
    let thread_id = msg.thread_id;

    // Check if this is a message in a forum topic (group mode)
    if let Some(tid) = thread_id {
        return handle_topic_message(bot, msg, state, &text, tid).await;
    }

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
    match state.send_message(msg.chat.id, &text, Some(msg.id)).await {
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

/// Handle messages sent in forum topics (group mode).
async fn handle_topic_message(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    text: &str,
    thread_id: ThreadId,
) -> ResponseResult<()> {
    // Check if group mode is enabled for this chat
    if !state.is_group_mode(msg.chat.id.0).await {
        // Not in group mode - ignore topic messages or send help
        debug!(
            chat_id = %msg.chat.id,
            thread_id = ?thread_id,
            "Topic message received but group mode not enabled"
        );
        return Ok(());
    }

    // Check if this topic has a session mapping
    if !state.has_topic_session(msg.chat.id, thread_id).await {
        // Topic not configured - could be General topic or unlinked topic
        // Check if there's a topic config (for topics created outside of /topic command)
        if let Some(topic_config) = state.get_topic_session(msg.chat.id.0, thread_id.0.0).await {
            // Topic exists in config but session not active - reconnect
            match state.connect_topic(msg.chat.id, thread_id, &topic_config.session_name).await {
                Ok(_) => {
                    debug!(
                        chat_id = %msg.chat.id,
                        thread_id = ?thread_id,
                        session = %topic_config.session_name,
                        "Reconnected topic session"
                    );
                }
                Err(e) => {
                    warn!(
                        chat_id = %msg.chat.id,
                        thread_id = ?thread_id,
                        error = %e,
                        "Failed to reconnect topic session"
                    );
                    bot.send_message(
                        msg.chat.id,
                        format!("Failed to connect to session: {}", e),
                    )
                    .message_thread_id(thread_id)
                    .await?;
                    return Ok(());
                }
            }
        } else {
            // Topic has no session mapping - ignore or send help
            bot.send_message(
                msg.chat.id,
                "This topic is not linked to a session.\n\n\
                Use <code>/topic &lt;session&gt;</code> in the main chat to create a linked topic.",
            )
            .message_thread_id(thread_id)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
            return Ok(());
        }
    }

    // Send typing indicator to the topic
    let _ = bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
        .message_thread_id(thread_id)
        .await;

    // Send message to the topic's session
    match state.send_message_to_topic(msg.chat.id, thread_id, text, Some(msg.id)).await {
        Ok(()) => {
            debug!(
                chat_id = %msg.chat.id,
                thread_id = ?thread_id,
                message = %text,
                "Message sent to topic session"
            );
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("❌ Error: {}", e))
                .message_thread_id(thread_id)
                .await?;
            error!(
                chat_id = %msg.chat.id,
                thread_id = ?thread_id,
                error = %e,
                "Failed to send topic message"
            );
        }
    }

    Ok(())
}

/// Handle the /sessions command - list tmux sessions with status info.
pub async fn handle_sessions(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let sessions = state.list_tmux_sessions_with_status();

    if sessions.is_empty() {
        bot.send_message(msg.chat.id, "No tmux sessions found.")
            .await?;
        return Ok(());
    }

    let current_session = state
        .get_session_info(msg.chat.id)
        .await
        .map(|(_, path)| format!("commander-{}", path.rsplit('/').next().unwrap_or("")));

    let mut text = String::from("<b>Sessions:</b>\n\n");

    for (name, is_commander, created_at, preview) in &sessions {
        let is_current = current_session.as_ref().map(|s| s == name).unwrap_or(false);
        let marker = if is_current {
            "✅"
        } else if *is_commander {
            "🤖"
        } else {
            "📟"
        };

        // Determine status from screen preview
        let status = if let Some(ref prev) = preview {
            if prev.contains("Waiting") || prev.contains(">") || prev.contains("$") {
                "💤 idle"
            } else {
                "🔄 running"
            }
        } else {
            "❓ unknown"
        };

        // Format created time as relative
        let age = chrono::Utc::now().signed_duration_since(*created_at);
        let age_str = if age.num_days() > 0 {
            format!("{}d ago", age.num_days())
        } else if age.num_hours() > 0 {
            format!("{}h ago", age.num_hours())
        } else {
            format!("{}m ago", age.num_minutes())
        };

        let display_name = name.strip_prefix("commander-").unwrap_or(name);

        // Use clickable command link instead of inline button
        text.push_str(&format!(
            "{} <b>{}</b>\n   {} | started {}\n   /connect {}\n\n",
            marker,
            html_escape(display_name),
            status,
            age_str,
            html_escape(name)
        ));
    }

    text.push_str("Click a /connect command to connect.");

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

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
            "Not authorized. Use <code>/pair &lt;code&gt;</code> first.\n\n\
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

/// Handle the /send command - send a message directly to the session without LLM interpretation.
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
            <b>What's the difference?</b>\n\
            • Regular messages are interpreted by the commander AI\n\
            • <code>/send</code> bypasses AI and sends directly to the session\n\n\
            <b>Examples:</b>\n\
            • <code>/send /help</code> - Send \"/help\" to Claude Code\n\
            • <code>/send cd ..</code> - Navigate without AI interpretation",
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

    // Send the message directly without LLM interpretation
    match state.send_message_direct(msg.chat.id, message, Some(msg.id)).await {
        Ok(()) => {
            debug!(chat_id = %msg.chat.id, message = %message, "Message sent via /send (direct, no LLM)");
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("Failed to send: {}", e))
                .await?;
            error!(chat_id = %msg.chat.id, error = %e, "Failed to send message via /send");
        }
    }

    Ok(())
}

/// Handle callback queries from inline keyboard buttons.
pub async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    let Some(data) = &q.data else {
        return Ok(());
    };

    // Acknowledge callback immediately to remove loading state
    bot.answer_callback_query(&q.id).await?;

    // Parse callback data
    if let Some(session) = data.strip_prefix("connect:") {
        let Some(msg) = q.message.as_ref() else {
            return Ok(());
        };
        let chat_id = msg.chat().id;

        // Check authorization
        if !state.is_authorized(chat_id.0).await {
            bot.send_message(
                chat_id,
                "Not authorized. Use <code>/pair &lt;code&gt;</code> first.",
            )
            .await?;
            return Ok(());
        }

        // Disconnect from current session if connected
        if let Some((current_project, _)) = state.get_session_info(chat_id).await {
            if current_project == session {
                bot.send_message(chat_id, format!("Already connected to <b>{}</b>", current_project))
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await?;
                return Ok(());
            }
            let _ = state.disconnect(chat_id).await;
        }

        // Connect to the selected session
        match state.connect(chat_id, session).await {
            Ok((name, tool_id)) => {
                let adapter = adapter_display_name(&tool_id);
                bot.send_message(
                    chat_id,
                    format!(
                        "✅ Connected to <b>{}</b>\n\nSend messages to interact with {}.",
                        name, adapter
                    ),
                )
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
                info!(chat_id = %chat_id, project = %name, "User connected via inline button");
            }
            Err(e) => {
                bot.send_message(chat_id, format!("❌ Failed to connect: {}", e))
                    .await?;
                error!(chat_id = %chat_id, error = %e, "Connection via button failed");
            }
        }
    }

    Ok(())
}

/// Handle the /groupmode command - enable group mode for a supergroup.
pub async fn handle_groupmode(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    // Check authorization first
    if !state.is_authorized(msg.chat.id.0).await {
        bot.send_message(
            msg.chat.id,
            "Not authorized. Use <code>/pair &lt;code&gt;</code> first.\n\n\
            Get a pairing code by running <code>/telegram</code> in the Commander CLI.",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    // Check if this is a supergroup with forums enabled
    let chat = bot.get_chat(msg.chat.id).await?;

    // Extract is_forum from the supergroup struct
    let is_forum = match &chat.kind {
        teloxide::types::ChatKind::Public(public) => {
            match &public.kind {
                teloxide::types::PublicChatKind::Supergroup(sg) => sg.is_forum,
                _ => false,
            }
        }
        _ => false,
    };

    let is_supergroup = matches!(&chat.kind, teloxide::types::ChatKind::Public(ref p)
        if matches!(p.kind, teloxide::types::PublicChatKind::Supergroup(_)));

    if !is_supergroup {
        bot.send_message(
            msg.chat.id,
            "Group mode is only available in supergroups.\n\n\
            To use group mode:\n\
            1. Convert this group to a supergroup (add a username or enable topics)\n\
            2. Enable Forum Topics in group settings\n\
            3. Run /groupmode again",
        )
        .await?;
        return Ok(());
    }

    // Check if forums are enabled
    if !is_forum {
        bot.send_message(
            msg.chat.id,
            "Forum Topics are not enabled for this supergroup.\n\n\
            To enable:\n\
            1. Go to Group Settings\n\
            2. Enable \"Topics\"\n\
            3. Run /groupmode again",
        )
        .await?;
        return Ok(());
    }

    // Enable group mode
    if let Err(e) = state.enable_group_mode(msg.chat.id.0).await {
        bot.send_message(msg.chat.id, format!("Failed to enable group mode: {}", e))
            .await?;
        return Ok(());
    }

    bot.send_message(
        msg.chat.id,
        "Group mode enabled!\n\n\
        You can now create topics for different sessions:\n\
        • <code>/topic &lt;session&gt;</code> - Create a topic for a session\n\
        • <code>/topics</code> - List all topics and their sessions\n\n\
        Messages in each topic will route to that topic's session.",
    )
    .parse_mode(teloxide::types::ParseMode::Html)
    .await?;

    info!(chat_id = %msg.chat.id, "Group mode enabled");
    Ok(())
}

/// Handle the /topic command - create a topic for a session.
pub async fn handle_topic(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
    session_name: String,
) -> ResponseResult<()> {
    // Check authorization first
    if !state.is_authorized(msg.chat.id.0).await {
        bot.send_message(
            msg.chat.id,
            "Not authorized. Use <code>/pair &lt;code&gt;</code> first.",
        )
        .await?;
        return Ok(());
    }

    let session_name = session_name.trim();

    if session_name.is_empty() {
        bot.send_message(
            msg.chat.id,
            "Please specify a session name.\n\n\
            <b>Usage:</b> <code>/topic &lt;session&gt;</code>\n\n\
            Use /list to see available projects and sessions.",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    // Check if group mode is enabled
    if !state.is_group_mode(msg.chat.id.0).await {
        bot.send_message(
            msg.chat.id,
            "Group mode is not enabled.\n\n\
            Run /groupmode first to enable it.",
        )
        .await?;
        return Ok(());
    }

    // Create the forum topic
    // Default icon color: 0x6FB9F0 (blue) = 7322096
    let icon_color: u32 = 7322096;

    // teloxide 0.13 create_forum_topic requires icon_custom_emoji_id
    // We'll use an empty string to get the default icon
    let topic_result = bot.create_forum_topic(msg.chat.id, session_name, icon_color, "")
        .await;

    match topic_result {
        Ok(topic) => {
            let thread_id = topic.thread_id;

            // Connect the topic to the session
            match state.connect_topic(msg.chat.id, thread_id, session_name).await {
                Ok((connected_name, tool_id)) => {
                    let adapter_name = adapter_display_name(&tool_id);

                    // Send confirmation to the topic
                    bot.send_message(msg.chat.id, format!(
                        "Topic created and connected to <b>{}</b>!\n\n\
                        Send messages in this topic to interact with {}.",
                        connected_name, adapter_name
                    ))
                    .message_thread_id(thread_id)
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await?;

                    info!(
                        chat_id = %msg.chat.id,
                        thread_id = ?thread_id,
                        session = %session_name,
                        "Topic created for session"
                    );
                }
                Err(e) => {
                    // Topic was created but connection failed
                    bot.send_message(msg.chat.id, format!(
                        "Topic created but failed to connect to session: {}\n\n\
                        The topic exists but is not linked to a session.",
                        e
                    ))
                    .message_thread_id(thread_id)
                    .await?;
                    error!(
                        chat_id = %msg.chat.id,
                        error = %e,
                        "Failed to connect topic to session"
                    );
                }
            }
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!(
                "Failed to create topic: {}\n\n\
                Make sure the bot has 'Manage Topics' permission.",
                e
            ))
            .await?;
            error!(chat_id = %msg.chat.id, error = %e, "Failed to create forum topic");
        }
    }

    Ok(())
}

/// Handle the /topics command - list all topics and their sessions.
pub async fn handle_topics(
    bot: Bot,
    msg: Message,
    state: Arc<TelegramState>,
) -> ResponseResult<()> {
    // Check if group mode is enabled
    if !state.is_group_mode(msg.chat.id.0).await {
        bot.send_message(
            msg.chat.id,
            "Group mode is not enabled.\n\n\
            Run /groupmode first to enable it, then use /topic to create topics.",
        )
        .await?;
        return Ok(());
    }

    let topics = state.list_topics(msg.chat.id.0).await;

    if topics.is_empty() {
        bot.send_message(
            msg.chat.id,
            "No topics configured.\n\n\
            Use <code>/topic &lt;session&gt;</code> to create a topic for a session.",
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
        return Ok(());
    }

    let mut text = String::from("<b>Forum Topics:</b>\n\n");
    for topic in &topics {
        text.push_str(&format!(
            "• <b>{}</b> (thread {})\n   tmux: <code>{}</code>\n\n",
            html_escape(&topic.session_name),
            topic.thread_id,
            html_escape(&topic.tmux_session)
        ));
    }

    bot.send_message(msg.chat.id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

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
        Command::Sessions => handle_sessions(bot, msg, state).await,
        Command::Disconnect => handle_disconnect(bot, msg, state).await,
        Command::Stop(session) => handle_stop(bot, msg, state, session).await,
        Command::Send(message) => handle_send(bot, msg, state, message).await,
        Command::Status => handle_status(bot, msg, state).await,
        Command::List => handle_list(bot, msg, state).await,
        Command::GroupMode => handle_groupmode(bot, msg, state).await,
        Command::Topic(session) => handle_topic(bot, msg, state, session).await,
        Command::Topics => handle_topics(bot, msg, state).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_git_branch_parens() {
        let screen = "user@host ~/project (main) $ ";
        assert_eq!(extract_git_branch(screen), Some("main".to_string()));
    }

    #[test]
    fn test_extract_git_branch_brackets() {
        let screen = "user@host ~/project [develop] $ ";
        assert_eq!(extract_git_branch(screen), Some("develop".to_string()));
    }

    #[test]
    fn test_extract_git_branch_with_slash() {
        let screen = "~/project (feature/new-feature) $ ";
        assert_eq!(extract_git_branch(screen), Some("feature/new-feature".to_string()));
    }

    #[test]
    fn test_extract_git_branch_unicode() {
        let screen = "~/project ◉ staging $ ";
        assert_eq!(extract_git_branch(screen), Some("staging".to_string()));
    }

    #[test]
    fn test_extract_git_branch_not_found() {
        let screen = "user@host ~/project $ ";
        assert_eq!(extract_git_branch(screen), None);
    }

    #[test]
    fn test_extract_git_branch_filters_numbers() {
        let screen = "user@host ~/project (12345) $ ";
        assert_eq!(extract_git_branch(screen), None); // All numeric, filtered out
    }

    #[test]
    fn test_extract_conversation_context_with_status() {
        let screen = "Some output\n✅ Fixed HTML parsing bug\nPrompt> ";
        let context = extract_conversation_context(screen);
        assert!(context.is_some());
        assert!(context.unwrap().contains("Fixed HTML parsing bug"));
    }

    #[test]
    fn test_extract_conversation_context_with_action() {
        let screen = "Processing...\nWorking on Telegram bot improvements\nReady $ ";
        let context = extract_conversation_context(screen);
        assert!(context.is_some());
        assert!(context.unwrap().contains("Working on Telegram bot improvements"));
    }

    #[test]
    fn test_extract_conversation_context_truncates_long_lines() {
        let long_line = "Fixed ".to_string() + &"x".repeat(200);
        let screen = format!("Some output\n{}\nPrompt> ", long_line);
        let context = extract_conversation_context(&screen);
        assert!(context.is_some());
        let ctx = context.unwrap();
        assert!(ctx.len() <= 103); // 100 chars + "..."
        assert!(ctx.ends_with("..."));
    }

    #[test]
    fn test_extract_conversation_context_skips_prompts() {
        let screen = "$ ls -la\n> command\n❯ prompt\n";
        let context = extract_conversation_context(screen);
        assert_eq!(context, None);
    }

    #[test]
    fn test_extract_conversation_context_empty_screen() {
        let screen = "";
        let context = extract_conversation_context(screen);
        assert_eq!(context, None);
    }
}
