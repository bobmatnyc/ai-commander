//! Interactive REPL (Read-Eval-Print Loop) for Commander.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use commander_adapters::AdapterRegistry;
use commander_models::Project;
use commander_persistence::StateStore;
use commander_tmux::TmuxOrchestrator;
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RlResult};
use tracing::{debug, info};

use crate::chat::ChatClient;

/// Arguments for the enhanced /connect command.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectArgs {
    /// Path to the project directory.
    pub path: PathBuf,
    /// Adapter tool ID (e.g., "claude-code", "mpm").
    pub tool: String,
    /// Friendly project alias/name.
    pub alias: String,
}

/// Help information for a command.
pub struct CommandHelp {
    /// Command name (e.g., "connect").
    pub name: &'static str,
    /// Command aliases (e.g., ["c"]).
    pub aliases: &'static [&'static str],
    /// Brief one-line description.
    pub brief: &'static str,
    /// Detailed description.
    pub description: &'static str,
    /// Usage syntax.
    pub usage: &'static str,
    /// Examples with descriptions.
    pub examples: &'static [(&'static str, &'static str)],
}

/// Static help entries for all commands.
static COMMAND_HELP: &[CommandHelp] = &[
    CommandHelp {
        name: "list",
        aliases: &["ls", "l"],
        brief: "List all projects",
        description: "Displays all registered projects with their state. Connected project is marked with *.",
        usage: "/list",
        examples: &[
            ("/list", "List all projects"),
            ("/ls", "Same as /list"),
        ],
    },
    CommandHelp {
        name: "status",
        aliases: &["s"],
        brief: "Show project status",
        description: "Displays detailed status for a project including state, path, and configuration.",
        usage: "/status [project]",
        examples: &[
            ("/status", "Show status of connected project"),
            ("/status myapp", "Show status of 'myapp' project"),
        ],
    },
    CommandHelp {
        name: "connect",
        aliases: &["c"],
        brief: "Connect to a project (starts if needed)",
        description: "Connect to a project. If the project is not running, it will be started first.\n\n\
                      For new projects: provide path with -a (adapter) and -n (name) flags.\n\
                      For existing projects: just provide the project name.\n\
                      Tool aliases: cc = claude-code, mpm = mpm",
        usage: "/connect <path> -a <adapter> -n <name>  (new project)\n       /connect <project-name>               (existing project)",
        examples: &[
            ("/connect ~/code/myapp -a cc -n myapp", "Start and connect to new project"),
            ("/connect ~/code/api -a mpm -n api", "Start project with mpm adapter"),
            ("/connect myapp", "Connect to existing project (starts if not running)"),
        ],
    },
    CommandHelp {
        name: "disconnect",
        aliases: &["dc"],
        brief: "Disconnect from current project",
        description: "Disconnects from the currently connected project. Messages will route to chat mode.",
        usage: "/disconnect",
        examples: &[
            ("/disconnect", "Disconnect from current project"),
            ("/dc", "Same as /disconnect"),
        ],
    },
    CommandHelp {
        name: "send",
        aliases: &[],
        brief: "Send message to connected project",
        description: "Explicitly sends a message to the connected project's session.",
        usage: "/send <message>",
        examples: &[
            ("/send hello world", "Send 'hello world' to connected project"),
        ],
    },
    CommandHelp {
        name: "sessions",
        aliases: &[],
        brief: "List all tmux sessions",
        description: "Lists all tmux sessions, showing which are commander sessions vs external, and which are currently connected.",
        usage: "/sessions",
        examples: &[
            ("/sessions", "List all tmux sessions"),
        ],
    },
    CommandHelp {
        name: "stop",
        aliases: &[],
        brief: "Stop session (commits changes, ends tmux)",
        description: "Stops a session by first committing any uncommitted git changes in the project directory, \
                      then destroying the tmux session. If stopping the connected session, also disconnects.",
        usage: "/stop [session]",
        examples: &[
            ("/stop", "Stop current connected session"),
            ("/stop duetto", "Stop the 'duetto' session"),
        ],
    },
    CommandHelp {
        name: "help",
        aliases: &["h", "?"],
        brief: "Show help",
        description: "Shows help for all commands or detailed help for a specific command.",
        usage: "/help [command]",
        examples: &[
            ("/help", "Show all commands"),
            ("/help connect", "Show detailed help for /connect"),
            ("/help c", "Also works with aliases"),
        ],
    },
    CommandHelp {
        name: "quit",
        aliases: &["q", "exit"],
        brief: "Exit the REPL",
        description: "Exits the Commander REPL. History is saved automatically.",
        usage: "/quit",
        examples: &[
            ("/quit", "Exit the REPL"),
            ("/q", "Same as /quit"),
        ],
    },
];

/// Slash commands available in the REPL.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplCommand {
    /// List all projects
    List,
    /// Show status of current or specified project
    Status(Option<String>),
    /// Connect to a project (starts if needed)
    /// - ConnectArgs: new project with path, adapter, name
    /// - String: existing project by name
    Connect(ConnectTarget),
    /// Disconnect from current project
    Disconnect,
    /// Send message to connected project
    Send(String),
    /// List all tmux sessions
    Sessions,
    /// Stop a session (commits git changes, destroys tmux)
    Stop(Option<String>),
    /// Show help (optionally for a specific command)
    Help(Option<String>),
    /// Quit the REPL
    Quit,
    /// Unknown command
    Unknown(String),
    /// Plain text (not a command)
    Text(String),
}

