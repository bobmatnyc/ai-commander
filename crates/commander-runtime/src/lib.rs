//! Async runtime for Commander.
//!
//! This crate provides the async execution infrastructure for Commander:
//! - `RuntimeExecutor` - spawns and manages AI tool instances
//! - `OutputPoller` - polls tmux output for changes
//! - `Runtime` - main entry point combining executor and poller
//!
//! # Example
//!
//! ```ignore
//! use commander_runtime::{Runtime, RuntimeConfig};
//! use commander_models::Project;
//! use commander_adapters::ClaudeCodeAdapter;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = RuntimeConfig::default();
//!     let mut runtime = Runtime::new(config).await?;
//!
//!     // Subscribe to events
//!     let mut events = runtime.executor().subscribe();
//!
//!     // Start the runtime
//!     runtime.start().await?;
//!
//!     // Start an instance
//!     let project = Project::new("/path/to/project", "my-project");
//!     let adapter = Arc::new(ClaudeCodeAdapter::new());
//!     runtime.executor().start(&project, adapter).await?;
//!
//!     // Handle events
//!     tokio::spawn(async move {
//!         while let Ok(event) = events.recv().await {
//!             println!("Event: {:?}", event);
//!         }
//!     });
//!
//!     // Wait for shutdown signal
//!     tokio::signal::ctrl_c().await?;
//!     runtime.shutdown().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Key Concepts
//!
//! ## RuntimeExecutor
//!
//! The executor manages the lifecycle of AI tool instances:
//! - Starting instances in tmux sessions
//! - Stopping instances gracefully or forcefully
//! - Tracking instance state
//! - Broadcasting events to subscribers
//!
//! ## OutputPoller
//!
//! The poller runs in a background task and periodically:
//! - Captures output from tmux sessions
//! - Detects output changes
//! - Analyzes output using adapter patterns
//! - Updates instance state
//! - Emits events
//!
//! ## Runtime
//!
//! The main entry point that combines the executor and poller:
//! - Creates and manages the executor
//! - Spawns the poller task
//! - Handles graceful shutdown

pub mod config;
pub mod error;
pub mod event;
pub mod executor;
pub mod poller;
pub mod runtime;

pub use config::RuntimeConfig;
pub use error::{Result, RuntimeError};
pub use event::RuntimeEvent;
pub use executor::{RunningInstance, RuntimeExecutor};
pub use poller::OutputPoller;
pub use runtime::Runtime;
