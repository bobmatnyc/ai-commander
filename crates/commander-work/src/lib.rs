//! Priority-based work queue with dependency tracking for Commander.
//!
//! This crate provides the `WorkQueue` for managing work items with:
//! - Thread-safe queue using `Arc<Mutex<T>>`
//! - Priority ordering using `BinaryHeap` with custom `Ord`
//! - Dependency tracking (items blocked until dependencies complete)
//! - Persistence integration with `WorkStore`
//!
//! # Example
//!
//! ```no_run
//! use commander_work::{WorkQueue, WorkFilter};
//! use commander_persistence::WorkStore;
//! use commander_models::{WorkItem, WorkPriority, ProjectId};
//!
//! // Create queue with persistence
//! let store = WorkStore::new("/tmp/commander");
//! let queue = WorkQueue::new(store);
//!
//! // Add work items
//! let item = WorkItem::with_priority("proj-1", "Build project", WorkPriority::High);
//! let id = queue.enqueue(item).unwrap();
//!
//! // Get next ready item
//! if let Some(work) = queue.dequeue() {
//!     println!("Working on: {}", work.content);
//!     queue.complete(&work.id).unwrap();
//! }
//! ```

pub mod error;
pub mod filter;
pub mod queue;

pub use error::{WorkError, Result};
pub use filter::WorkFilter;
pub use queue::WorkQueue;