/// Target for /connect command - either a new project or existing project name.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectTarget {
    /// New project: path with adapter and name
    New(ConnectArgs),
    /// Existing project by name
    Existing(String),
}

impl ReplCommand {
    /// Parses input into a REPL command.
    pub fn parse(input: &str) -> Self {
        let input = input.trim();

        if input.is_empty() {
            return ReplCommand::Text(String::new());
        }

        // Check for slash commands
        if let Some(stripped) = input.strip_prefix('/') {
            let parts: Vec<&str> = stripped.splitn(2, ' ').collect();
            let cmd = parts[0].to_lowercase();
            let arg = parts.get(1).map(|s| s.trim().to_string());

            match cmd.as_str() {
                "list" | "ls" | "l" => ReplCommand::List,
                "status" | "s" => ReplCommand::Status(arg),
                "connect" | "c" => Self::parse_connect(arg),
                "disconnect" | "dc" => ReplCommand::Disconnect,
                "send" => arg
                    .map(ReplCommand::Send)
                    .unwrap_or(ReplCommand::Unknown("send requires a message".to_string())),
                "sessions" => ReplCommand::Sessions,
                "stop" => ReplCommand::Stop(arg),
                "help" | "h" | "?" => ReplCommand::Help(arg),
                "quit" | "q" | "exit" => ReplCommand::Quit,
                _ => ReplCommand::Unknown(cmd),
            }
        } else if let Some(stripped) = input.strip_prefix('@') {
            // @mention syntax - treat as send to project
            let parts: Vec<&str> = stripped.splitn(2, ' ').collect();
            if parts.len() == 2 {
                // For now, just echo - actual implementation in Phase 7
                ReplCommand::Send(input.to_string())
            } else {
                ReplCommand::Text(input.to_string())
            }
        } else {
            // Check for conversational commands
            Self::parse_conversational(input)
        }
    }

    /// Parse conversational commands (natural language alternatives to slash commands).
    fn parse_conversational(input: &str) -> Self {
        let lower = input.to_lowercase();

        // Connect patterns: "connect to X", "connect X"
        if let Some(target) = lower.strip_prefix("connect to ") {
            let target = target.trim();
            if !target.is_empty() {
                return Self::parse_connect(Some(target.to_string()));
            }
        }
        if let Some(target) = lower.strip_prefix("connect ") {
            let target = target.trim();
            if !target.is_empty() {
                return Self::parse_connect(Some(target.to_string()));
            }
        }

        // List patterns
        if lower == "list" || lower == "list projects" || lower == "show projects" {
            return ReplCommand::List;
        }

        // Status patterns: "status", "show status", "status of X"
        if lower == "status" || lower == "show status" {
            return ReplCommand::Status(None);
        }
        if let Some(project) = lower.strip_prefix("status of ") {
            let project = project.trim();
            if !project.is_empty() {
                return ReplCommand::Status(Some(project.to_string()));
            }
        }

        // Disconnect patterns
        if lower == "disconnect" || lower == "disconnect from project" {
            return ReplCommand::Disconnect;
        }

        // Help patterns
        if lower == "help" || lower == "show help" || lower == "?" {
            return ReplCommand::Help(None);
        }

        // Quit patterns
        if lower == "quit" || lower == "exit" || lower == "bye" {
            return ReplCommand::Quit;
        }

        // Default: treat as text
        ReplCommand::Text(input.to_string())
    }

    /// Parse connect command arguments.
    /// Supports:
    /// - /connect <name> - connect to existing project
    /// - /connect <path> -a <adapter> -n <name> - start new project and connect
    fn parse_connect(arg: Option<String>) -> Self {
        let Some(arg) = arg else {
            return ReplCommand::Unknown("connect requires arguments".to_string());
        };

        let parts: Vec<&str> = arg.split_whitespace().collect();

        // Check if this looks like a new project command (has -a or -n flags)
        if parts.iter().any(|&p| p == "-a" || p == "-n") {
            // New project syntax: /connect <path> -a <adapter> -n <name>
            if parts.is_empty() {
                return ReplCommand::Unknown(
                    "connect requires: /connect <path> -a <adapter> -n <name>".to_string(),
                );
            }

            let path = PathBuf::from(shellexpand::tilde(parts[0]).to_string());
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
                            return ReplCommand::Unknown("-a requires an adapter name".to_string());
                        }
                    }
                    "-n" => {
                        if i + 1 < parts.len() {
                            name = Some(parts[i + 1].to_string());
                            i += 2;
                        } else {
                            return ReplCommand::Unknown("-n requires a project name".to_string());
                        }
                    }
                    _ => {
                        return ReplCommand::Unknown(format!("unknown flag: {}", parts[i]));
                    }
                }
            }

            match (adapter, name) {
                (Some(tool), Some(alias)) => {
                    ReplCommand::Connect(ConnectTarget::New(ConnectArgs { path, tool, alias }))
                }
                (None, _) => ReplCommand::Unknown("connect requires -a <adapter>".to_string()),
                (_, None) => ReplCommand::Unknown("connect requires -n <name>".to_string()),
            }
        } else if parts.len() == 1 {
            // Existing project: /connect project-name
            ReplCommand::Connect(ConnectTarget::Existing(parts[0].to_string()))
        } else {
            ReplCommand::Unknown(
                "connect: use '/connect <path> -a <adapter> -n <name>' or '/connect <project>'"
                    .to_string(),
            )
        }
    }
}

