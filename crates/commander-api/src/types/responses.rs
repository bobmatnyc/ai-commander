//! Response DTOs for the API.

use chrono::{DateTime, Utc};
use serde::Serialize;

use commander_adapters::AdapterInfo;
use commander_models::{Event, Project, WorkItem};

/// Health check response.
#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    /// Service status.
    pub status: String,
    /// Service version.
    pub version: String,
    /// Uptime in seconds.
    pub uptime_seconds: u64,
}

/// Project list response.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectListResponse {
    /// List of projects.
    pub projects: Vec<ProjectSummary>,
    /// Total count.
    pub total: usize,
}

/// Project summary for list responses.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    /// Project ID.
    pub id: String,
    /// Project name.
    pub name: String,
    /// Project path.
    pub path: String,
    /// Current state.
    pub state: String,
    /// Adapter in use (if any).
    pub adapter: Option<String>,
    /// When the project was created.
    pub created_at: DateTime<Utc>,
}

impl From<&Project> for ProjectSummary {
    fn from(project: &Project) -> Self {
        Self {
            id: project.id.as_str().to_string(),
            name: project.name.clone(),
            path: project.path.clone(),
            state: format!("{:?}", project.state).to_lowercase(),
            adapter: None, // Adapter info not stored in Project
            created_at: project.created_at,
        }
    }
}

/// Project detail response.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetailResponse {
    /// Project ID.
    pub id: String,
    /// Project name.
    pub name: String,
    /// Project path.
    pub path: String,
    /// Current state.
    pub state: String,
    /// State reason.
    pub state_reason: Option<String>,
    /// Whether the project has blocking events.
    pub has_blocking_events: bool,
    /// Pending events count.
    pub pending_events_count: usize,
    /// Work queue size.
    pub work_queue_size: usize,
    /// When the project was created.
    pub created_at: DateTime<Utc>,
    /// When the project was last active.
    pub last_activity: Option<DateTime<Utc>>,
}

impl From<&Project> for ProjectDetailResponse {
    fn from(project: &Project) -> Self {
        Self {
            id: project.id.as_str().to_string(),
            name: project.name.clone(),
            path: project.path.clone(),
            state: format!("{:?}", project.state).to_lowercase(),
            state_reason: project.state_reason.clone(),
            has_blocking_events: project.has_blocking_events(),
            pending_events_count: project.pending_events.len(),
            work_queue_size: project.work_queue.len(),
            created_at: project.created_at,
            last_activity: project.last_activity,
        }
    }
}

/// Event list response.
#[derive(Debug, Clone, Serialize)]
pub struct EventListResponse {
    /// List of events.
    pub events: Vec<EventSummary>,
    /// Total count.
    pub total: usize,
}

/// Event summary for list responses.
#[derive(Debug, Clone, Serialize)]
pub struct EventSummary {
    /// Event ID.
    pub id: String,
    /// Project ID.
    pub project_id: String,
    /// Event type.
    pub event_type: String,
    /// Event title.
    pub title: String,
    /// Event priority.
    pub priority: String,
    /// Event status.
    pub status: String,
    /// When the event was created.
    pub created_at: DateTime<Utc>,
}

impl From<&Event> for EventSummary {
    fn from(event: &Event) -> Self {
        Self {
            id: event.id.as_str().to_string(),
            project_id: event.project_id.as_str().to_string(),
            event_type: format!("{:?}", event.event_type).to_lowercase(),
            title: event.title.clone(),
            priority: format!("{:?}", event.priority).to_lowercase(),
            status: format!("{:?}", event.status).to_lowercase(),
            created_at: event.created_at,
        }
    }
}

/// Event detail response.
#[derive(Debug, Clone, Serialize)]
pub struct EventDetailResponse {
    /// Event ID.
    pub id: String,
    /// Project ID.
    pub project_id: String,
    /// Event type.
    pub event_type: String,
    /// Event title.
    pub title: String,
    /// Event content.
    pub content: Option<String>,
    /// Event priority.
    pub priority: String,
    /// Event status.
    pub status: String,
    /// Response text (if resolved).
    pub response: Option<String>,
    /// Whether this is a blocking event.
    pub is_blocking: bool,
    /// When the event was created.
    pub created_at: DateTime<Utc>,
    /// When the event was responded to.
    pub responded_at: Option<DateTime<Utc>>,
}

impl From<&Event> for EventDetailResponse {
    fn from(event: &Event) -> Self {
        Self {
            id: event.id.as_str().to_string(),
            project_id: event.project_id.as_str().to_string(),
            event_type: format!("{:?}", event.event_type).to_lowercase(),
            title: event.title.clone(),
            content: event.content.clone(),
            priority: format!("{:?}", event.priority).to_lowercase(),
            status: format!("{:?}", event.status).to_lowercase(),
            response: event.response.clone(),
            is_blocking: event.is_blocking(),
            created_at: event.created_at,
            responded_at: event.responded_at,
        }
    }
}

