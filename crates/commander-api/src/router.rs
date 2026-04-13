//! Router configuration and server setup.

use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::info;

use crate::config::ApiConfig;
use crate::handlers;
use crate::state::AppState;

/// Creates the API router with all routes configured.
pub fn create_router(state: AppState) -> Router {
    // Build CORS layer
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let router = Router::new()
        // Health
        .route("/api/health", get(handlers::health))
        // Auth
        .route("/api/auth/pair", post(handlers::pair))
        .route("/api/auth/status", get(handlers::auth_status))
        .route("/api/auth/generate-code", post(handlers::generate_code))
        // Projects
        .route("/api/projects", get(handlers::list_projects))
        .route("/api/projects", post(handlers::create_project))
        .route("/api/projects/directories", get(handlers::web::list_project_directories))
        .route("/api/projects/{id}", get(handlers::get_project))
        .route("/api/projects/{id}", delete(handlers::delete_project))
        .route("/api/projects/{id}/start", post(handlers::start_project))
        .route("/api/projects/{id}/stop", post(handlers::stop_project))
        .route("/api/projects/{id}/send", post(handlers::send_message))
        // Events
        .route("/api/events", get(handlers::list_events))
        .route("/api/events/{id}", get(handlers::get_event))
        .route(
            "/api/events/{id}/acknowledge",
            post(handlers::acknowledge_event),
        )
        .route("/api/events/{id}/resolve", post(handlers::resolve_event))
        // Work
        .route("/api/work", get(handlers::list_work))
        .route("/api/work", post(handlers::create_work))
        .route("/api/work/{id}", get(handlers::get_work))
        .route("/api/work/{id}/complete", post(handlers::complete_work))
        // Adapters
        .route("/api/adapters", get(handlers::list_adapters))
        // Web UI — Session management
        .route("/api/sessions", get(handlers::web::list_sessions))
        .route("/api/sessions", post(handlers::web::create_session))
        .route("/api/sessions/disconnect", post(handlers::web::disconnect_session))
        .route("/api/sessions/message", post(handlers::web::send_message))
        .route("/api/sessions/rename", post(handlers::web::rename_session))
        .route("/api/sessions/{name}/connect", post(handlers::web::connect_session))
        .route("/api/sessions/{name}", delete(handlers::web::stop_session))
        .route("/api/sessions/{name}/interpret", post(handlers::web::interpret_session))
        .route("/api/sessions/{name}/summary", post(handlers::web::get_session_summary))
        .route("/api/sessions/{name}/capture", post(handlers::web::capture_session_output))
        // Web UI — Process monitoring
        .route("/api/processes", get(handlers::web::list_processes))
        .route("/api/processes/clean", post(handlers::web::kill_stale_processes))
        // Web UI — Bot status
        .route("/api/bot/status", get(handlers::web::get_bot_status))
        // Web UI — Config
        .route("/api/config", get(handlers::web::get_config).post(handlers::web::save_config))
        // Apply middleware
        .layer(cors)
        .with_state(state);

    // Attach static file serving when the web-dist directory is present.
    // The directory is configurable via the AIC_WEB_DIR environment variable.
    let web_dir = std::env::var("AIC_WEB_DIR").unwrap_or_else(|_| {
        // Default: look for web-dist next to the running binary.
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));
        if let Some(dir) = exe_dir {
            let candidate = dir.join("web-dist");
            if candidate.is_dir() {
                return candidate.to_string_lossy().to_string();
            }
        }
        "web-dist".to_string()
    });

    if std::path::Path::new(&web_dir).is_dir() {
        info!("serving web UI from {}", web_dir);
        router.fallback_service(
            ServeDir::new(&web_dir).append_index_html_on_directories(true),
        )
    } else {
        router
    }
}

