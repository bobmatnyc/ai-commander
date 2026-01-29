//! Interactive REPL (Read-Eval-Print Loop) for Commander.

use std::path::Path;

use commander_persistence::StateStore;
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RlResult};
use tracing::{debug, info};

/// Slash commands available in the REPL.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplCommand {
    /// List all projects
    List,
    /// Show status of current or specified project
    Status(Option<String>),
    /// Connect to a project
    Connect(String),
    /// Disconnect from current project
    Disconnect,
    /// Send message to connected project
    Send(String),
    /// Show help
    Help,
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
        if input.starts_with('/') {
            let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
            let cmd = parts[0].to_lowercase();
            let arg = parts.get(1).map(|s| s.trim().to_string());

            match cmd.as_str() {
                "list" | "ls" | "l" => ReplCommand::List,
                "status" | "s" => ReplCommand::Status(arg),
                "connect" | "c" => arg
                    .map(ReplCommand::Connect)
                    .unwrap_or(ReplCommand::Unknown("connect requires a project".to_string())),
                "disconnect" | "dc" => ReplCommand::Disconnect,
                "send" => arg
                    .map(ReplCommand::Send)
                    .unwrap_or(ReplCommand::Unknown("send requires a message".to_string())),
                "help" | "h" | "?" => ReplCommand::Help,
                "quit" | "q" | "exit" => ReplCommand::Quit,
                _ => ReplCommand::Unknown(cmd),
            }
        } else if input.starts_with('@') {
            // @mention syntax - treat as send to project
            let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
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
}

/// REPL state
pub struct Repl {
    editor: DefaultEditor,
    store: StateStore,
    connected_project: Option<String>,
    history_path: Option<std::path::PathBuf>,
}

impl Repl {
    /// Creates a new REPL instance.
    pub fn new(state_dir: &Path) -> RlResult<Self> {
        let mut editor = DefaultEditor::new()?;
        let store = StateStore::new(state_dir);

        // Set up history file
        let history_path = state_dir.join("repl_history.txt");
        if history_path.exists() {
            let _ = editor.load_history(&history_path);
        }

        Ok(Self {
            editor,
            store,
            connected_project: None,
            history_path: Some(history_path),
        })
    }

    /// Runs the REPL loop.
    pub fn run(&mut self) -> RlResult<()> {
        println!("Commander REPL v{}", env!("CARGO_PKG_VERSION"));
        println!("Type /help for commands, /quit to exit\n");

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
            Some(project) => format!("commander ({})> ", project),
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
                }
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

            ReplCommand::Help => {
                print_help();
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
                    } else {
                        println!("Not connected. Use /connect <project> or prefix with @project");
                    }
                }
                Ok(false)
            }
        }
    }
}

fn print_help() {
    println!("Commander REPL Commands:");
    println!();
    println!("  /list, /ls, /l      List all projects");
    println!("  /status [project]   Show project status");
    println!("  /connect <project>  Connect to a project");
    println!("  /disconnect, /dc    Disconnect from current project");
    println!("  /send <message>     Send message to connected project");
    println!("  /help, /h, /?       Show this help");
    println!("  /quit, /q, /exit    Exit the REPL");
    println!();
    println!("When connected, type messages directly to send to the project.");
    println!("Use @project message to send to a specific project.");
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
    fn test_parse_connect() {
        assert_eq!(
            ReplCommand::parse("/connect myproject"),
            ReplCommand::Connect("myproject".to_string())
        );
    }

    #[test]
    fn test_parse_quit() {
        assert_eq!(ReplCommand::parse("/quit"), ReplCommand::Quit);
        assert_eq!(ReplCommand::parse("/q"), ReplCommand::Quit);
        assert_eq!(ReplCommand::parse("/exit"), ReplCommand::Quit);
    }

    #[test]
    fn test_parse_help() {
        assert_eq!(ReplCommand::parse("/help"), ReplCommand::Help);
        assert_eq!(ReplCommand::parse("/?"), ReplCommand::Help);
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
}