/// Work list response.
#[derive(Debug, Clone, Serialize)]
pub struct WorkListResponse {
    /// List of work items.
    pub items: Vec<WorkSummary>,
    /// Total count.
    pub total: usize,
}

/// Work item summary for list responses.
#[derive(Debug, Clone, Serialize)]
pub struct WorkSummary {
    /// Work item ID.
    pub id: String,
    /// Project ID.
    pub project_id: String,
    /// Content/description.
    pub content: String,
    /// Priority.
    pub priority: String,
    /// State.
    pub state: String,
    /// When the item was created.
    pub created_at: DateTime<Utc>,
}

impl From<&WorkItem> for WorkSummary {
    fn from(item: &WorkItem) -> Self {
        Self {
            id: item.id.as_str().to_string(),
            project_id: item.project_id.as_str().to_string(),
            content: item.content.clone(),
            priority: format!("{:?}", item.priority).to_lowercase(),
            state: format!("{:?}", item.state).to_lowercase(),
            created_at: item.created_at,
        }
    }
}

/// Work item detail response.
#[derive(Debug, Clone, Serialize)]
pub struct WorkDetailResponse {
    /// Work item ID.
    pub id: String,
    /// Project ID.
    pub project_id: String,
    /// Content/description.
    pub content: String,
    /// Priority.
    pub priority: String,
    /// State.
    pub state: String,
    /// Dependencies.
    pub depends_on: Vec<String>,
    /// Result (if completed).
    pub result: Option<String>,
    /// Error (if failed).
    pub error: Option<String>,
    /// When the item was created.
    pub created_at: DateTime<Utc>,
    /// When the item started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the item completed.
    pub completed_at: Option<DateTime<Utc>>,
}

impl From<&WorkItem> for WorkDetailResponse {
    fn from(item: &WorkItem) -> Self {
        Self {
            id: item.id.as_str().to_string(),
            project_id: item.project_id.as_str().to_string(),
            content: item.content.clone(),
            priority: format!("{:?}", item.priority).to_lowercase(),
            state: format!("{:?}", item.state).to_lowercase(),
            depends_on: item.depends_on.iter().map(|id| id.as_str().to_string()).collect(),
            result: item.result.clone(),
            error: item.error.clone(),
            created_at: item.created_at,
            started_at: item.started_at,
            completed_at: item.completed_at,
        }
    }
}

/// Adapter list response.
#[derive(Debug, Clone, Serialize)]
pub struct AdapterListResponse {
    /// List of adapters.
    pub adapters: Vec<AdapterSummary>,
    /// Total count.
    pub total: usize,
}

/// Adapter summary for list responses.
#[derive(Debug, Clone, Serialize)]
pub struct AdapterSummary {
    /// Adapter ID.
    pub id: String,
    /// Adapter name.
    pub name: String,
    /// Adapter description.
    pub description: String,
    /// Launch command.
    pub command: String,
}

impl From<&AdapterInfo> for AdapterSummary {
    fn from(info: &AdapterInfo) -> Self {
        Self {
            id: info.id.clone(),
            name: info.name.clone(),
            description: info.description.clone(),
            command: info.command.clone(),
        }
    }
}

/// Generic success response.
#[derive(Debug, Clone, Serialize)]
pub struct SuccessResponse {
    /// Success message.
    pub message: String,
}

/// Created response with ID.
#[derive(Debug, Clone, Serialize)]
pub struct CreatedResponse {
    /// ID of the created resource.
    pub id: String,
    /// Success message.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_models::{EventType, WorkPriority};

    #[test]
    fn test_health_response_serialize() {
        let resp = HealthResponse {
            status: "ok".to_string(),
            version: "0.1.0".to_string(),
            uptime_seconds: 100,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"version\":\"0.1.0\""));
        assert!(json.contains("\"uptime_seconds\":100"));
    }

    #[test]
    fn test_project_summary_from_project() {
        let project = Project::new("/tmp/test", "test-project");
        let summary = ProjectSummary::from(&project);

        assert_eq!(summary.name, "test-project");
        assert_eq!(summary.path, "/tmp/test");
        assert_eq!(summary.state, "idle");
    }

    #[test]
    fn test_event_summary_from_event() {
        let event = Event::new("proj-1", EventType::Status, "Test event");
        let summary = EventSummary::from(&event);

        assert_eq!(summary.project_id, "proj-1");
        assert_eq!(summary.title, "Test event");
        assert_eq!(summary.event_type, "status");
    }

    #[test]
    fn test_work_summary_from_work_item() {
        let item = WorkItem::with_priority("proj-1", "Build it", WorkPriority::High);
        let summary = WorkSummary::from(&item);

        assert_eq!(summary.project_id, "proj-1");
        assert_eq!(summary.content, "Build it");
        assert_eq!(summary.priority, "high");
    }
}
