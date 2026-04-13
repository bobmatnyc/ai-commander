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
//! - **structured_summarizer**: Extract structured facts and template-based summaries
//! - **summarizer**: Summarize long responses using OpenRouter API

pub mod change_detector;
pub mod client_adapter;
pub mod config;
pub mod migration;
pub mod notification_parser;
pub mod ollama;
pub mod onboarding;
pub mod options;
pub mod output_filter;
pub mod structured_summarizer;
pub mod summarizer;
pub mod usage;

// Re-export Ollama client
pub use ollama::{OllamaClient, OllamaError};

// Re-export commonly used items for convenience
pub use config::{
    cache_dir, chroma_dir, config_dir, config_file, db_dir, ensure_all_dirs, ensure_config_dir,
    ensure_runtime_state_dir, ensure_sessions_dir, ensure_state_dir, env_file, legacy_state_dir,
    logs_dir, notifications_file, pairing_file, projects_file, runtime_state_dir, sessions_dir,
    state_dir, telegram_pid_file,
};
pub use migration::migrate_if_needed;
pub use onboarding::{load_config, needs_onboarding, run_onboarding};
pub use output_filter::{clean_response, clean_screen_preview, detect_adapter, detect_selector, find_new_lines, is_claude_ready, is_mpm_ready, is_ui_noise, Adapter, SelectorPrompt, SessionEvent};
pub use summarizer::{
    interpret_screen_context, is_available as is_summarization_available, summarize_async,
    summarize_blocking, summarize_blocking_with_fallback, summarize_incremental,
    summarize_incremental_tiered, summarize_tiered, summarize_with_fallback, SummarizerError,
};

// Re-export change detection types
pub use change_detector::{
    ChangeDetector, ChangeEvent, ChangeNotification, ChangeType, Significance, SmartPoller,
};

// Re-export notification parsing
pub use notification_parser::{parse_notification, parse_notifications_all, parse_session_preview, strip_ansi, ParsedSessionStatus};

// Re-export option detection
pub use options::{DetectedOptions, OptionDetector, OptionFormat, ParsedOption};

// Re-export client adapter types and pipeline functions
pub use client_adapter::{
    interpret_output, interpret_output_with_summary, ClientCapabilities, ClientFragment,
    ClientRenderer, InterpretedOutput, SessionStatus,
};

// Re-export structured summarizer
pub use structured_summarizer::{extract as extract_structured, StructuredSummary, TestResult};
