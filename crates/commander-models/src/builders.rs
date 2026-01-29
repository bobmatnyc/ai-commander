//! Builder patterns for complex types.

use chrono::Utc;
use std::collections::HashMap;

use crate::event::{default_priority, Event, EventPriority, EventStatus, EventType};
use crate::ids::{EventId, ProjectId, SessionId};

/// Builder for creating Event instances with a fluent API.
#[derive(Debug, Clone)]
pub struct EventBuilder {
    project_id: ProjectId,
    event_type: EventType,
    title: String,
    priority: Option<EventPriority>,
    session_id: Option<SessionId>,
    content: Option<String>,
    context: HashMap<String, serde_json::Value>,
    options: Vec<String>,
}

impl EventBuilder {
    /// Creates a new EventBuilder with required fields.
    pub fn new(
        project_id: impl Into<ProjectId>,
        event_type: EventType,
        title: impl Into<String>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            event_type,
            title: title.into(),
            priority: None,
            session_id: None,
            content: None,
            context: HashMap::new(),
            options: Vec::new(),
        }
    }

    /// Sets the priority (defaults to type-based priority if not set).
    pub fn priority(mut self, priority: EventPriority) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Sets the session ID.
    pub fn session(mut self, session_id: impl Into<SessionId>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Sets the content.
    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    /// Adds a context value.
    pub fn with_context(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// Sets the options for decision events.
    pub fn options(mut self, options: Vec<String>) -> Self {
        self.options = options;
        self
    }

    /// Adds a single option.
    pub fn add_option(mut self, option: impl Into<String>) -> Self {
        self.options.push(option.into());
        self
    }

    /// Builds the Event.
    pub fn build(self) -> Event {
        Event {
            id: EventId::new(),
            project_id: self.project_id,
            event_type: self.event_type,
            priority: self
                .priority
                .unwrap_or_else(|| default_priority(self.event_type)),
            title: self.title,
            session_id: self.session_id,
            status: EventStatus::Pending,
            content: self.content,
            context: self.context,
            options: self.options,
            response: None,
            responded_at: None,
            created_at: Utc::now(),
        }
    }
}

/// Convenience methods on Event for creating builders.
impl Event {
    /// Creates a builder for a new event.
    pub fn builder(
        project_id: impl Into<ProjectId>,
        event_type: EventType,
        title: impl Into<String>,
    ) -> EventBuilder {
        EventBuilder::new(project_id, event_type, title)
    }

    /// Creates a decision event that requires user choice.
    pub fn decision(
        project_id: impl Into<ProjectId>,
        title: impl Into<String>,
        options: Vec<String>,
    ) -> Self {
        EventBuilder::new(project_id, EventType::DecisionNeeded, title)
            .options(options)
            .build()
    }

    /// Creates an error event.
    pub fn error(
        project_id: impl Into<ProjectId>,
        title: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        EventBuilder::new(project_id, EventType::Error, title)
            .content(content)
            .build()
    }

    /// Creates a status event.
    pub fn status(project_id: impl Into<ProjectId>, title: impl Into<String>) -> Self {
        EventBuilder::new(project_id, EventType::Status, title).build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_builder_basic() {
        let event = Event::builder("proj-1", EventType::Status, "Test event").build();

        assert!(event.id.as_str().starts_with("evt-"));
        assert_eq!(event.project_id.as_str(), "proj-1");
        assert_eq!(event.event_type, EventType::Status);
        assert_eq!(event.title, "Test event");
        assert_eq!(event.priority, EventPriority::Info); // Default for Status
    }

    #[test]
    fn test_event_builder_with_priority() {
        let event = Event::builder("proj-1", EventType::Status, "Test")
            .priority(EventPriority::High)
            .build();

        assert_eq!(event.priority, EventPriority::High);
    }

    #[test]
    fn test_event_builder_with_options() {
        let event = Event::builder("proj-1", EventType::DecisionNeeded, "Choose")
            .add_option("Option A")
            .add_option("Option B")
            .build();

        assert_eq!(event.options, vec!["Option A", "Option B"]);
    }

    #[test]
    fn test_event_builder_with_context() {
        let event = Event::builder("proj-1", EventType::Error, "Error")
            .with_context("file", "main.rs")
            .with_context("line", 42)
            .build();

        assert_eq!(
            event.context.get("file"),
            Some(&serde_json::json!("main.rs"))
        );
        assert_eq!(event.context.get("line"), Some(&serde_json::json!(42)));
    }

    #[test]
    fn test_event_decision_helper() {
        let event = Event::decision("proj-1", "Choose option", vec!["A".into(), "B".into()]);

        assert_eq!(event.event_type, EventType::DecisionNeeded);
        assert_eq!(event.options, vec!["A", "B"]);
    }

    #[test]
    fn test_event_error_helper() {
        let event = Event::error("proj-1", "Build failed", "Compilation error on line 42");

        assert_eq!(event.event_type, EventType::Error);
        assert_eq!(event.priority, EventPriority::Critical);
        assert_eq!(
            event.content,
            Some("Compilation error on line 42".to_string())
        );
    }

    #[test]
    fn test_event_status_helper() {
        let event = Event::status("proj-1", "Build started");

        assert_eq!(event.event_type, EventType::Status);
        assert_eq!(event.priority, EventPriority::Info);
    }
}
