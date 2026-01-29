//! Persistence layer for Commander.
//!
//! This crate provides crash-safe persistence for Commander state using
//! atomic file operations (write to temp file, then rename).
//!
//! # Example
//!
//! ```no_run
//! use commander_persistence::{StateStore, EventStore, WorkStore};
//! use commander_models::Project;
//!
//! let store = StateStore::new("/home/user/.commander");
//!
//! // Save a project
//! let project = Project::new("/path/to/code", "my-project");
//! store.save_project(&project).unwrap();
//!
//! // Load it back
//! let loaded = store.load_project(&project.id).unwrap();
//! ```

pub mod atomic;
pub mod error;
pub mod event_store;
pub mod state_store;
pub mod work_store;

pub use error::{PersistenceError, Result};
pub use event_store::EventStore;
pub use state_store::StateStore;
pub use work_store::WorkStore;
