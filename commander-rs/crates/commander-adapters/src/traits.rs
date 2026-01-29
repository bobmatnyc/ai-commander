//! Core traits for runtime adapters.
//!
//! The `RuntimeAdapter` trait defines the interface that all AI coding tool
//! adapters must implement. This allows Commander to work with different
//! tools (Claude Code, Aider, Codex, etc.) through a unified interface.

use std::collections::HashMap;

/// The state of a runtime instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    /// Instance is starting up.
    Starting,
    /// Instance is ready and waiting for input.
    Idle,
    /// Instance is actively processing.
    Working,
    /// Instance encountered an error.
    Error,
    /// Instance has stopped.
    Stopped,
}

/// Information about a runtime adapter.
#[derive(Debug, Clone)]
pub struct AdapterInfo {
    /// Unique identifier for this adapter type.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the adapter.
    pub description: String,
    /// Command to launch the runtime.
    pub command: String,
    /// Default arguments for the command.
    pub default_args: Vec<String>,
}

/// Result of analyzing runtime output.
#[derive(Debug, Clone)]
pub struct OutputAnalysis {
    /// Detected state of the runtime.
    pub state: RuntimeState,
    /// Confidence in the state detection (0.0 - 1.0).
    pub confidence: f32,
    /// Any detected errors.
    pub errors: Vec<String>,
    /// Extracted structured data.
    pub data: HashMap<String, String>,
}

/// Trait for runtime adapters.
///
/// Each adapter knows how to:
/// - Launch its runtime (command and arguments)
/// - Detect when the runtime is idle/working/error
/// - Parse output to extract events and status
///
/// # Example
///
/// ```ignore
/// use commander_adapters::{RuntimeAdapter, RuntimeState};
///
/// fn check_runtime(adapter: &dyn RuntimeAdapter, output: &str) {
///     let analysis = adapter.analyze_output(output);
///     match analysis.state {
///         RuntimeState::Idle => println!("Ready for input!"),
///         RuntimeState::Working => println!("Still processing..."),
///         RuntimeState::Error => println!("Error: {:?}", analysis.errors),
///         _ => {}
///     }
/// }
/// ```
pub trait RuntimeAdapter: Send + Sync {
    /// Returns information about this adapter.
    fn info(&self) -> &AdapterInfo;

    /// Returns the command to launch this runtime.
    fn launch_command(&self, project_path: &str) -> (String, Vec<String>);

    /// Analyzes output to determine runtime state.
    fn analyze_output(&self, output: &str) -> OutputAnalysis;

    /// Checks if the given output indicates the runtime is idle.
    fn is_idle(&self, output: &str) -> bool {
        self.analyze_output(output).state == RuntimeState::Idle
    }

    /// Checks if the given output indicates an error.
    fn is_error(&self, output: &str) -> bool {
        self.analyze_output(output).state == RuntimeState::Error
    }

    /// Formats a message to send to the runtime.
    fn format_message(&self, message: &str) -> String {
        message.to_string()
    }

    /// Returns patterns that indicate the runtime is idle.
    fn idle_patterns(&self) -> &[&str];

    /// Returns patterns that indicate an error.
    fn error_patterns(&self) -> &[&str];
}
