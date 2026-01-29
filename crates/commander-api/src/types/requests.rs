//! Request DTOs for the API.

use serde::Deserialize;

/// Create project request.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProjectRequest {
    /// Project name.
    pub name: String,
    /// Path to the project directory.
    pub path: String,
    /// Optional adapter ID to use.
    pub adapter: Option<String>,
}

/// Send message to project request.
#[derive(Debug, Clone, Deserialize)]
pub struct SendMessageRequest {
    /// Message content.
    pub message: String,
}

/// Event list query parameters.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EventListQuery {
    /// Filter by project ID.
    pub project_id: Option<String>,
    /// Filter by status.
    pub status: Option<String>,
    /// Filter by priority.
    pub priority: Option<String>,
    /// Maximum number of events to return.
    pub limit: Option<usize>,
}

/// Resolve event request.
#[derive(Debug, Clone, Deserialize)]
pub struct ResolveEventRequest {
    /// Optional response text.
    pub response: Option<String>,
}

/// Create work item request.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateWorkRequest {
    /// Project ID for the work item.
    pub project_id: String,
    /// Work item content/description.
    pub content: String,
    /// Optional priority (low, medium, high, critical).
    pub priority: Option<String>,
    /// Optional list of work item IDs this depends on.
    pub depends_on: Option<Vec<String>>,
}

/// Complete work item request.
#[derive(Debug, Clone, Deserialize)]
pub struct CompleteWorkRequest {
    /// Optional result text.
    pub result: Option<String>,
}

/// Work list query parameters.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkListQuery {
    /// Filter by project ID.
    pub project_id: Option<String>,
    /// Filter by state.
    pub state: Option<String>,
    /// Filter by priority.
    pub priority: Option<String>,
    /// Maximum number of items to return.
    pub limit: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_project_request_deserialize() {
        let json = r#"{"name": "test", "path": "/tmp/test", "adapter": "claude-code"}"#;
        let req: CreateProjectRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "test");
        assert_eq!(req.path, "/tmp/test");
        assert_eq!(req.adapter, Some("claude-code".to_string()));
    }

    #[test]
    fn test_create_project_request_optional_adapter() {
        let json = r#"{"name": "test", "path": "/tmp/test"}"#;
        let req: CreateProjectRequest = serde_json::from_str(json).unwrap();
        assert!(req.adapter.is_none());
    }

    #[test]
    fn test_event_list_query_defaults() {
        let query = EventListQuery::default();
        assert!(query.project_id.is_none());
        assert!(query.status.is_none());
        assert!(query.limit.is_none());
    }

    #[test]
    fn test_create_work_request_deserialize() {
        let json = r#"{
            "project_id": "proj-1",
            "content": "Build the thing",
            "priority": "high",
            "depends_on": ["work-1", "work-2"]
        }"#;
        let req: CreateWorkRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.project_id, "proj-1");
        assert_eq!(req.content, "Build the thing");
        assert_eq!(req.priority, Some("high".to_string()));
        assert_eq!(req.depends_on, Some(vec!["work-1".to_string(), "work-2".to_string()]));
    }
}
