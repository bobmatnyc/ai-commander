//! Option detection and parsing for interactive selection in TUI.
//!
//! Re-exports the shared option detection logic from commander-core,
//! with TUI-specific helpers.

// Re-export all types from commander-core
pub use commander_core::options::{
    DetectedOptions,
    OptionDetector,
    OptionFormat,
};
