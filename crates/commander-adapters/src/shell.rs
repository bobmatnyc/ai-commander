//! Generic shell runtime adapter.
//!
//! This adapter allows connecting to arbitrary shell sessions (bash, zsh, etc.)
//! and detecting common shell prompts and error messages.

use std::collections::HashMap;
use std::env;

use crate::patterns::{self, shell as shell_patterns};
use crate::traits::{AdapterInfo, OutputAnalysis, RuntimeAdapter, RuntimeState};

/// Adapter for generic shell sessions.
///
/// This adapter detects common shell prompts ($ # % >) and error messages,
/// making it suitable for connecting to arbitrary shell sessions via tmux.
pub struct ShellAdapter {
    info: AdapterInfo,
}

impl ShellAdapter {
    /// Creates a new shell adapter using the user's default shell.
    pub fn new() -> Self {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        Self {
            info: AdapterInfo {
                id: "shell".to_string(),
                name: "Shell".to_string(),
                description: "Generic shell adapter for arbitrary shell sessions".to_string(),
                command: shell,
                default_args: vec![],
            },
        }
    }

    /// Creates a shell adapter with a specific shell command.
    pub fn with_shell(shell: &str) -> Self {
        Self {
            info: AdapterInfo {
                id: "shell".to_string(),
                name: "Shell".to_string(),
                description: "Generic shell adapter for arbitrary shell sessions".to_string(),
                command: shell.to_string(),
                default_args: vec![],
            },
        }
    }

    /// Analyzes the last N lines of output for state detection.
    fn analyze_recent_output(&self, output: &str, lines: usize) -> RuntimeState {
        let recent: String = output
            .lines()
            .rev()
            .take(lines)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");

        // Check for errors first (highest priority)
        if patterns::any_match(&recent, shell_patterns::error_patterns()) {
            return RuntimeState::Error;
        }

        // Check for idle state (shell prompt)
        if patterns::any_match(&recent, shell_patterns::idle_patterns()) {
            return RuntimeState::Idle;
        }

        // Check for working state
        if patterns::any_match(&recent, shell_patterns::working_patterns()) {
            return RuntimeState::Working;
        }

        // Default to working if we have output but no clear state
        if !recent.trim().is_empty() {
            RuntimeState::Working
        } else {
            RuntimeState::Starting
        }
    }

    /// Extracts error messages from output.
    fn extract_errors(&self, output: &str) -> Vec<String> {
        let mut errors = Vec::new();
        let patterns = shell_patterns::error_patterns();

        for line in output.lines() {
            for pattern in patterns {
                if pattern.matches(line) {
                    errors.push(line.trim().to_string());
                    break;
                }
            }
        }

        errors
    }
}

impl Default for ShellAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeAdapter for ShellAdapter {
    fn info(&self) -> &AdapterInfo {
        &self.info
    }

    fn launch_command(&self, _project_path: &str) -> (String, Vec<String>) {
        // Shell doesn't need project path - it just launches a shell
        (self.info.command.clone(), self.info.default_args.clone())
    }

    fn analyze_output(&self, output: &str) -> OutputAnalysis {
        let state = self.analyze_recent_output(output, 10);
        let errors = if state == RuntimeState::Error {
            self.extract_errors(output)
        } else {
            Vec::new()
        };

        // Calculate confidence based on pattern matches
        let confidence = match state {
            RuntimeState::Error => 0.95,
            RuntimeState::Idle => {
                patterns::best_match(output, shell_patterns::idle_patterns())
                    .map(|p| p.confidence)
                    .unwrap_or(0.5)
            }
            RuntimeState::Working => 0.7,
            RuntimeState::Starting => 0.5,
            RuntimeState::Stopped => 1.0,
        };

        OutputAnalysis {
            state,
            confidence,
            errors,
            data: HashMap::new(),
        }
    }

    fn idle_patterns(&self) -> &[&str] {
        &[
            r"[$#%>]\s*$",                  // Common shell prompts
            r"\w+[@:~][^$#%>]*[$#%>]\s*$", // PS1 with user/host
            r"^\s*\$\s*$",                  // Just $
        ]
    }

    fn error_patterns(&self) -> &[&str] {
        &[
            r"(?i)command not found",
            r"(?i)no such file or directory",
            r"(?i)permission denied",
            r"(?i)syntax error",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_info() {
        let adapter = ShellAdapter::new();
        assert_eq!(adapter.info().id, "shell");
        assert!(adapter.info().name.contains("Shell"));
    }

    #[test]
    fn test_with_shell() {
        let adapter = ShellAdapter::with_shell("/bin/zsh");
        assert_eq!(adapter.info().command, "/bin/zsh");
    }

    #[test]
    fn test_launch_command() {
        let adapter = ShellAdapter::with_shell("/bin/bash");
        let (cmd, args) = adapter.launch_command("/path/to/project");

        assert_eq!(cmd, "/bin/bash");
        assert!(args.is_empty());
    }

    #[test]
    fn test_analyze_idle_bash_prompt() {
        let adapter = ShellAdapter::new();
        let output = "output\n$ ";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Idle);
        assert!(analysis.confidence > 0.8);
    }

    #[test]
    fn test_analyze_idle_zsh_prompt() {
        let adapter = ShellAdapter::new();
        let output = "output\n% ";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Idle);
    }

    #[test]
    fn test_analyze_idle_root_prompt() {
        let adapter = ShellAdapter::new();
        let output = "output\n# ";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Idle);
    }

    #[test]
    fn test_analyze_idle_ps1_prompt() {
        let adapter = ShellAdapter::new();
        let output = "output\nuser@hostname:~/projects$ ";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Idle);
    }

    #[test]
    fn test_analyze_error_command_not_found() {
        let adapter = ShellAdapter::new();
        let output = "bash: foo: command not found\n$ ";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Error);
        assert!(!analysis.errors.is_empty());
        assert!(analysis.errors[0].contains("command not found"));
    }

    #[test]
    fn test_analyze_error_no_such_file() {
        let adapter = ShellAdapter::new();
        let output = "cat: file.txt: No such file or directory\n$ ";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Error);
        assert!(!analysis.errors.is_empty());
    }

    #[test]
    fn test_analyze_error_permission_denied() {
        let adapter = ShellAdapter::new();
        let output = "bash: /root/file: Permission denied\n$ ";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Error);
    }

    #[test]
    fn test_analyze_error_syntax_error() {
        let adapter = ShellAdapter::new();
        let output = "bash: syntax error near unexpected token\n$ ";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Error);
    }

    #[test]
    fn test_analyze_working_output() {
        let adapter = ShellAdapter::new();
        let output = "Compiling project...\nBuilding...";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Working);
    }

    #[test]
    fn test_is_idle() {
        let adapter = ShellAdapter::new();
        assert!(adapter.is_idle("$ "));
        assert!(adapter.is_idle("% "));
        assert!(adapter.is_idle("# "));
        assert!(adapter.is_idle("user@host:~$ "));
        assert!(!adapter.is_idle("Processing..."));
    }

    #[test]
    fn test_is_error() {
        let adapter = ShellAdapter::new();
        assert!(adapter.is_error("command not found"));
        assert!(adapter.is_error("No such file or directory"));
        assert!(adapter.is_error("Permission denied"));
        assert!(!adapter.is_error("All good!"));
    }
}
