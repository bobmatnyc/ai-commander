//! Thread-safe event management with pub/sub for Commander.
//!
//! This crate provides the `EventManager` for managing events with:
//! - Thread-safe storage using `Arc<RwLock<T>>`
//! - Pub/sub notifications using `mpsc` channels
//! - Persistence integration with `EventStore`
//!
//! # Example
//!
//! ```no_run
//! use commander_events::{EventManager, EventFilter};
//! use commander_persistence::EventStore;
//! use commander_models::{Event, EventType, ProjectId};
//!
//! // Create manager with persistence
//! let store = EventStore::new("/tmp/commander");
//! let manager = EventManager::new(store);
//!
//! // Subscribe to events
//! let receiver = manager.subscribe();
//!
//! // Emit an event
//! let event = Event::new("proj-1", EventType::Status, "Build complete");
//! let event_id = manager.emit(event).unwrap();
//!
//! // Filter events
//! let filter = EventFilter::default().with_project_id("proj-1".into());
//! let events = manager.list(Some(filter));
//! ```

pub mod error;
pub mod filter;
pub mod manager;

pub use error::{EventError, Result};
pub use filter::EventFilter;
pub use manager::EventManager;