/// REPL state
pub struct Repl {
    editor: DefaultEditor,
    store: StateStore,
    registry: AdapterRegistry,
    connected_project: Option<String>,
    history_path: Option<std::path::PathBuf>,
    chat_client: ChatClient,
    runtime: tokio::runtime::Runtime,
    /// Tmux orchestrator for sending messages to project sessions.
    tmux: Option<TmuxOrchestrator>,
    /// Map of project name/alias to tmux session name.
    sessions: HashMap<String, String>,
}

impl Repl {
    /// Creates a new REPL instance.
    pub fn new(state_dir: &Path) -> RlResult<Self> {
        let mut editor = DefaultEditor::new()?;
        let store = StateStore::new(state_dir);
        let registry = AdapterRegistry::new();
        let chat_client = ChatClient::new();

        // Create tokio runtime for async operations
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

        // Set up history file
        let history_path = state_dir.join("repl_history.txt");
        if history_path.exists() {
            let _ = editor.load_history(&history_path);
        }

        // Initialize tmux orchestrator (gracefully handle if unavailable)
        let tmux = match TmuxOrchestrator::new() {
            Ok(t) => {
                debug!("tmux orchestrator initialized");
                Some(t)
            }
            Err(e) => {
                debug!("tmux not available: {}", e);
                None
            }
        };

        Ok(Self {
            editor,
            store,
            registry,
            connected_project: None,
            history_path: Some(history_path),
            chat_client,
            runtime,
            tmux,
            sessions: HashMap::new(),
        })
    }

