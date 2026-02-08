//! Commander Core - shared business logic for all Commander interfaces.
//!
//! This crate provides core functionality used by both the TUI (ai-commander)
//! and Telegram (commander-telegram) interfaces:
//!
//! - **change_detector**: Smart change detection to reduce inference costs
//! - **config**: Shared configuration paths and utilities
//! - **migration**: Storage migration from legacy paths
//! - **notification_parser**: Parse timer notifications into structured data
//! - **onboarding**: First-run setup wizard
//! - **output_filter**: Filter UI noise from Claude Code terminal output
//! - **summarizer**: Summarize long responses using OpenRouter API

pub mod change_detector;
pub mod config;
pub mod migration;
pub mod notification_parser;
pub mod onboarding;
pub mod output_filter;
pub mod summarizer;

// Re-export commonly used items for convenience
pub use config::{
    cache_dir, chroma_dir, config_dir, config_file, db_dir, ensure_all_dirs, ensure_config_dir,
    ensure_runtime_state_dir, ensure_sessions_dir, ensure_state_dir, env_file, legacy_state_dir,
    logs_dir, notifications_file, pairing_file, projects_file, runtime_state_dir, sessions_dir,
    state_dir, telegram_pid_file,
};
pub use migration::migrate_if_needed;
pub use onboarding::{load_config, needs_onboarding, run_onboarding};
pub use output_filter::{clean_response, clean_screen_preview, find_new_lines, is_claude_ready, is_ui_noise};
pub use summarizer::{
    interpret_screen_context, is_available as is_summarization_available, summarize_async,
    summarize_blocking, summarize_blocking_with_fallback, summarize_with_fallback, SummarizerError,
};

// Re-export change detection types
pub use change_detector::{
    ChangeDetector, ChangeEvent, ChangeNotification, ChangeType, Significance, SmartPoller,
};

// Re-export notification parsing
pub use notification_parser::{parse_notification, parse_session_preview, strip_ansi, ParsedSessionStatus};
