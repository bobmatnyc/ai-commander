//! Event types and handling for Commander.
//!
//! Events represent notifications, decisions, and status updates that flow
//! through the Commander system.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::ids::{EventId, ProjectId, SessionId};

/// Types of events that can occur in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// A decision is needed from the user.
    DecisionNeeded,
    /// Clarification is needed on requirements.
    Clarification,
    /// An error has occurred.
    Error,
    /// Approval is needed to proceed.
    Approval,
    /// A task has been completed.
    TaskComplete,
    /// A milestone has been reached.
    Milestone,
    /// General status update.
    Status,
    /// Project is idle and waiting for work.
    ProjectIdle,
    /// An instance is starting up.
    InstanceStarting,
    /// An instance is ready.
    InstanceReady,
    /// An instance encountered an error.
    InstanceError,
}

/// Priority levels for events.
///
/// Higher priority events should be handled first.
/// Ordering: Critical > High > Normal > Low > Info
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventPriority {
    /// Informational only.
    Info,
    /// Low priority.
    Low,
    /// Normal priority.
    #[default]
    Normal,
    /// High priority.
    High,
    /// Critical priority - requires immediate attention.
    Critical,
}

impl PartialOrd for EventPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EventPriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_value().cmp(&other.as_value())
    }
}

impl EventPriority {
    /// Returns numeric value for priority comparison.
    /// Higher value = higher priority.
    fn as_value(&self) -> u8 {
        match self {
            EventPriority::Info => 0,
            EventPriority::Low => 1,
            EventPriority::Normal => 2,
            EventPriority::High => 3,
            EventPriority::Critical => 4,
        }
    }
}

/// Status of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    /// Event is pending and awaiting action.
    #[default]
    Pending,
    /// Event has been acknowledged.
    Acknowledged,
    /// Event has been resolved.
    Resolved,
    /// Event has been dismissed.
    Dismissed,
}

/// Event types that can block progress when pending.
pub const BLOCKING_EVENTS: &[EventType] = &[
    EventType::Error,
    EventType::DecisionNeeded,
    EventType::Approval,
];

/// Default priorities for each event type.
pub fn default_priority(event_type: EventType) -> EventPriority {
    match event_type {
        EventType::Error => EventPriority::Critical,
        EventType::DecisionNeeded => EventPriority::High,
        EventType::Approval => EventPriority::High,
        EventType::Clarification => EventPriority::Normal,
        EventType::TaskComplete => EventPriority::Normal,
        EventType::Milestone => EventPriority::Normal,
        EventType::Status => EventPriority::Info,
        EventType::ProjectIdle => EventPriority::Low,
        EventType::InstanceStarting => EventPriority::Info,
        EventType::InstanceReady => EventPriority::Info,
        EventType::InstanceError => EventPriority::Critical,
    }
}

/// Returns a map of all event types to their default priorities.
pub fn get_default_priorities() -> HashMap<EventType, EventPriority> {
    let mut m = HashMap::new();
    m.insert(EventType::Error, EventPriority::Critical);
    m.insert(EventType::DecisionNeeded, EventPriority::High);
    m.insert(EventType::Approval, EventPriority::High);
    m.insert(EventType::Clarification, EventPriority::Normal);
    m.insert(EventType::TaskComplete, EventPriority::Normal);
    m.insert(EventType::Milestone, EventPriority::Normal);
    m.insert(EventType::Status, EventPriority::Info);
    m.insert(EventType::ProjectIdle, EventPriority::Low);
    m.insert(EventType::InstanceStarting, EventPriority::Info);
    m.insert(EventType::InstanceReady, EventPriority::Info);
    m.insert(EventType::InstanceError, EventPriority::Critical);
    m
}

/// Alias for get_default_priorities for compatibility.
pub const DEFAULT_PRIORITIES: fn() -> HashMap<EventType, EventPriority> = get_default_priorities;

