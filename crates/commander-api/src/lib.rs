//! REST API for Commander.
//!
//! This crate provides a REST API for programmatic control of Commander:
//! - Project management (CRUD, start/stop, send messages)
//! - Event management (list, acknowledge, resolve)
//! - Work queue management (list, create, complete)
//! - Adapter listing
//!
//! # Example
//!
//! ```ignore
//! use commander_api::{ApiConfig, AppState, create_router, serve};
//! use commander_runtime::Runtime;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let state = AppState::new(/* ... */);
//!     let config = ApiConfig::default();
//!
//!     serve(config, state).await?;
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod error;
pub mod handlers;
pub mod router;
pub mod state;
pub mod types;

pub use config::ApiConfig;
pub use error::{ApiError, Result};
pub use router::{create_router, serve};
pub use state::AppState;
