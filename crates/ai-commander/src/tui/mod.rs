//! Terminal User Interface for Commander.
//!
//! Provides a multi-pane TUI with:
//! - Header showing project name and connection status
//! - Scrollable output area for messages
//! - Status bar with working indicator
//! - Input area for commands/messages
//! - Footer with keybindings

mod agents;
mod app;
mod completion;
mod events;
mod git;
mod helpers;
mod inspect;
mod scroll;
mod ui;

pub use app::{App, Message, MessageDirection, SessionInfo, ViewMode};
pub use events::run;
pub use helpers::extract_ready_preview;
