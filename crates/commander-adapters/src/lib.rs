//! Runtime adapters for AI coding tools.
//!
//! This crate provides a unified interface for working with different
//! AI coding assistants (Claude Code, Aider, Codex, etc.) through the
//! `RuntimeAdapter` trait.
//!
//! # Key Concepts
//!
//! - **RuntimeAdapter**: Trait that all adapters implement
//! - **AdapterRegistry**: Discovers and manages available adapters
//! - **Pattern matching**: Detects idle/error/working states from output
//!
//! # Example
//!
//! ```
//! use commander_adapters::{AdapterRegistry, RuntimeAdapter, RuntimeState};
//!
//! // Get an adapter from the registry
//! let registry = AdapterRegistry::new();
//! let adapter = registry.get("claude-code").unwrap();
//!
//! // Check if output indicates idle state
//! let output = "> ";
//! if adapter.is_idle(output) {
//!     println!("Runtime is ready for input!");
//! }
//!
//! // Get detailed analysis
//! let analysis = adapter.analyze_output(output);
//! println!("State: {:?}, Confidence: {}", analysis.state, analysis.confidence);
//! ```

pub mod claude_code;
pub mod mpm;
pub mod patterns;
pub mod registry;
pub mod shell;
pub mod traits;

pub use claude_code::ClaudeCodeAdapter;
pub use mpm::MpmAdapter;
pub use patterns::Pattern;
pub use registry::AdapterRegistry;
pub use shell::ShellAdapter;
pub use traits::{AdapterInfo, OutputAnalysis, RuntimeAdapter, RuntimeState};
