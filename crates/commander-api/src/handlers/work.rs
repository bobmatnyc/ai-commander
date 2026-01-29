//! Work queue handlers.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};

use commander_models::{ProjectId, WorkId, WorkItem, WorkPriority, WorkState};
use commander_work::WorkFilter;

use crate::error::{ApiError, Result};
use crate::state::AppState;
use crate::types::{
    CompleteWorkRequest, CreateWorkRequest, CreatedResponse, WorkDetailResponse, WorkListQuery,
    WorkListResponse, WorkSummary, SuccessResponse,
};

/// GET /api/work - List work items with optional filters.
pub async fn list_work(
    State(state): State<AppState>,
    Query(query): Query<WorkListQuery>,
) -> Json<WorkListResponse> {
    let mut filter = WorkFilter::new();

    if let Some(project_id) = query.project_id {
        filter = filter.with_project_id(ProjectId::from_string(&project_id));
    }

    if let Some(state_str) = query.state {
        if let Some(work_state) = parse_work_state(&state_str) {
            filter = filter.with_state(work_state);
        }
    }

    if let Some(priority_str) = query.priority {
        if let Some(priority) = parse_work_priority(&priority_str) {
            filter = filter.with_priority(priority);
        }
    }

    let items = state.work_queue.list(Some(filter));
    let mut summaries: Vec<WorkSummary> = items.iter().map(WorkSummary::from).collect();

    // Apply limit if specified
    if let Some(limit) = query.limit {
        summaries.truncate(limit);
    }

    let total = summaries.len();

    Json(WorkListResponse {
        items: summaries,
        total,
    })
}

/// POST /api/work - Create a new work item.
pub async fn create_work(
    State(state): State<AppState>,
    Json(req): Json<CreateWorkRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>)> {
    let priority = req
        .priority
        .as_ref()
        .and_then(|s| parse_work_priority(s))
        .unwrap_or(WorkPriority::Medium);

    let mut item = WorkItem::with_priority(req.project_id.as_str(), &req.content, priority);

    // Add dependencies if specified
    if let Some(depends_on) = req.depends_on {
        item.depends_on = depends_on.into_iter().map(|id| WorkId::from(id.as_str())).collect();
    }

    let work_id = state.work_queue.enqueue(item)?;

    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse {
            id: work_id.as_str().to_string(),
            message: "work item created".to_string(),
        }),
    ))
}