/// An event in the Commander system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique identifier for the event.
    pub id: EventId,

    /// ID of the project this event belongs to.
    pub project_id: ProjectId,

    /// Type of the event.
    pub event_type: EventType,

    /// Priority level of the event.
    pub priority: EventPriority,

    /// Short title describing the event.
    pub title: String,

    /// Session ID associated with this event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,

    /// Current status of the event.
    pub status: EventStatus,

    /// Detailed content of the event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Additional context data.
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,

    /// Available options for decision events.
    #[serde(default)]
    pub options: Vec<String>,

    /// Response provided by the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,

    /// When the response was provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responded_at: Option<DateTime<Utc>>,

    /// When the event was created.
    pub created_at: DateTime<Utc>,
}

impl Event {
    /// Creates a new event with the given parameters.
    pub fn new(project_id: impl Into<ProjectId>, event_type: EventType, title: impl Into<String>) -> Self {
        Self {
            id: EventId::new(),
            project_id: project_id.into(),
            event_type,
            priority: default_priority(event_type),
            title: title.into(),
            session_id: None,
            status: EventStatus::Pending,
            content: None,
            context: HashMap::new(),
            options: Vec::new(),
            response: None,
            responded_at: None,
            created_at: Utc::now(),
        }
    }

    /// Returns true if this event blocks progress.
    ///
    /// An event is blocking if it is of a blocking type and is still pending.
    pub fn is_blocking(&self) -> bool {
        if self.status != EventStatus::Pending {
            return false;
        }

        matches!(
            self.event_type,
            EventType::Error | EventType::DecisionNeeded | EventType::Approval
        )
    }

    /// Returns the scope of blocking for this event.
    ///
    /// - "all" for errors (blocks everything)
    /// - "project" for other blocking events
    /// - None for non-blocking events
    pub fn blocking_scope(&self) -> Option<&'static str> {
        if !self.is_blocking() {
            return None;
        }

