//! Project handlers.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use commander_models::Project;

use crate::error::{ApiError, Result};
use crate::state::AppState;
use crate::types::{
    CreateProjectRequest, CreatedResponse, ProjectDetailResponse, ProjectListResponse,
    ProjectSummary, SendMessageRequest, SuccessResponse,
};

/// GET /api/projects - List all projects.
pub async fn list_projects(State(state): State<AppState>) -> Json<ProjectListResponse> {
    let projects = state.list_projects().await;
    let summaries: Vec<ProjectSummary> = projects.iter().map(ProjectSummary::from).collect();
    let total = summaries.len();

    Json(ProjectListResponse {
        projects: summaries,
        total,
    })
}

/// POST /api/projects - Create a new project.
pub async fn create_project(
    State(state): State<AppState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>)> {
    // Validate adapter if specified
    if let Some(adapter_id) = &req.adapter {
        if state.adapter_registry.get(adapter_id).is_none() {
            return Err(ApiError::BadRequest(format!(
                "unknown adapter: {}",
                adapter_id
            )));
        }
    }

    let project = Project::new(&req.path, &req.name);
    let project_id = project.id.as_str().to_string();
    state.save_project(project).await;

    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse {
            id: project_id,
            message: "project created".to_string(),
        }),
    ))
}

/// GET /api/projects/:id - Get a project by ID.
pub async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProjectDetailResponse>> {
    let project = state
        .get_project(&id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("project not found: {}", id)))?;

    Ok(Json(ProjectDetailResponse::from(&project)))
}

/// DELETE /api/projects/:id - Delete a project.
pub async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>> {
    // Stop the instance first if running
    if let Some(ref runtime) = state.runtime {
        let runtime = runtime.read().await;
        let project_id = commander_models::ProjectId::from_string(&id);
        if runtime.executor().has_instance(&project_id).await {
            let _ = runtime.executor().stop(&project_id, true).await;
        }
    }

    state
        .remove_project(&id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("project not found: {}", id)))?;

    Ok(Json(SuccessResponse {
        message: "project deleted".to_string(),
    }))
}

/// POST /api/projects/:id/start - Start a project instance.
pub async fn start_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>> {
    let project = state
        .get_project(&id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("project not found: {}", id)))?;

    let runtime = state
        .runtime
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("runtime not available".to_string()))?;

    let adapter = state
        .adapter_registry
        .default_adapter()
        .ok_or_else(|| ApiError::ServiceUnavailable("no adapter available".to_string()))?;

    let runtime = runtime.read().await;
    runtime.executor().start(&project, adapter).await?;

    Ok(Json(SuccessResponse {
        message: "project started".to_string(),
    }))
}

/// POST /api/projects/:id/stop - Stop a project instance.
pub async fn stop_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>> {
    let runtime = state
        .runtime
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("runtime not available".to_string()))?;

    let project_id = commander_models::ProjectId::from_string(&id);
    let runtime = runtime.read().await;
    runtime.executor().stop(&project_id, false).await?;

    Ok(Json(SuccessResponse {
        message: "project stopped".to_string(),
    }))
}

/// POST /api/projects/:id/send - Send a message to a project instance.
pub async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<SuccessResponse>> {
    let runtime = state
        .runtime
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("runtime not available".to_string()))?;

    let project = state
        .get_project(&id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("project not found: {}", id)))?;

    let project_id = commander_models::ProjectId::from_string(&id);
    let runtime = runtime.read().await;

    // Check if instance is running
    let executor = runtime.executor();
    if !executor.has_instance(&project_id).await {
        return Err(ApiError::NotFound(format!(
            "no running instance for project: {}",
            id
        )));
    }

    // Generate session name from project (same logic as in executor.start)
    let session_name = format!("cmd-{}", project.name.replace([' ', '.', '/'], "-"));

    // Send message via tmux
    executor
        .tmux()
        .send_line(&session_name, None, &req.message)
        .map_err(|e| ApiError::Internal(format!("failed to send message: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: "message sent".to_string(),
    }))
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
    async fn test_list_projects_empty() {
        let state = make_test_state();
        let response = list_projects(State(state)).await;

        assert_eq!(response.total, 0);
        assert!(response.projects.is_empty());
    }

    #[tokio::test]
    async fn test_create_and_list_projects() {
        let state = make_test_state();

        // Create a project
        let req = CreateProjectRequest {
            name: "test".to_string(),
            path: "/tmp/test".to_string(),
            adapter: None,
        };
        let (status, response) = create_project(State(state.clone()), Json(req))
            .await
            .unwrap();

        assert_eq!(status, StatusCode::CREATED);
        assert!(!response.id.is_empty());

        // List should contain it
        let list_response = list_projects(State(state)).await;
        assert_eq!(list_response.total, 1);
        assert_eq!(list_response.projects[0].name, "test");
    }

    #[tokio::test]
    async fn test_create_project_with_invalid_adapter() {
        let state = make_test_state();

        let req = CreateProjectRequest {
            name: "test".to_string(),
            path: "/tmp/test".to_string(),
            adapter: Some("invalid-adapter".to_string()),
        };
        let result = create_project(State(state), Json(req)).await;

        assert!(matches!(result, Err(ApiError::BadRequest(_))));
    }

    #[tokio::test]
    async fn test_get_project() {
        let state = make_test_state();

        // Create a project
        let project = Project::new("/tmp/test", "test");
        let project_id = project.id.as_str().to_string();
        state.save_project(project).await;

        // Get it
        let response = get_project(State(state), Path(project_id)).await.unwrap();
        assert_eq!(response.name, "test");
    }

    #[tokio::test]
    async fn test_get_project_not_found() {
        let state = make_test_state();
        let result = get_project(State(state), Path("nonexistent".to_string())).await;

        assert!(matches!(result, Err(ApiError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_delete_project() {
        let state = make_test_state();

        // Create a project
        let project = Project::new("/tmp/test", "test");
        let project_id = project.id.as_str().to_string();
        state.save_project(project).await;

        // Delete it
        let response = delete_project(State(state.clone()), Path(project_id.clone()))
            .await
            .unwrap();
        assert_eq!(response.message, "project deleted");

        // Should be gone
        assert!(state.get_project(&project_id).await.is_none());
    }

    #[tokio::test]
    async fn test_start_project_no_runtime() {
        let state = make_test_state();

        // Create a project
        let project = Project::new("/tmp/test", "test");
        let project_id = project.id.as_str().to_string();
        state.save_project(project).await;

        // Try to start (no runtime available)
        let result = start_project(State(state), Path(project_id)).await;

        assert!(matches!(result, Err(ApiError::ServiceUnavailable(_))));
    }
}
