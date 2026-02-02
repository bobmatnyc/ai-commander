//! Commander Core - shared business logic for all Commander interfaces.
//!
//! This crate provides core functionality used by both the TUI (commander-cli)
//! and Telegram (commander-telegram) interfaces:
//!
//! - **output_filter**: Filter UI noise from Claude Code terminal output
//! - **summarizer**: Summarize long responses using OpenRouter API
//! - **config**: Shared configuration paths and utilities

pub mod config;
pub mod output_filter;
pub mod summarizer;

// Re-export commonly used items for convenience
pub use config::state_dir;
pub use output_filter::{clean_response, clean_screen_preview, find_new_lines, is_claude_ready, is_ui_noise};
pub use summarizer::{
    is_available as is_summarization_available, summarize_async, summarize_blocking,
    summarize_blocking_with_fallback, summarize_with_fallback, SummarizerError,
};
