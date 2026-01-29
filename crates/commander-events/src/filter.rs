//! Event filtering for queries.

use commander_models::{Event, EventPriority, EventStatus, EventType, ProjectId};

/// Filter criteria for querying events.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Filter by project ID.
    pub project_id: Option<ProjectId>,
    /// Filter by event type.
    pub event_type: Option<EventType>,
    /// Filter by event status.
    pub status: Option<EventStatus>,
    /// Filter by minimum priority.
    pub priority_min: Option<EventPriority>,
}

impl EventFilter {
    /// Creates a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the project ID filter.
    pub fn with_project_id(mut self, project_id: ProjectId) -> Self {
        self.project_id = Some(project_id);
        self
    }

    /// Sets the event type filter.
    pub fn with_event_type(mut self, event_type: EventType) -> Self {
        self.event_type = Some(event_type);
        self
    }

    /// Sets the status filter.
    pub fn with_status(mut self, status: EventStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets the minimum priority filter.
    pub fn with_priority_min(mut self, priority: EventPriority) -> Self {
        self.priority_min = Some(priority);
        self
    }

    /// Returns true if the event matches this filter.
    pub fn matches(&self, event: &Event) -> bool {
        if let Some(ref project_id) = self.project_id {
            if event.project_id != *project_id {
                return false;
            }
        }

        if let Some(event_type) = self.event_type {
            if event.event_type != event_type {
                return false;
            }
        }

        if let Some(status) = self.status {
            if event.status != status {
                return false;
            }
        }

        if let Some(priority_min) = self.priority_min {
            if event.priority < priority_min {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_models::Event;

    fn make_event(project: &str, event_type: EventType) -> Event {
        Event::new(project, event_type, "Test")
    }

    #[test]
    fn test_empty_filter_matches_all() {
        let filter = EventFilter::new();
        let event = make_event("proj-1", EventType::Status);
        assert!(filter.matches(&event));
    }

    #[test]
    fn test_filter_by_project_id() {
        let filter = EventFilter::new().with_project_id("proj-1".into());

        let e1 = make_event("proj-1", EventType::Status);
        let e2 = make_event("proj-2", EventType::Status);

        assert!(filter.matches(&e1));
        assert!(!filter.matches(&e2));
    }

    #[test]
    fn test_filter_by_event_type() {
        let filter = EventFilter::new().with_event_type(EventType::Error);

        let e1 = make_event("proj-1", EventType::Error);
        let e2 = make_event("proj-1", EventType::Status);

        assert!(filter.matches(&e1));
        assert!(!filter.matches(&e2));
    }

    #[test]
    fn test_filter_by_status() {
        let filter = EventFilter::new().with_status(EventStatus::Pending);

        let e1 = make_event("proj-1", EventType::Status);
        let mut e2 = make_event("proj-1", EventType::Status);
        e2.status = EventStatus::Resolved;

        assert!(filter.matches(&e1));
        assert!(!filter.matches(&e2));
    }

    #[test]
    fn test_filter_by_priority_min() {
        let filter = EventFilter::new().with_priority_min(EventPriority::High);

        // Error has Critical priority
        let e1 = make_event("proj-1", EventType::Error);
        // Status has Info priority
        let e2 = make_event("proj-1", EventType::Status);
        // DecisionNeeded has High priority
        let e3 = make_event("proj-1", EventType::DecisionNeeded);

        assert!(filter.matches(&e1)); // Critical >= High
        assert!(!filter.matches(&e2)); // Info < High
        assert!(filter.matches(&e3)); // High >= High
    }

    #[test]
    fn test_combined_filters() {
        let filter = EventFilter::new()
            .with_project_id("proj-1".into())
            .with_event_type(EventType::Error)
            .with_status(EventStatus::Pending);

        let e1 = make_event("proj-1", EventType::Error);
        let e2 = make_event("proj-2", EventType::Error);
        let e3 = make_event("proj-1", EventType::Status);

        assert!(filter.matches(&e1));
        assert!(!filter.matches(&e2)); // wrong project
        assert!(!filter.matches(&e3)); // wrong type
    }
}