/// Starts the API server.
pub async fn serve(config: ApiConfig, state: AppState) -> Result<(), std::io::Error> {
    let addr = config.bind_address();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("API server listening on {}", addr);
    axum::serve(listener, create_router(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    use commander_adapters::AdapterRegistry;
    use commander_events::EventManager;
    use commander_models::{Event, EventType, WorkItem, WorkPriority};
    use commander_persistence::{EventStore, WorkStore};
    use commander_work::WorkQueue;
    use serde_json::json;
    use tempfile::tempdir;

    fn make_test_state() -> AppState {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);

        let event_store = EventStore::new(&path);
        let work_store = WorkStore::new(&path);

        AppState::new_with_storage(
            ApiConfig::default(),
            None,
            EventManager::new(event_store),
            WorkQueue::new(work_store),
            AdapterRegistry::new(),
            path,
        )
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = make_test_state();
        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server.get("/api/health").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["status"], "ok");
        assert!(!body["version"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_list_projects_empty() {
        let state = make_test_state();
        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server.get("/api/projects").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["total"], 0);
        assert!(body["projects"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_create_project() {
        let state = make_test_state();
        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server
            .post("/api/projects")
            .json(&json!({
                "name": "test-project",
                "path": "/tmp/test"
            }))
            .await;

        response.assert_status(axum::http::StatusCode::CREATED);

        let body: serde_json::Value = response.json();
        assert!(!body["id"].as_str().unwrap().is_empty());
        assert_eq!(body["message"], "project created");
    }

    #[tokio::test]
    async fn test_get_project() {
        let state = make_test_state();

        // Create a project first
        let project = commander_models::Project::new("/tmp/test", "test");
        let project_id = project.id.as_str().to_string();
        state.save_project(project).await;

        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server.get(&format!("/api/projects/{}", project_id)).await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["name"], "test");
    }

    #[tokio::test]
    async fn test_delete_project() {
        let state = make_test_state();

        // Create a project first
        let project = commander_models::Project::new("/tmp/test", "test");
        let project_id = project.id.as_str().to_string();
        state.save_project(project).await;

        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server
            .delete(&format!("/api/projects/{}", project_id))
            .await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["message"], "project deleted");

        // Verify it's gone
        let response = server.get(&format!("/api/projects/{}", project_id)).await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_adapters() {
        let state = make_test_state();
        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server.get("/api/adapters").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert!(body["total"].as_u64().unwrap() >= 2);

        let adapters = body["adapters"].as_array().unwrap();
        let ids: Vec<&str> = adapters
            .iter()
            .map(|a| a["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"mpm"));
    }

    #[tokio::test]
    async fn test_list_events() {
        let state = make_test_state();

        // Add some events
        let event = Event::new("proj-1", EventType::Status, "Test event");
        state.event_manager.emit(event).unwrap();

        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server.get("/api/events").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["total"], 1);
    }

    #[tokio::test]
    async fn test_list_events_with_filter() {
        let state = make_test_state();

        // Add events for different projects
        state
            .event_manager
            .emit(Event::new("proj-1", EventType::Status, "Event 1"))
            .unwrap();
        state
            .event_manager
            .emit(Event::new("proj-2", EventType::Status, "Event 2"))
            .unwrap();

        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server.get("/api/events?project_id=proj-1").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["total"], 1);
    }

    #[tokio::test]
    async fn test_list_work() {
        let state = make_test_state();

        // Add some work
        let item = WorkItem::with_priority("proj-1", "Test task", WorkPriority::High);
        state.work_queue.enqueue(item).unwrap();

        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server.get("/api/work").await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["total"], 1);
    }

    #[tokio::test]
    async fn test_create_work() {
        let state = make_test_state();
        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server
            .post("/api/work")
            .json(&json!({
                "project_id": "proj-1",
                "content": "Build it",
                "priority": "high"
            }))
            .await;

        response.assert_status(axum::http::StatusCode::CREATED);

        let body: serde_json::Value = response.json();
        assert!(!body["id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_cors_headers() {
        let state = make_test_state();
        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server.get("/api/health").await;

        // CORS headers should be present
        assert!(response.headers().contains_key("access-control-allow-origin"));
    }

    #[tokio::test]
    async fn test_not_found() {
        let state = make_test_state();
        let app = create_router(state);
        let server = TestServer::new(app);

        let response = server.get("/api/projects/nonexistent").await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);

        let body: serde_json::Value = response.json();
        assert!(body["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_bad_request() {
        let state = make_test_state();
        let app = create_router(state);
        let server = TestServer::new(app);

        // Try to create project with invalid adapter
        let response = server
            .post("/api/projects")
            .json(&json!({
                "name": "test",
                "path": "/tmp/test",
                "adapter": "nonexistent-adapter"
            }))
            .await;

        response.assert_status(axum::http::StatusCode::BAD_REQUEST);
    }
}