/// GET /api/work/:id - Get a work item by ID.
pub async fn get_work(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WorkDetailResponse>> {
    let work_id = WorkId::from(id.as_str());
    let item = state
        .work_queue
        .get(&work_id)
        .ok_or_else(|| ApiError::NotFound(format!("work item not found: {}", id)))?;

    Ok(Json(WorkDetailResponse::from(&item)))
}

/// POST /api/work/:id/complete - Complete a work item.
pub async fn complete_work(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CompleteWorkRequest>,
) -> Result<Json<SuccessResponse>> {
    let work_id = WorkId::from(id.as_str());

    if let Some(result) = req.result {
        state.work_queue.complete_with_result(&work_id, result)?;
    } else {
        state.work_queue.complete(&work_id)?;
    }

    Ok(Json(SuccessResponse {
        message: "work item completed".to_string(),
    }))
}

fn parse_work_state(s: &str) -> Option<WorkState> {
    match s.to_lowercase().as_str() {
        "pending" => Some(WorkState::Pending),
        "queued" => Some(WorkState::Queued),
        "inprogress" | "in_progress" => Some(WorkState::InProgress),
        "completed" => Some(WorkState::Completed),
        "failed" => Some(WorkState::Failed),
        "cancelled" => Some(WorkState::Cancelled),
        "blocked" => Some(WorkState::Blocked),
        _ => None,
    }
}

fn parse_work_priority(s: &str) -> Option<WorkPriority> {
    match s.to_lowercase().as_str() {
        "low" => Some(WorkPriority::Low),
        "medium" => Some(WorkPriority::Medium),
        "high" => Some(WorkPriority::High),
        "critical" => Some(WorkPriority::Critical),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApiConfig;
    use commander_adapters::AdapterRegistry;
    use commander_events::EventManager;
    use commander_persistence::{EventStore, WorkStore};
    use commander_work::WorkQueue;
    use tempfile::tempdir;

    fn make_test_state() -> AppState {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);

        let event_store = EventStore::new(&path);
        let work_store = WorkStore::new(&path);

        AppState::new(
            ApiConfig::default(),
            None,
            EventManager::new(event_store),
            WorkQueue::new(work_store),
            AdapterRegistry::new(),
        )
    }

    #[tokio::test]
    async fn test_list_work_empty() {
        let state = make_test_state();
        let response = list_work(State(state), Query(WorkListQuery::default())).await;

        assert_eq!(response.total, 0);
        assert!(response.items.is_empty());
    }

    #[tokio::test]
    async fn test_create_and_list_work() {
        let state = make_test_state();

        // Create work item
        let req = CreateWorkRequest {
            project_id: "proj-1".to_string(),
            content: "Build the thing".to_string(),
            priority: Some("high".to_string()),
            depends_on: None,
        };
        let (status, response) = create_work(State(state.clone()), Json(req)).await.unwrap();

        assert_eq!(status, StatusCode::CREATED);
        assert!(!response.id.is_empty());

        // List should contain it
        let list_response = list_work(State(state), Query(WorkListQuery::default())).await;
        assert_eq!(list_response.total, 1);
        assert_eq!(list_response.items[0].content, "Build the thing");
        assert_eq!(list_response.items[0].priority, "high");
    }

    #[tokio::test]
    async fn test_create_work_with_dependencies() {
        let state = make_test_state();

        // Create first item
        let req1 = CreateWorkRequest {
            project_id: "proj-1".to_string(),
            content: "First task".to_string(),
            priority: None,
            depends_on: None,
        };
        let (_, resp1) = create_work(State(state.clone()), Json(req1)).await.unwrap();
        let resp1_id = resp1.id.clone();

        // Create second item depending on first
        let req2 = CreateWorkRequest {
            project_id: "proj-1".to_string(),
            content: "Second task".to_string(),
            priority: None,
            depends_on: Some(vec![resp1_id.clone()]),
        };
        let (_, resp2) = create_work(State(state.clone()), Json(req2)).await.unwrap();

        // Verify dependency
        let item = get_work(State(state), Path(resp2.id.clone())).await.unwrap();
        assert_eq!(item.depends_on.len(), 1);
        assert_eq!(item.depends_on[0], resp1_id);
    }

    #[tokio::test]
    async fn test_get_work() {
        let state = make_test_state();

        let req = CreateWorkRequest {
            project_id: "proj-1".to_string(),
            content: "Test task".to_string(),
            priority: None,
            depends_on: None,
        };
        let (_, created) = create_work(State(state.clone()), Json(req)).await.unwrap();

        let response = get_work(State(state), Path(created.id.clone())).await.unwrap();
        assert_eq!(response.content, "Test task");
    }

    #[tokio::test]
    async fn test_get_work_not_found() {
        let state = make_test_state();
        let result = get_work(State(state), Path("nonexistent".to_string())).await;

        assert!(matches!(result, Err(ApiError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_complete_work() {
        let state = make_test_state();

        // Create and dequeue (to make it InProgress)
        let req = CreateWorkRequest {
            project_id: "proj-1".to_string(),
            content: "Test task".to_string(),
            priority: None,
            depends_on: None,
        };
        let (_, created) = create_work(State(state.clone()), Json(req)).await.unwrap();

        // Dequeue to make it InProgress
        let _ = state.work_queue.dequeue();

        // Complete it
        let complete_req = CompleteWorkRequest {
            result: Some("Done!".to_string()),
        };
        let response = complete_work(State(state.clone()), Path(created.id.clone()), Json(complete_req))
            .await
            .unwrap();
        assert_eq!(response.message, "work item completed");

        // Verify state
        let work_id = WorkId::from(created.id.as_str());
        let item = state.work_queue.get(&work_id).unwrap();
        assert_eq!(item.state, WorkState::Completed);
        assert_eq!(item.result, Some("Done!".to_string()));
    }

    #[tokio::test]
    async fn test_list_work_with_filter() {
        let state = make_test_state();

        // Create items for different projects
        let req1 = CreateWorkRequest {
            project_id: "proj-1".to_string(),
            content: "Task 1".to_string(),
            priority: None,
            depends_on: None,
        };
        let req2 = CreateWorkRequest {
            project_id: "proj-2".to_string(),
            content: "Task 2".to_string(),
            priority: None,
            depends_on: None,
        };
        let _ = create_work(State(state.clone()), Json(req1)).await.unwrap();
        let _ = create_work(State(state.clone()), Json(req2)).await.unwrap();

        // Filter by project
        let query = WorkListQuery {
            project_id: Some("proj-1".to_string()),
            ..Default::default()
        };
        let response = list_work(State(state), Query(query)).await;
        assert_eq!(response.total, 1);
        assert_eq!(response.items[0].project_id, "proj-1");
    }

    #[test]
    fn test_parse_work_state() {
        assert_eq!(parse_work_state("pending"), Some(WorkState::Pending));
        assert_eq!(parse_work_state("queued"), Some(WorkState::Queued));
        assert_eq!(parse_work_state("inprogress"), Some(WorkState::InProgress));
        assert_eq!(parse_work_state("in_progress"), Some(WorkState::InProgress));
        assert_eq!(parse_work_state("completed"), Some(WorkState::Completed));
        assert_eq!(parse_work_state("failed"), Some(WorkState::Failed));
        assert_eq!(parse_work_state("cancelled"), Some(WorkState::Cancelled));
        assert_eq!(parse_work_state("blocked"), Some(WorkState::Blocked));
        assert_eq!(parse_work_state("PENDING"), Some(WorkState::Pending));
        assert_eq!(parse_work_state("invalid"), None);
    }

    #[test]
    fn test_parse_work_priority() {
        assert_eq!(parse_work_priority("low"), Some(WorkPriority::Low));
        assert_eq!(parse_work_priority("medium"), Some(WorkPriority::Medium));
        assert_eq!(parse_work_priority("high"), Some(WorkPriority::High));
        assert_eq!(parse_work_priority("critical"), Some(WorkPriority::Critical));
        assert_eq!(parse_work_priority("HIGH"), Some(WorkPriority::High));
        assert_eq!(parse_work_priority("invalid"), None);
    }
}
