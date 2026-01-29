//! Health check handler.

use axum::{extract::State, Json};

use crate::state::AppState;
use crate::types::HealthResponse;

/// GET /api/health - Health check endpoint.
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.config.uptime_seconds(),
    })
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
    async fn test_health_handler() {
        let state = make_test_state();
        let response = health(State(state)).await;

        assert_eq!(response.status, "ok");
        assert!(!response.version.is_empty());
    }
}
