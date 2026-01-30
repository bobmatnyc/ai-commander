//! Interactive REPL (Read-Eval-Print Loop) for Commander.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use commander_adapters::AdapterRegistry;
use commander_models::Project;
use commander_persistence::StateStore;
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
        name: "start",
        aliases: &[],
        brief: "Start a new project and connect to it",
        description: "Creates a new project with the specified adapter and connects to it immediately.\n\n\
                      Adapter aliases: cc = claude-code, mpm = mpm",
        usage: "/start <path> -a <adapter> -n <name>",
        examples: &[
            ("/start ~/code/myapp -a cc -n myapp", "Start project with claude-code"),
            ("/start ~/code/api -a mpm -n api-server", "Start project with mpm adapter"),
        ],
    },
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
        brief: "Connect to a project",
        description: "Connect to an existing project by name, or create and connect to a new project.\n\n\
                      New syntax creates project from path with specified tool adapter.\n\
                      Tool aliases: cc = claude-code, mpm = mpm",
        usage: "/connect <path> <tool> <alias>  (new)\n       /connect <project-name>       (legacy)",
        examples: &[
            ("/connect ~/code/myapp cc myapp", "Create project with claude-code"),
            ("/connect ~/code/api mpm api-server", "Create project with mpm adapter"),
            ("/connect myapp", "Connect to existing 'myapp' project"),
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
    /// Start a new project and connect to it
    Start(ConnectArgs),
    /// List all projects
    List,
    /// Show status of current or specified project
    Status(Option<String>),
    /// Connect to a project (legacy: by name)
    Connect(String),
    /// Connect with new syntax: path, tool, alias
    ConnectNew(ConnectArgs),
    /// Disconnect from current project
    Disconnect,
    /// Send message to connected project
    Send(String),
    /// Show help (optionally for a specific command)
    Help(Option<String>),
    /// Quit the REPL
    Quit,
    /// Unknown command
    Unknown(String),
    /// Plain text (not a command)
    Text(String),
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
                "start" => Self::parse_start(arg),
                "list" | "ls" | "l" => ReplCommand::List,
                "status" | "s" => ReplCommand::Status(arg),
                "connect" | "c" => Self::parse_connect(arg),
                "disconnect" | "dc" => ReplCommand::Disconnect,
                "send" => arg
                    .map(ReplCommand::Send)
                    .unwrap_or(ReplCommand::Unknown("send requires a message".to_string())),
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
            ReplCommand::Text(input.to_string())
        }
    }

    /// Parse start command arguments: /start <path> -a <adapter> -n <name>
    fn parse_start(arg: Option<String>) -> Self {
        let Some(arg) = arg else {
            return ReplCommand::Unknown("start requires: /start <path> -a <adapter> -n <name>".to_string());
        };

        let parts: Vec<&str> = arg.split_whitespace().collect();
        if parts.len() < 5 {
            return ReplCommand::Unknown("start requires: /start <path> -a <adapter> -n <name>".to_string());
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
                ReplCommand::Start(ConnectArgs { path, tool, alias })
            }
            (None, _) => ReplCommand::Unknown("start requires -a <adapter>".to_string()),
            (_, None) => ReplCommand::Unknown("start requires -n <name>".to_string()),
        }
    }

    /// Parse connect command arguments.
    fn parse_connect(arg: Option<String>) -> Self {
        let Some(arg) = arg else {
            return ReplCommand::Unknown("connect requires arguments".to_string());
        };

        let parts: Vec<&str> = arg.split_whitespace().collect();

        match parts.len() {
            // Legacy: /connect project-name
            1 => ReplCommand::Connect(parts[0].to_string()),
            // New: /connect <path> <tool> <alias>
            3 => {
                let path = PathBuf::from(shellexpand::tilde(parts[0]).to_string());
                let tool = parts[1].to_string();
                let alias = parts[2].to_string();
                ReplCommand::ConnectNew(ConnectArgs { path, tool, alias })
            }
            _ => ReplCommand::Unknown(
                "connect: use '/connect <path> <tool> <alias>' or '/connect <project>'".to_string(),
            ),
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
}

impl Repl {
    /// Creates a new REPL instance.
    pub fn new(state_dir: &Path) -> RlResult<Self> {
        let mut editor = DefaultEditor::new()?;
        let store = StateStore::new(state_dir);
        let registry = AdapterRegistry::new();
        let chat_client = ChatClient::new();

        // Create tokio runtime for async operations
        let runtime = tokio::runtime::Runtime::new()
            .expect("Failed to create tokio runtime");

        // Set up history file
        let history_path = state_dir.join("repl_history.txt");
        if history_path.exists() {
            let _ = editor.load_history(&history_path);
        }

        Ok(Self {
            editor,
            store,
            registry,
            connected_project: None,
            history_path: Some(history_path),
            chat_client,
            runtime,
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
                        Ok(true) => break,  // Quit requested
                        Ok(false) => {}     // Continue
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
            ReplCommand::Start(args) => {
                // Resolve tool alias
                let tool_id = match self.registry.resolve(&args.tool) {
                    Some(id) => id.to_string(),
                    None => {
                        println!("Unknown adapter: {}. Available: cc (claude-code), mpm", args.tool);
                        return Ok(false);
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
                if projects.values().any(|p| p.name == args.alias) {
                    println!("Project '{}' already exists. Use '/connect {}' to connect.", args.alias, args.alias);
                    return Ok(false);
                }

                // Create new project
                let mut project = Project::new(&path_str, &args.alias);
                project.config.insert("tool".to_string(), serde_json::json!(tool_id));

                // Save project
                self.store.save_project(&project)?;

                info!(
                    project = %args.alias,
                    path = %path_str,
                    tool = %tool_id,
                    "Started and connected to project"
                );

                self.connected_project = Some(args.alias.clone());
                println!("Started and connected to '{}'", args.alias);
                Ok(false)
            }

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
                        println!("No project connected. Use /connect <project> or /status <project>")
                    }
                }
                Ok(false)
            }

            ReplCommand::Connect(project) => {
                let projects = self.store.load_all_projects()?;
                if projects
                    .values()
                    .any(|p| p.name == project || p.id.as_str() == project)
                {
                    info!(project = %project, "Connected to project");
                    self.connected_project = Some(project.clone());
                    println!("Connected to '{}'", project);
                } else {
                    println!("Project not found: {}", project);
                    println!("Use '/connect <path> <tool> <alias>' to create a new project.");
                }
                Ok(false)
            }

            ReplCommand::ConnectNew(args) => {
                // Resolve tool alias
                let tool_id = match self.registry.resolve(&args.tool) {
                    Some(id) => id.to_string(),
                    None => {
                        println!("Unknown tool: {}. Available: cc (claude-code), mpm", args.tool);
                        return Ok(false);
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
                if projects.values().any(|p| p.name == args.alias) {
                    println!("Project '{}' already exists. Use '/connect {}' to connect.", args.alias, args.alias);
                    return Ok(false);
                }

                // Create new project
                let mut project = Project::new(&path_str, &args.alias);
                project.config.insert("tool".to_string(), serde_json::json!(tool_id));

                // Save project
                self.store.save_project(&project)?;

                info!(
                    project = %args.alias,
                    path = %path_str,
                    tool = %tool_id,
                    "Created and connected to project"
                );

                self.connected_project = Some(args.alias.clone());
                println!("Created project '{}' at {} with {}", args.alias, path_str, tool_id);
                println!("Connected to '{}'", args.alias);
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
                        println!("[{}] {}", project, message);
                        println!("(Message sending will be implemented in Phase 7)");
                    }
                    None => {
                        println!("Not connected to any project. Use /connect <project> first.");
                    }
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

/// Finds help for a command by name or alias.
fn find_command_help(name: &str) -> Option<&'static CommandHelp> {
    let name_lower = name.to_lowercase();
    COMMAND_HELP.iter().find(|h| {
        h.name == name_lower || h.aliases.contains(&name_lower.as_str())
    })
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
                println!("Unknown command: {}. Type /help for available commands.", cmd);
            }
        }
        None => {
            // Show overview of all commands
            println!("Commander REPL Commands:");
            println!();
            for help in COMMAND_HELP {
                let aliases = if help.aliases.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", help.aliases.join(", "))
                };
                println!("  /{}{:<12} {}", help.name, aliases, help.brief);
            }
            println!();
            println!("Type /help <command> for detailed help on a specific command.");
            println!();
            println!("When connected, type messages directly to send to the project.");
            if std::env::var("OPENROUTER_API_KEY").is_ok() {
                println!("When disconnected, messages are sent to chat (OpenRouter).");
            }
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
    fn test_parse_connect_legacy() {
        assert_eq!(
            ReplCommand::parse("/connect myproject"),
            ReplCommand::Connect("myproject".to_string())
        );
    }

    #[test]
    fn test_parse_start() {
        let cmd = ReplCommand::parse("/start ~/code/myapp -a cc -n myapp");
        match cmd {
            ReplCommand::Start(args) => {
                assert!(args.path.to_string_lossy().contains("code/myapp"));
                assert_eq!(args.tool, "cc");
                assert_eq!(args.alias, "myapp");
            }
            _ => panic!("Expected Start, got {:?}", cmd),
        }
    }

    #[test]
    fn test_parse_start_mpm() {
        let cmd = ReplCommand::parse("/start /tmp/api -a mpm -n api-server");
        match cmd {
            ReplCommand::Start(args) => {
                assert_eq!(args.path, PathBuf::from("/tmp/api"));
                assert_eq!(args.tool, "mpm");
                assert_eq!(args.alias, "api-server");
            }
            _ => panic!("Expected Start, got {:?}", cmd),
        }
    }

    #[test]
    fn test_parse_start_missing_args() {
        assert!(matches!(
            ReplCommand::parse("/start ~/code/myapp"),
            ReplCommand::Unknown(_)
        ));
        assert!(matches!(
            ReplCommand::parse("/start ~/code/myapp -a cc"),
            ReplCommand::Unknown(_)
        ));
        assert!(matches!(
            ReplCommand::parse("/start ~/code/myapp -n myapp"),
            ReplCommand::Unknown(_)
        ));
    }

    #[test]
    fn test_parse_connect_new() {
        let cmd = ReplCommand::parse("/connect ~/code/myapp cc myapp");
        match cmd {
            ReplCommand::ConnectNew(args) => {
                assert!(args.path.to_string_lossy().contains("code/myapp"));
                assert_eq!(args.tool, "cc");
                assert_eq!(args.alias, "myapp");
            }
            _ => panic!("Expected ConnectNew, got {:?}", cmd),
        }
    }

    #[test]
    fn test_parse_connect_new_mpm() {
        let cmd = ReplCommand::parse("/connect /tmp/api mpm api-server");
        match cmd {
            ReplCommand::ConnectNew(args) => {
                assert_eq!(args.path, PathBuf::from("/tmp/api"));
                assert_eq!(args.tool, "mpm");
                assert_eq!(args.alias, "api-server");
            }
            _ => panic!("Expected ConnectNew, got {:?}", cmd),
        }
    }

    #[test]
    fn test_parse_quit() {
        assert_eq!(ReplCommand::parse("/quit"), ReplCommand::Quit);
        assert_eq!(ReplCommand::parse("/q"), ReplCommand::Quit);
        assert_eq!(ReplCommand::parse("/exit"), ReplCommand::Quit);
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
        assert!(find_command_help("start").is_some());
        assert!(find_command_help("connect").is_some());
        assert!(find_command_help("c").is_some());  // alias
        assert!(find_command_help("CONNECT").is_some());  // case insensitive
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
}
