//! Tmux orchestration for Commander.
//!
//! This crate provides tmux session and pane management for Commander agents:
//! - Create and destroy tmux sessions
//! - Create panes within sessions
//! - Capture pane output
//! - Send input to panes
//! - Handle missing tmux gracefully
//!
//! # Example
//!
//! ```no_run
//! use commander_tmux::TmuxOrchestrator;
//!
//! // Create orchestrator (verifies tmux is available)
//! let tmux = TmuxOrchestrator::new().expect("tmux not found");
//!
//! // Create a session
//! let session = tmux.create_session("my-session").unwrap();
//! println!("Created session: {}", session.name);
//!
//! // Send a command
//! tmux.send_line("my-session", None, "echo hello").unwrap();
//!
//! // Capture output
//! let output = tmux.capture_output("my-session", None, Some(10)).unwrap();
//! println!("Output: {}", output);
//!
//! // Create another pane
//! let pane = tmux.create_pane("my-session").unwrap();
//! println!("Created pane: {}", pane.id);
//!
//! // Send to specific pane
//! tmux.send_line("my-session", Some(&pane.id), "ls -la").unwrap();
//!
//! // Clean up
//! tmux.destroy_session("my-session").unwrap();
//! ```
//!
//! # Checking tmux Availability
//!
//! ```
//! use commander_tmux::TmuxOrchestrator;
//!
//! if TmuxOrchestrator::is_available() {
//!     println!("tmux is available");
//! } else {
//!     println!("tmux not found, using fallback");
//! }
//! ```

pub mod error;
pub mod orchestrator;
pub mod session;

pub use error::{Result, TmuxError};
pub use orchestrator::TmuxOrchestrator;
pub use session::{TmuxPane, TmuxSession};