    /// Runs the REPL loop.
    pub fn run(&mut self) -> RlResult<()> {
        println!("Commander REPL v{}", env!("CARGO_PKG_VERSION"));
        println!("Type /help for commands, /quit to exit");
        if self.chat_client.is_available() {
            println!("Chat mode available (OpenRouter)");
        }
        println!();

        loop {
            let prompt = self.prompt();

            match self.editor.readline(&prompt) {
                Ok(line) => {
                    self.editor.add_history_entry(&line)?;

                    let cmd = ReplCommand::parse(&line);
                    debug!(?cmd, "Parsed command");

                    match self.handle_command(cmd) {
                        Ok(true) => break, // Quit requested
                        Ok(false) => {}    // Continue
                        Err(e) => eprintln!("Error: {}", e),
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                    // Don't exit on Ctrl+C, just clear line
                }
                Err(ReadlineError::Eof) => {
                    println!("^D");
                    break;
                }
                Err(err) => {
                    eprintln!("Error: {:?}", err);
                    break;
                }
            }
        }

        // Save history
        if let Some(path) = &self.history_path {
            let _ = self.editor.save_history(path);
        }

        println!("Goodbye!");
        Ok(())
    }

    /// Returns the prompt string.
    fn prompt(&self) -> String {
        match &self.connected_project {
            Some(project) => format!("commander [{}]> ", project),
            None => "commander> ".to_string(),
        }
    }

    /// Handles a REPL command. Returns Ok(true) if should quit.
    fn handle_command(&mut self, cmd: ReplCommand) -> Result<bool, Box<dyn std::error::Error>> {
        match cmd {
            ReplCommand::List => {
                let projects = self.store.load_all_projects()?;
                if projects.is_empty() {
                    println!("No projects.");
                } else {
                    for project in projects.values() {
                        let marker = if Some(&project.name) == self.connected_project.as_ref() {
                            "*"
                        } else {
                            " "
                        };
                        println!(
                            "{} {} ({:?}) - {}",
                            marker, project.name, project.state, project.id
                        );
                    }
                }
                Ok(false)
            }

            ReplCommand::Status(project) => {
                let id = project.as_ref().or(self.connected_project.as_ref());
                match id {
                    Some(id) => {
                        let projects = self.store.load_all_projects()?;
                        match projects
                            .values()
                            .find(|p| &p.name == id || p.id.as_str() == id)
                        {
                            Some(p) => {
                                println!("Project: {} ({})", p.name, p.id);
                                println!("  State: {:?}", p.state);
                                println!("  Path: {}", p.path);
                            }
                            None => println!("Project not found: {}", id),
                        }
                    }
                    None => {
                        println!(
                            "No project connected. Use /connect <project> or /status <project>"
                        )
                    }
                }
                Ok(false)
            }

            ReplCommand::Connect(target) => {
                self.handle_connect(target)?;
                Ok(false)
            }

            ReplCommand::Disconnect => {
                if let Some(project) = self.connected_project.take() {
                    println!("Disconnected from '{}'", project);
                } else {
                    println!("Not connected to any project");
                }
                Ok(false)
            }

            ReplCommand::Send(message) => {
                match &self.connected_project {
                    Some(project) => {
                        if let Some(tmux) = &self.tmux {
                            if let Some(session) = self.sessions.get(project) {
                                // Capture initial output to establish baseline (full content hash)
                                let initial_output = tmux
                                    .capture_output(session, None, Some(200))
                                    .unwrap_or_default();

                                match tmux.send_line(session, None, &message) {
                                    Ok(_) => {
                                        println!("[{}] > {}", project, message);
                                        print!("[working");
                                        io::stdout().flush().ok();

                                        // Poll for new output using content comparison
                                        let poll_interval = std::time::Duration::from_millis(250);
                                        let max_wait = std::time::Duration::from_secs(60);
                                        let start = std::time::Instant::now();
                                        let mut last_change_time = start;
                                        let idle_timeout = std::time::Duration::from_secs(3);
                                        let mut last_output = initial_output.clone();
                                        let mut dots_printed = 0;
                                        let mut got_response = false;

                                        while start.elapsed() < max_wait {
                                            std::thread::sleep(poll_interval);

                                            // Show progress dots
                                            if dots_printed < 20 {
                                                print!(".");
                                                io::stdout().flush().ok();
                                                dots_printed += 1;
                                            }

                                            if let Ok(current_output) =
                                                tmux.capture_output(session, None, Some(200))
                                            {
                                                // Find new content by comparing outputs
                                                if current_output != last_output {
                                                    // Find lines that weren't in the previous capture
                                                    let new_lines = find_new_lines(&last_output, &current_output, &message);

                                                    if !new_lines.is_empty() {
                                                        // End the [working...] line on first output
                                                        if !got_response {
                                                            println!("]");
                                                            got_response = true;
                                                        }

                                                        for line in &new_lines {
                                                            println!("[{}] {}", project, line);
                                                        }
                                                        last_change_time = std::time::Instant::now();
                                                    }

                                                    last_output = current_output;
                                                }
                                            }

                                            // Stop polling after idle period with some response
                                            if last_change_time.elapsed() > idle_timeout && got_response {
                                                break;
                                            }
                                        }

                                        // End progress indicator if no response received
                                        if !got_response {
                                            println!("]");
                                            println!("(AI is processing - response will appear in tmux session)");
                                        }
                                    }
                                    Err(e) => {
                                        println!("Failed to send message: {}", e);
                                    }
                                }
                            } else {
                                println!(
                                    "Project '{}' not running. Reconnect with path to start it.",
                                    project
                                );
                            }
                        } else {
                            println!("Tmux not available. Cannot send messages to projects.");
                        }
                    }
                    None => {
                        println!("Not connected to any project. Use /connect <project> first.");
                    }
                }
                Ok(false)
            }

            ReplCommand::Sessions => {
                if let Some(tmux) = &self.tmux {
                    match tmux.list_sessions() {
                        Ok(sessions) => {
                            if sessions.is_empty() {
                                println!("No tmux sessions found.");
                            } else {
                                println!("Available tmux sessions:");
                                for session in sessions {
                                    let is_commander = session.name.starts_with("commander-");
                                    let is_connected = self.sessions.values().any(|s| s == &session.name);

                                    let marker = if is_connected { "*" } else { " " };
                                    let suffix = if is_connected {
                                        " (connected)"
                                    } else if !is_commander {
                                        " (external)"
                                    } else {
                                        ""
                                    };

                                    println!("  {} {}{}", marker, session.name, suffix);
                                }
                                println!();
                                println!("Use /connect <name> to connect");
                            }
                        }
                        Err(e) => {
                            println!("Failed to list sessions: {}", e);
                        }
                    }
                } else {
                    println!("Tmux not available");
                }
                Ok(false)
            }

            ReplCommand::Stop(target) => {
                let name = target.or_else(|| self.connected_project.clone());

                if let Some(name) = name {
                    self.stop_session(&name)?;
                } else {
                    println!("Usage: /stop [session] or connect to a session first");
                }
                Ok(false)
            }

            ReplCommand::Help(topic) => {
                print_help(topic.as_deref());
                Ok(false)
            }

            ReplCommand::Quit => Ok(true),

            ReplCommand::Unknown(cmd) => {
                println!(
                    "Unknown command: /{}. Type /help for available commands.",
                    cmd
                );
                Ok(false)
            }

            ReplCommand::Text(text) => {
                if !text.is_empty() {
                    // If connected, treat as message to send
                    if self.connected_project.is_some() {
                        self.handle_command(ReplCommand::Send(text))?;
                    } else if self.chat_client.is_available() {
                        // Chat mode - send to OpenRouter
                        self.handle_chat(&text)?;
                    } else {
                        println!("Not connected. Use /connect <project> or set OPENROUTER_API_KEY for chat.");
                    }
                }
                Ok(false)
            }
        }
    }

    /// Handle /connect command - connects to existing project or starts new one.
    fn handle_connect(&mut self, target: ConnectTarget) -> Result<(), Box<dyn std::error::Error>> {
        match target {
            ConnectTarget::New(args) => {
                // Resolve tool alias
                let tool_id = match self.registry.resolve(&args.tool) {
                    Some(id) => id.to_string(),
                    None => {
                        println!(
                            "Unknown adapter: {}. Available: cc (claude-code), mpm",
                            args.tool
                        );
                        return Ok(());
                    }
                };

                // Expand and validate path
                let path = if args.path.starts_with("~") {
                    dirs::home_dir()
                        .map(|h| h.join(args.path.strip_prefix("~").unwrap_or(&args.path)))
                        .unwrap_or(args.path.clone())
                } else {
                    args.path.clone()
                };

                let path_str = path.to_string_lossy().to_string();

                // Check if project with this alias already exists
                let projects = self.store.load_all_projects()?;
                if let Some(existing) = projects.values().find(|p| p.name == args.alias) {
                    // Project exists - check if running, if not start it
                    if self.sessions.contains_key(&args.alias) {
                        println!("Connected to '{}'", args.alias);
                    } else {
                        // Start the existing project
                        self.start_project_session(&args.alias, &existing.path, &tool_id)?;
                        println!("Starting '{}'...", args.alias);
                        println!("Connected to '{}'", args.alias);
                    }
                    self.connected_project = Some(args.alias);
                    return Ok(());
                }

                // Create new project
                let mut project = Project::new(&path_str, &args.alias);
                project
                    .config
                    .insert("tool".to_string(), serde_json::json!(tool_id));

                // Save project
                self.store.save_project(&project)?;

                // Start tmux session and launch adapter
                self.start_project_session(&args.alias, &path_str, &tool_id)?;

                info!(
                    project = %args.alias,
                    path = %path_str,
                    tool = %tool_id,
                    "Started and connected to project"
                );

                println!("Starting '{}'...", args.alias);
                self.connected_project = Some(args.alias.clone());
                println!("Connected to '{}'", args.alias);
            }

            ConnectTarget::Existing(name) => {
                let projects = self.store.load_all_projects()?;
                if let Some(project) = projects
                    .values()
                    .find(|p| p.name == name || p.id.as_str() == name)
                {
                    // Check if session is already tracked locally
                    let session_name = format!("commander-{}", project.name);
                    let already_tracked = self.sessions.contains_key(&project.name);

                    // Check if tmux session actually exists
                    let session_exists = self.tmux.as_ref()
                        .map(|t| t.session_exists(&session_name))
                        .unwrap_or(false);

                    if !already_tracked && !session_exists {
                        // Need to start the project
                        let tool_id = project
                            .config
                            .get("tool")
                            .and_then(|v| v.as_str())
                            .unwrap_or("claude-code");

                        println!("Starting '{}'...", project.name);
                        self.start_project_session(&project.name, &project.path, tool_id)?;
                    } else if !already_tracked && session_exists {
                        // Session exists but not tracked - just register it
                        self.sessions.insert(project.name.clone(), session_name);
                    }

                    info!(project = %project.name, "Connected to project");
                    self.connected_project = Some(project.name.clone());
                    println!("Connected to '{}'", project.name);
                } else {
                    println!("Project not found: {}", name);
                    println!(
                        "Use '/connect <path> -a <adapter> -n <name>' to create a new project."
                    );
                }
            }
        }
        Ok(())
    }

    /// Start a tmux session for a project and launch its adapter.
    /// If session already exists, just register it without recreating.
    fn start_project_session(
        &mut self,
        name: &str,
        path: &str,
        tool_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session_name = format!("commander-{}", name);

        if let Some(tmux) = &self.tmux {
            // Check if session already exists
            if tmux.session_exists(&session_name) {
                // Session exists - just register it and return
                self.sessions.insert(name.to_string(), session_name.clone());
                debug!(session = %session_name, "Reconnected to existing tmux session");
                return Ok(());
            }

            // Get adapter and its launch command
            if let Some(adapter) = self.registry.get(tool_id) {
                let (cmd, cmd_args) = adapter.launch_command(path);
                let full_cmd = if cmd_args.is_empty() {
                    cmd
                } else {
                    format!("{} {}", cmd, cmd_args.join(" "))
                };

                // Create tmux session
                match tmux.create_session(&session_name) {
                    Ok(_) => {
                        // Send command to start the AI tool
                        if let Err(e) = tmux.send_line(&session_name, None, &full_cmd) {
                            println!("Warning: Failed to launch adapter: {}", e);
                        } else {
                            // Track the session
                            self.sessions.insert(name.to_string(), session_name.clone());
                            debug!(session = %session_name, "tmux session created");
                        }
                    }
                    Err(e) => {
                        println!("Warning: Failed to create tmux session: {}", e);
                    }
                }
            }
        } else {
            println!("Note: Tmux not available. Project registered but not started in tmux.");
        }

        Ok(())
    }

    /// Stop a session: commit git changes and destroy tmux session.
    fn stop_session(&mut self, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let session_name = format!("commander-{}", name);

        // Find project path for git operations
        let project_path = {
            let projects = self.store.load_all_projects()?;
            projects.values()
                .find(|p| p.name == name)
                .map(|p| p.path.clone())
        };

        // Step 1: Commit any git changes
        if let Some(path) = &project_path {
            println!("Checking for uncommitted changes in {}...", path);

            match Self::git_commit_changes(path, name) {
                Ok(true) => println!("Changes committed."),
                Ok(false) => println!("No changes to commit."),
                Err(e) => println!("Git warning: {}", e),
            }
        }

        // Step 2: Destroy tmux session
        if let Some(tmux) = &self.tmux {
            match tmux.destroy_session(&session_name) {
                Ok(_) => {
                    println!("Session '{}' stopped.", name);

                    // Remove from tracking
                    self.sessions.remove(name);

                    // Disconnect if it was current
                    if self.connected_project.as_deref() == Some(name) {
                        self.connected_project = None;
                        println!("Disconnected.");
                    }
                }
                Err(e) => {
                    println!("Failed to stop session: {}", e);
                }
            }
        } else {
            println!("Tmux not available");
        }

        Ok(())
    }

    /// Commit any uncommitted git changes in the project directory.
    fn git_commit_changes(path: &str, project_name: &str) -> Result<bool, String> {
        use std::process::Command;

        // Check if there are changes
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to run git status: {}", e))?;

        let changes = String::from_utf8_lossy(&status.stdout);
        if changes.trim().is_empty() {
            return Ok(false); // No changes
        }

        // Stage all changes
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to stage changes: {}", e))?;

        // Commit with message
        let message = format!("WIP: Auto-commit from Commander session '{}'", project_name);
        let commit = Command::new("git")
            .args(["commit", "-m", &message])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to commit: {}", e))?;

        if commit.status.success() {
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&commit.stderr);
            Err(format!("Commit failed: {}", stderr))
        }
    }

