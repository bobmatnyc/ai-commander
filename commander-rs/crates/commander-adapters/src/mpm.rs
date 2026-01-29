//! MPM (Multi-Project Manager) runtime adapter.

use std::collections::HashMap;

use crate::patterns::{self, mpm as mpm_patterns};
use crate::traits::{AdapterInfo, OutputAnalysis, RuntimeAdapter, RuntimeState};

/// Adapter for MPM CLI.
pub struct MpmAdapter {
    info: AdapterInfo,
}

impl MpmAdapter {
    /// Creates a new MPM adapter.
    pub fn new() -> Self {
        Self {
            info: AdapterInfo {
                id: "mpm".to_string(),
                name: "MPM".to_string(),
                description: "Multi-Project Manager for coordinating AI agents".to_string(),
                command: "claude-mpm".to_string(),
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
        if patterns::any_match(&recent, mpm_patterns::error_patterns()) {
            return RuntimeState::Error;
        }

        // Check for idle state
        if patterns::any_match(&recent, mpm_patterns::idle_patterns()) {
            return RuntimeState::Idle;
        }

        // Check for working state
        if patterns::any_match(&recent, mpm_patterns::working_patterns()) {
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
        let patterns = mpm_patterns::error_patterns();

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

impl Default for MpmAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeAdapter for MpmAdapter {
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
                patterns::best_match(output, mpm_patterns::idle_patterns())
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
        &[r"(?i)PM ready", r"(?i)awaiting instructions", r"\[IDLE\]"]
    }

    fn error_patterns(&self) -> &[&str] {
        &[r"(?i)^error:", r"(?i)exception", r"(?i)agent.*error"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_info() {
        let adapter = MpmAdapter::new();
        assert_eq!(adapter.info().id, "mpm");
        assert_eq!(adapter.info().command, "claude-mpm");
    }

    #[test]
    fn test_launch_command() {
        let adapter = MpmAdapter::new();
        let (cmd, args) = adapter.launch_command("/path/to/project");

        assert_eq!(cmd, "claude-mpm");
        assert!(args.contains(&"--project".to_string()));
        assert!(args.contains(&"/path/to/project".to_string()));
    }

    #[test]
    fn test_analyze_idle_output() {
        let adapter = MpmAdapter::new();
        let output = "Tasks complete!\nPM ready";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Idle);
        assert!(analysis.confidence > 0.8);
    }

    #[test]
    fn test_analyze_error_output() {
        let adapter = MpmAdapter::new();
        let output = "Delegating task...\nError: agent failed to respond\n";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Error);
        assert!(!analysis.errors.is_empty());
    }

    #[test]
    fn test_analyze_working_output() {
        let adapter = MpmAdapter::new();
        let output = "Delegating task to agent-1...\nCoordinating responses...";
        let analysis = adapter.analyze_output(output);

        assert_eq!(analysis.state, RuntimeState::Working);
    }

    #[test]
    fn test_is_idle() {
        let adapter = MpmAdapter::new();
        assert!(adapter.is_idle("PM ready"));
        assert!(adapter.is_idle("Awaiting instructions"));
        assert!(!adapter.is_idle("Delegating..."));
    }

    #[test]
    fn test_is_error() {
        let adapter = MpmAdapter::new();
        assert!(adapter.is_error("Error: something failed"));
        assert!(adapter.is_error("Agent error occurred"));
        assert!(!adapter.is_error("All good!"));
    }
}
