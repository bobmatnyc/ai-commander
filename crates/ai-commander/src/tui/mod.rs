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
mod commands;
mod completion;
mod connection;
mod events;
mod git;
mod helpers;
mod input;
mod inspect;
mod messaging;
mod scroll;
mod sessions;
mod ui;

pub use app::{App, ClickAction, ClickableItem, Message, MessageDirection, SessionInfo, ViewMode};
pub use events::run;
pub use helpers::extract_ready_preview;
