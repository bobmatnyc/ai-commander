//! Claude Code runtime adapter.

use std::collections::HashMap;

use crate::patterns::{self, claude_code as cc_patterns};
use crate::traits::{AdapterInfo, OutputAnalysis, RuntimeAdapter, RuntimeState};

/// Adapter for Claude Code CLI.
pub struct ClaudeCodeAdapter {
    info: AdapterInfo,
}

impl ClaudeCodeAdapter {
    /// Creates a new Claude Code adapter.
    pub fn new() -> Self {
        Self {
            info: AdapterInfo {
                id: "claude-code".to_string(),
                name: "Claude Code".to_string(),
                description: "Anthropic's Claude Code CLI for AI-assisted coding".to_string(),
                command: "claude".to_string(),
                default_args: vec!["--dangerously-skip-permissions".to_string()],
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
        if patterns::any_match(&recent, cc_patterns::error_patterns()) {
            return RuntimeState::Error;
        }

        // Check for idle state
        if patterns::any_match(&recent, cc_patterns::idle_patterns()) {
            return RuntimeState::Idle;
        }

        // Check for working state
        if patterns::any_match(&recent, cc_patterns::working_patterns()) {
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
        let patterns = cc_patterns::error_patterns();

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

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeAdapter for ClaudeCodeAdapter {
    fn info(&self) -> &AdapterInfo {
        &self.info
    }

    fn launch_command(&self, project_path: &str) -> (String, Vec<String>) {
        let mut args = self.info.default_args.clone();
        args.push("--project".to_string());
        args.push(project_path.to_string());
        (self.info.command.clone(), args)
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
                patterns::best_match(output, cc_patterns::idle_patterns())
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
        &[r"^>\s*$", r"(?i)waiting for input", r"\[IDLE\]"]
    }

    fn error_patterns(&self) -> &[&str] {
        &[r"(?i)^error:", r"(?i)exception", r"(?i)failed"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_info() {
        let adapter = ClaudeCodeAdapter::new();
        assert_eq!(adapter.info().id, "claude-code");
        assert_eq!(adapter.info().command, "claude");
    }

    #[test]
    fn test_launch_command() {
        let adapter = ClaudeCodeAdapter::new();
        let (cmd, args) = adapter.launch_command("/path/to/project");

        assert_eq!(cmd, "claude");
        assert!(args.contains(&"--project".to_string()));
        assert!(args.contains(&"/path/to/project".to_string()));
    }

    #[test]
    fn test_analyze_idle_output() {
        let adapter = ClaudeCodeAdapter::new();
        let output = "Done!\n> ";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Idle);
        assert!(analysis.confidence > 0.8);
    }

    #[test]
    fn test_analyze_error_output() {
        let adapter = ClaudeCodeAdapter::new();
        let output = "Processing...\nError: Permission denied\n";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Error);
        assert!(!analysis.errors.is_empty());
        assert!(analysis.errors[0].contains("Permission denied"));
    }

    #[test]
    fn test_analyze_working_output() {
        let adapter = ClaudeCodeAdapter::new();
        let output = "Thinking about your request...\nAnalyzing the codebase...";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Working);
    }

    #[test]
    fn test_is_idle() {
        let adapter = ClaudeCodeAdapter::new();
        assert!(adapter.is_idle("> "));
        assert!(adapter.is_idle("[IDLE]"));
        assert!(!adapter.is_idle("Processing..."));
    }

    #[test]
    fn test_is_error() {
        let adapter = ClaudeCodeAdapter::new();
        assert!(adapter.is_error("Error: something failed"));
        assert!(!adapter.is_error("All good!"));
    }
}
