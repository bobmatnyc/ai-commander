//! Core data models for Commander.
//!
//! This crate provides the fundamental data types used throughout the
//! Commander system, including events, work items, and projects.

pub mod builders;
pub mod event;
pub mod ids;
pub mod project;
pub mod work;

// Re-export main types
pub use builders::EventBuilder;
pub use event::{
    default_priority, get_default_priorities, Event, EventPriority, EventStatus, EventType,
    BLOCKING_EVENTS, DEFAULT_PRIORITIES,
};
pub use ids::{EventId, MessageId, ProjectId, SessionId, WorkId};
pub use project::{Project, ProjectState, ThreadMessage, ToolSession};
pub use work::{WorkItem, WorkPriority, WorkState};