    /// Handle chat message via OpenRouter.
    fn handle_chat(&mut self, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        print!("Thinking...");
        io::stdout().flush()?;

        let result = self.runtime.block_on(self.chat_client.send(message));

        // Clear "Thinking..." line
        print!("\r            \r");
        io::stdout().flush()?;

        match result {
            Ok(response) => {
                println!("{}", response);
            }
            Err(e) => {
                println!("Chat error: {}", e);
            }
        }

        Ok(())
    }
}

/// Find new lines in tmux output by comparing previous and current captures.
///
/// Uses a set-based approach to find lines that appear in the current output
/// but not in the previous output, filtering out echoed input and empty lines.
fn find_new_lines(prev: &str, current: &str, message: &str) -> Vec<String> {
    use std::collections::HashSet;

    let prev_lines: HashSet<&str> = prev.lines().collect();
    let mut new_lines = Vec::new();

    for line in current.lines() {
        let trimmed = line.trim();

        // Skip if line was in previous output
        if prev_lines.contains(line) || prev_lines.contains(trimmed) {
            continue;
        }

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Skip echoed input message
        if trimmed == message || trimmed.ends_with(&format!("> {}", message)) {
            continue;
        }

        // Skip prompt-only lines (common patterns)
        if is_prompt_line(trimmed) {
            continue;
        }

        new_lines.push(line.to_string());
    }

    new_lines
}

