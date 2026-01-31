//! Terminal User Interface for Commander.
//!
//! Provides a multi-pane TUI with:
//! - Header showing project name and connection status
//! - Scrollable output area for messages
//! - Status bar with working indicator
//! - Input area for commands/messages
//! - Footer with keybindings

mod app;
mod events;
mod ui;

pub use app::{App, Message, MessageDirection, ViewMode};
pub use events::run;
