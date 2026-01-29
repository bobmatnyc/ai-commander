//! Event handlers.

use axum::{
    extract::{Path, Query, State},
    Json,
};

use commander_events::EventFilter;
use commander_models::{EventId, EventPriority, EventStatus, ProjectId};

use crate::error::{ApiError, Result};
use crate::state::AppState;
use crate::types::{
    EventDetailResponse, EventListQuery, EventListResponse, EventSummary, ResolveEventRequest,
    SuccessResponse,
};

/// GET /api/events - List events with optional filters.
pub async fn list_events(
    State(state): State<AppState>,
    Query(query): Query<EventListQuery>,
) -> Json<EventListResponse> {
    let mut filter = EventFilter::new();

    if let Some(project_id) = query.project_id {
        filter = filter.with_project_id(ProjectId::from_string(&project_id));
    }

    if let Some(status_str) = query.status {
        if let Some(status) = parse_event_status(&status_str) {
            filter = filter.with_status(status);
        }
    }

    if let Some(priority_str) = query.priority {
        if let Some(priority) = parse_event_priority(&priority_str) {
            filter = filter.with_priority_min(priority);
        }
    }

    let events = state.event_manager.list(Some(filter));
    let mut summaries: Vec<EventSummary> = events.iter().map(EventSummary::from).collect();

    // Apply limit if specified
    if let Some(limit) = query.limit {
        summaries.truncate(limit);
    }

    let total = summaries.len();

    Json(EventListResponse {
        events: summaries,
        total,
    })
}

/// GET /api/events/:id - Get an event by ID.
pub async fn get_event(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<EventDetailResponse>> {
    let event_id = EventId::from(id.as_str());
    let event = state
        .event_manager
        .get(&event_id)
        .ok_or_else(|| ApiError::NotFound(format!("event not found: {}", id)))?;

    Ok(Json(EventDetailResponse::from(&event)))
}

/// POST /api/events/:id/acknowledge - Acknowledge an event.
pub async fn acknowledge_event(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>> {
    let event_id = EventId::from(id.as_str());
    state.event_manager.acknowledge(&event_id)?;

    Ok(Json(SuccessResponse {
        message: "event acknowledged".to_string(),
    }))
}

/// POST /api/events/:id/resolve - Resolve an event.
pub async fn resolve_event(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ResolveEventRequest>,
) -> Result<Json<SuccessResponse>> {
    let event_id = EventId::from(id.as_str());
    state.event_manager.resolve(&event_id, req.response)?;

    Ok(Json(SuccessResponse {
        message: "event resolved".to_string(),
    }))
}

fn parse_event_status(s: &str) -> Option<EventStatus> {
    match s.to_lowercase().as_str() {
        "pending" => Some(EventStatus::Pending),
        "acknowledged" => Some(EventStatus::Acknowledged),
        "resolved" => Some(EventStatus::Resolved),
        _ => None,
    }
}

fn parse_event_priority(s: &str) -> Option<EventPriority> {
    match s.to_lowercase().as_str() {
        "info" => Some(EventPriority::Info),
        "low" => Some(EventPriority::Low),
        "normal" | "medium" => Some(EventPriority::Normal),
        "high" => Some(EventPriority::High),
        "critical" => Some(EventPriority::Critical),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApiConfig;
    use commander_adapters::AdapterRegistry;
    use commander_events::EventManager;
    use commander_models::{Event, EventType};
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
    async fn test_list_events_empty() {
        let state = make_test_state();
        let response = list_events(State(state), Query(EventListQuery::default())).await;

        assert_eq!(response.total, 0);
        assert!(response.events.is_empty());
    }

    #[tokio::test]
    async fn test_list_events_with_data() {
        let state = make_test_state();

        // Add some events
        let event1 = Event::new("proj-1", EventType::Status, "Event 1");
        let event2 = Event::new("proj-2", EventType::Error, "Event 2");
        state.event_manager.emit(event1).unwrap();
        state.event_manager.emit(event2).unwrap();

        // List all
        let response = list_events(State(state.clone()), Query(EventListQuery::default())).await;
        assert_eq!(response.total, 2);

        // Filter by project
        let query = EventListQuery {
            project_id: Some("proj-1".to_string()),
            ..Default::default()
        };
        let response = list_events(State(state), Query(query)).await;
        assert_eq!(response.total, 1);
        assert_eq!(response.events[0].project_id, "proj-1");
    }

    #[tokio::test]
    async fn test_get_event() {
        let state = make_test_state();

        let event = Event::new("proj-1", EventType::Status, "Test Event");
        let event_id = state.event_manager.emit(event).unwrap();

        let response = get_event(State(state), Path(event_id.as_str().to_string()))
            .await
            .unwrap();
        assert_eq!(response.title, "Test Event");
    }

    #[tokio::test]
    async fn test_get_event_not_found() {
        let state = make_test_state();
        let result = get_event(State(state), Path("nonexistent".to_string())).await;

        assert!(matches!(result, Err(ApiError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_acknowledge_event() {
        let state = make_test_state();

        let event = Event::new("proj-1", EventType::Status, "Test Event");
        let event_id = state.event_manager.emit(event).unwrap();

        let response = acknowledge_event(State(state.clone()), Path(event_id.as_str().to_string()))
            .await
            .unwrap();
        assert_eq!(response.message, "event acknowledged");

        // Verify status changed
        let event = state.event_manager.get(&event_id).unwrap();
        assert_eq!(event.status, EventStatus::Acknowledged);
    }

    #[tokio::test]
    async fn test_resolve_event() {
        let state = make_test_state();

        let event = Event::new("proj-1", EventType::DecisionNeeded, "Make a choice");
        let event_id = state.event_manager.emit(event).unwrap();

        let req = ResolveEventRequest {
            response: Some("Option A".to_string()),
        };
        let response = resolve_event(
            State(state.clone()),
            Path(event_id.as_str().to_string()),
            Json(req),
        )
        .await
        .unwrap();
        assert_eq!(response.message, "event resolved");

        // Verify status and response
        let event = state.event_manager.get(&event_id).unwrap();
        assert_eq!(event.status, EventStatus::Resolved);
        assert_eq!(event.response, Some("Option A".to_string()));
    }

    #[test]
    fn test_parse_event_status() {
        assert_eq!(parse_event_status("pending"), Some(EventStatus::Pending));
        assert_eq!(
            parse_event_status("acknowledged"),
            Some(EventStatus::Acknowledged)
        );
        assert_eq!(parse_event_status("resolved"), Some(EventStatus::Resolved));
        assert_eq!(parse_event_status("PENDING"), Some(EventStatus::Pending));
        assert_eq!(parse_event_status("invalid"), None);
    }

    #[test]
    fn test_parse_event_priority() {
        assert_eq!(parse_event_priority("info"), Some(EventPriority::Info));
        assert_eq!(parse_event_priority("low"), Some(EventPriority::Low));
        assert_eq!(parse_event_priority("normal"), Some(EventPriority::Normal));
        assert_eq!(parse_event_priority("medium"), Some(EventPriority::Normal));
        assert_eq!(parse_event_priority("high"), Some(EventPriority::High));
        assert_eq!(
            parse_event_priority("critical"),
            Some(EventPriority::Critical)
        );
        assert_eq!(parse_event_priority("HIGH"), Some(EventPriority::High));
        assert_eq!(parse_event_priority("invalid"), None);
    }
}