        match self.event_type {
            EventType::Error => Some("all"),
            _ => Some("project"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_priority_ordering() {
        assert!(EventPriority::Critical > EventPriority::High);
        assert!(EventPriority::High > EventPriority::Normal);
        assert!(EventPriority::Normal > EventPriority::Low);
        assert!(EventPriority::Low > EventPriority::Info);
    }

    #[test]
    fn test_event_priority_equality() {
        assert_eq!(EventPriority::Critical, EventPriority::Critical);
        assert_ne!(EventPriority::Critical, EventPriority::High);
    }

    #[test]
    fn test_event_creation() {
        let event = Event::new(
            "project-1",
            EventType::DecisionNeeded,
            "Choose an option",
        );

        assert!(event.id.as_str().starts_with("evt-"));
        assert_eq!(event.project_id.as_str(), "project-1");
        assert_eq!(event.event_type, EventType::DecisionNeeded);
        assert_eq!(event.priority, EventPriority::High);
        assert_eq!(event.status, EventStatus::Pending);
    }

    #[test]
    fn test_is_blocking_for_pending_error() {
        let event = Event::new("p1", EventType::Error, "Error");
        assert!(event.is_blocking());
    }

    #[test]
    fn test_is_blocking_for_pending_decision() {
        let event = Event::new(
            "p1",
            EventType::DecisionNeeded,
            "Decision",
        );
        assert!(event.is_blocking());
    }

    #[test]
    fn test_is_blocking_for_pending_approval() {
        let event = Event::new("p1", EventType::Approval, "Approval");
        assert!(event.is_blocking());
    }

    #[test]
    fn test_is_not_blocking_for_status() {
        let event = Event::new("p1", EventType::Status, "Status");
        assert!(!event.is_blocking());
    }

    #[test]
    fn test_is_not_blocking_when_resolved() {
        let mut event = Event::new("p1", EventType::Error, "Error");
        event.status = EventStatus::Resolved;
        assert!(!event.is_blocking());
    }

    #[test]
    fn test_is_not_blocking_when_acknowledged() {
        let mut event = Event::new("p1", EventType::Error, "Error");
        event.status = EventStatus::Acknowledged;
        assert!(!event.is_blocking());
    }

    #[test]
    fn test_is_not_blocking_when_dismissed() {
        let mut event = Event::new("p1", EventType::Error, "Error");
        event.status = EventStatus::Dismissed;
        assert!(!event.is_blocking());
    }

    #[test]
    fn test_blocking_scope_for_error() {
        let event = Event::new("p1", EventType::Error, "Error");
        assert_eq!(event.blocking_scope(), Some("all"));
    }

    #[test]
    fn test_blocking_scope_for_decision() {
        let event = Event::new(
            "p1",
            EventType::DecisionNeeded,
            "Decision",
        );
        assert_eq!(event.blocking_scope(), Some("project"));
    }

    #[test]
    fn test_blocking_scope_for_approval() {
        let event = Event::new("p1", EventType::Approval, "Approval");
        assert_eq!(event.blocking_scope(), Some("project"));
    }

    #[test]
    fn test_blocking_scope_for_non_blocking() {
        let event = Event::new("p1", EventType::Status, "Status");
        assert_eq!(event.blocking_scope(), None);
    }

    #[test]
    fn test_blocking_scope_when_resolved() {
        let mut event = Event::new("p1", EventType::Error, "Error");
        event.status = EventStatus::Resolved;
        assert_eq!(event.blocking_scope(), None);
    }

    #[test]
    fn test_default_priorities() {
        assert_eq!(default_priority(EventType::Error), EventPriority::Critical);
        assert_eq!(
            default_priority(EventType::DecisionNeeded),
            EventPriority::High
        );
        assert_eq!(default_priority(EventType::Approval), EventPriority::High);
        assert_eq!(
            default_priority(EventType::Clarification),
            EventPriority::Normal
        );
        assert_eq!(default_priority(EventType::Status), EventPriority::Info);
        assert_eq!(
            default_priority(EventType::InstanceError),
            EventPriority::Critical
        );
    }

    #[test]
    fn test_event_serialization_roundtrip() {
        let mut event = Event::new(
            "project-1",
            EventType::DecisionNeeded,
            "Choose an option",
        );
        event.content = Some("Please choose".to_string());
        event.options = vec!["Option A".to_string(), "Option B".to_string()];
        event
            .context
            .insert("key".to_string(), serde_json::json!("value"));

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: Event = serde_json::from_str(&json).unwrap();

        assert_eq!(event.id, deserialized.id);
        assert_eq!(event.project_id, deserialized.project_id);
        assert_eq!(event.event_type, deserialized.event_type);
        assert_eq!(event.priority, deserialized.priority);
        assert_eq!(event.title, deserialized.title);
        assert_eq!(event.status, deserialized.status);
        assert_eq!(event.content, deserialized.content);
        assert_eq!(event.options, deserialized.options);
    }

    #[test]
    fn test_event_type_serialization() {
        let json = serde_json::to_string(&EventType::DecisionNeeded).unwrap();
        assert_eq!(json, "\"decision_needed\"");

        let deserialized: EventType = serde_json::from_str("\"decision_needed\"").unwrap();
        assert_eq!(deserialized, EventType::DecisionNeeded);
    }

    #[test]
    fn test_event_priority_serialization() {
        let json = serde_json::to_string(&EventPriority::Critical).unwrap();
        assert_eq!(json, "\"critical\"");

        let deserialized: EventPriority = serde_json::from_str("\"critical\"").unwrap();
        assert_eq!(deserialized, EventPriority::Critical);
    }

    #[test]
    fn test_event_status_serialization() {
        let json = serde_json::to_string(&EventStatus::Pending).unwrap();
        assert_eq!(json, "\"pending\"");

        let deserialized: EventStatus = serde_json::from_str("\"pending\"").unwrap();
        assert_eq!(deserialized, EventStatus::Pending);
    }
}