/// Check if a line is just a prompt (not actual output).
fn is_prompt_line(line: &str) -> bool {
    let trimmed = line.trim();

    // Common prompt patterns to skip
    let prompt_patterns = [
        "commander>",
        "commander [",
        ">",  // Single > at end often indicates prompt
        "$",  // Shell prompt
        "%",  // zsh prompt
    ];

    // Check if line is just a prompt
    for pattern in prompt_patterns {
        if trimmed == pattern || trimmed.ends_with(pattern) {
            // But not if it has substantial content before
            if trimmed.len() <= pattern.len() + 20 {
                return true;
            }
        }
    }

    false
}

/// Finds help for a command by name or alias.
fn find_command_help(name: &str) -> Option<&'static CommandHelp> {
    let name_lower = name.to_lowercase();
    COMMAND_HELP
        .iter()
        .find(|h| h.name == name_lower || h.aliases.contains(&name_lower.as_str()))
}

/// Prints help information.
fn print_help(topic: Option<&str>) {
    match topic {
        Some(cmd) => {
            // Show help for specific command
            if let Some(help) = find_command_help(cmd) {
                println!("/{} - {}", help.name, help.brief);
                if !help.aliases.is_empty() {
                    println!("Aliases: {}", help.aliases.join(", "));
                }
                println!();
                println!("{}", help.description);
                println!();
                println!("Usage:");
                for line in help.usage.lines() {
                    println!("  {}", line);
                }
                if !help.examples.is_empty() {
                    println!();
                    println!("Examples:");
                    for (example, desc) in help.examples {
                        println!("  {}  # {}", example, desc);
                    }
                }
            } else {
                println!(
                    "Unknown command: {}. Type /help for available commands.",
                    cmd
                );
            }
        }
        None => {
            // Show comprehensive help with both slash and conversational forms
            println!("Commander REPL - AI Project Orchestrator");
            println!();
            println!("COMMANDS:");
            println!();
            println!("  Connection:");
            println!("    /connect <path> -a <adapter> -n <name>   Start new project");
            println!("    /connect <name>                          Connect to existing project");
            println!("    /disconnect                              Disconnect from current project");
            println!("    connect to <name>                        (conversational)");
            println!("    connect <name>                           (conversational)");
            println!("    disconnect                               (conversational)");
            println!();
            println!("  Project Management:");
            println!("    /list                                    List all projects");
            println!("    /status [project]                        Show project status");
            println!("    /sessions                                List tmux sessions");
            println!("    /stop [session]                          Stop session (commits changes, ends tmux)");
            println!("    list, list projects, show projects       (conversational)");
            println!("    status, show status, status of <name>    (conversational)");
            println!();
            println!("  Communication:");
            println!("    <message>                                Send message to connected project");
            println!("    [disconnected] <message>                 Chat with AI (OpenRouter)");
            println!();
            println!("  Other:");
            println!("    /help [command], help, ?                 Show this help");
            println!("    /quit, quit, exit, bye                   Exit REPL");
            println!();
            println!("ADAPTERS:");
            println!("    cc, claude-code                          Claude Code CLI");
            println!("    mpm                                      Claude MPM");
            println!();
            println!("EXAMPLES:");
            println!("    /connect ~/code/myapp -a cc -n myapp");
            println!("    connect to myapp");
            println!("    list projects");
            println!("    how many files are in src/?");
            println!("    disconnect");
            println!();
            println!("Type /help <command> for detailed help on a specific command.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_list() {
        assert_eq!(ReplCommand::parse("/list"), ReplCommand::List);
        assert_eq!(ReplCommand::parse("/ls"), ReplCommand::List);
        assert_eq!(ReplCommand::parse("/l"), ReplCommand::List);
    }

    #[test]
    fn test_parse_status() {
        assert_eq!(ReplCommand::parse("/status"), ReplCommand::Status(None));
        assert_eq!(
            ReplCommand::parse("/status myproject"),
            ReplCommand::Status(Some("myproject".to_string()))
        );
    }

    #[test]
    fn test_parse_connect_existing() {
        assert_eq!(
            ReplCommand::parse("/connect myproject"),
            ReplCommand::Connect(ConnectTarget::Existing("myproject".to_string()))
        );
    }

    #[test]
    fn test_parse_connect_new() {
        let cmd = ReplCommand::parse("/connect ~/code/myapp -a cc -n myapp");
        match cmd {
            ReplCommand::Connect(ConnectTarget::New(args)) => {
                assert!(args.path.to_string_lossy().contains("code/myapp"));
                assert_eq!(args.tool, "cc");
                assert_eq!(args.alias, "myapp");
            }
            _ => panic!("Expected Connect(New), got {:?}", cmd),
        }
    }

    #[test]
    fn test_parse_connect_new_mpm() {
        let cmd = ReplCommand::parse("/connect /tmp/api -a mpm -n api-server");
        match cmd {
            ReplCommand::Connect(ConnectTarget::New(args)) => {
                assert_eq!(args.path, PathBuf::from("/tmp/api"));
                assert_eq!(args.tool, "mpm");
                assert_eq!(args.alias, "api-server");
            }
            _ => panic!("Expected Connect(New), got {:?}", cmd),
        }
    }

    #[test]
    fn test_parse_connect_missing_args() {
        assert!(matches!(
            ReplCommand::parse("/connect ~/code/myapp -a cc"),
            ReplCommand::Unknown(_)
        ));
        assert!(matches!(
            ReplCommand::parse("/connect ~/code/myapp -n myapp"),
            ReplCommand::Unknown(_)
        ));
    }

    #[test]
    fn test_parse_quit() {
        assert_eq!(ReplCommand::parse("/quit"), ReplCommand::Quit);
        assert_eq!(ReplCommand::parse("/q"), ReplCommand::Quit);
        assert_eq!(ReplCommand::parse("/exit"), ReplCommand::Quit);
    }

    #[test]
    fn test_parse_sessions() {
        assert_eq!(ReplCommand::parse("/sessions"), ReplCommand::Sessions);
    }

    #[test]
    fn test_parse_stop() {
        assert_eq!(ReplCommand::parse("/stop"), ReplCommand::Stop(None));
        assert_eq!(
            ReplCommand::parse("/stop duetto"),
            ReplCommand::Stop(Some("duetto".to_string()))
        );
        assert_eq!(
            ReplCommand::parse("/stop my-project"),
            ReplCommand::Stop(Some("my-project".to_string()))
        );
    }

    #[test]
    fn test_parse_help() {
        assert_eq!(ReplCommand::parse("/help"), ReplCommand::Help(None));
        assert_eq!(ReplCommand::parse("/?"), ReplCommand::Help(None));
        assert_eq!(
            ReplCommand::parse("/help connect"),
            ReplCommand::Help(Some("connect".to_string()))
        );
    }

    #[test]
    fn test_parse_text() {
        assert_eq!(
            ReplCommand::parse("hello world"),
            ReplCommand::Text("hello world".to_string())
        );
    }

    #[test]
    fn test_parse_empty() {
        assert_eq!(ReplCommand::parse(""), ReplCommand::Text(String::new()));
        assert_eq!(ReplCommand::parse("   "), ReplCommand::Text(String::new()));
    }

    #[test]
    fn test_parse_unknown() {
        assert!(matches!(
            ReplCommand::parse("/foobar"),
            ReplCommand::Unknown(_)
        ));
    }

    #[test]
    fn test_find_command_help() {
        assert!(find_command_help("connect").is_some());
        assert!(find_command_help("c").is_some()); // alias
        assert!(find_command_help("CONNECT").is_some()); // case insensitive
        assert!(find_command_help("notacommand").is_none());
    }

    #[test]
    fn test_connect_args_equality() {
        let args1 = ConnectArgs {
            path: PathBuf::from("/tmp/test"),
            tool: "cc".to_string(),
            alias: "test".to_string(),
        };
        let args2 = ConnectArgs {
            path: PathBuf::from("/tmp/test"),
            tool: "cc".to_string(),
            alias: "test".to_string(),
        };
        assert_eq!(args1, args2);
    }

    // Conversational command tests

    #[test]
    fn test_conversational_connect_to() {
        assert_eq!(
            ReplCommand::parse("connect to duetto"),
            ReplCommand::Connect(ConnectTarget::Existing("duetto".to_string()))
        );
        assert_eq!(
            ReplCommand::parse("Connect To MyProject"),
            ReplCommand::Connect(ConnectTarget::Existing("myproject".to_string()))
        );
    }

    #[test]
    fn test_conversational_connect() {
        assert_eq!(
            ReplCommand::parse("connect duetto"),
            ReplCommand::Connect(ConnectTarget::Existing("duetto".to_string()))
        );
    }

    #[test]
    fn test_conversational_list() {
        assert_eq!(ReplCommand::parse("list"), ReplCommand::List);
        assert_eq!(ReplCommand::parse("list projects"), ReplCommand::List);
        assert_eq!(ReplCommand::parse("show projects"), ReplCommand::List);
        assert_eq!(ReplCommand::parse("List Projects"), ReplCommand::List);
    }

    #[test]
    fn test_conversational_status() {
        assert_eq!(ReplCommand::parse("status"), ReplCommand::Status(None));
        assert_eq!(ReplCommand::parse("show status"), ReplCommand::Status(None));
        assert_eq!(
            ReplCommand::parse("status of myapp"),
            ReplCommand::Status(Some("myapp".to_string()))
        );
    }

    #[test]
    fn test_conversational_disconnect() {
        assert_eq!(ReplCommand::parse("disconnect"), ReplCommand::Disconnect);
        assert_eq!(
            ReplCommand::parse("disconnect from project"),
            ReplCommand::Disconnect
        );
    }

    #[test]
    fn test_conversational_help() {
        assert_eq!(ReplCommand::parse("help"), ReplCommand::Help(None));
        assert_eq!(ReplCommand::parse("show help"), ReplCommand::Help(None));
        assert_eq!(ReplCommand::parse("?"), ReplCommand::Help(None));
    }

    #[test]
    fn test_conversational_quit() {
        assert_eq!(ReplCommand::parse("quit"), ReplCommand::Quit);
        assert_eq!(ReplCommand::parse("exit"), ReplCommand::Quit);
        assert_eq!(ReplCommand::parse("bye"), ReplCommand::Quit);
    }

    #[test]
    fn test_conversational_case_insensitive() {
        assert_eq!(ReplCommand::parse("LIST"), ReplCommand::List);
        assert_eq!(ReplCommand::parse("DISCONNECT"), ReplCommand::Disconnect);
        assert_eq!(ReplCommand::parse("Help"), ReplCommand::Help(None));
        assert_eq!(ReplCommand::parse("QUIT"), ReplCommand::Quit);
    }

    #[test]
    fn test_conversational_not_matching_partial() {
        // "listing" should not match "list"
        assert_eq!(
            ReplCommand::parse("listing"),
            ReplCommand::Text("listing".to_string())
        );
        // "helper" should not match "help"
        assert_eq!(
            ReplCommand::parse("helper"),
            ReplCommand::Text("helper".to_string())
        );
    }

    // Tests for find_new_lines helper
    #[test]
    fn test_find_new_lines_basic() {
        let prev = "line1\nline2\n";
        let current = "line1\nline2\nline3\n";
        let message = "test message";

        let new_lines = super::find_new_lines(prev, current, message);
        assert_eq!(new_lines, vec!["line3"]);
    }

    #[test]
    fn test_find_new_lines_filters_message() {
        let prev = "line1\n";
        let current = "line1\ntest message\nresponse\n";
        let message = "test message";

        let new_lines = super::find_new_lines(prev, current, message);
        assert_eq!(new_lines, vec!["response"]);
    }

    #[test]
    fn test_find_new_lines_filters_echoed_input() {
        let prev = "line1\n";
        let current = "line1\n[project] > test message\nAI response\n";
        let message = "test message";

        let new_lines = super::find_new_lines(prev, current, message);
        assert_eq!(new_lines, vec!["AI response"]);
    }

    #[test]
    fn test_find_new_lines_skips_empty() {
        let prev = "line1\n";
        let current = "line1\n\n  \nresponse\n";
        let message = "test";

        let new_lines = super::find_new_lines(prev, current, message);
        assert_eq!(new_lines, vec!["response"]);
    }

    #[test]
    fn test_is_prompt_line() {
        assert!(super::is_prompt_line("commander>"));
        assert!(super::is_prompt_line("commander [duetto]>"));
        assert!(super::is_prompt_line("$"));
        assert!(super::is_prompt_line("%"));
        assert!(!super::is_prompt_line("This is actual output from the AI"));
        assert!(!super::is_prompt_line("The answer is 42"));
    }
}
