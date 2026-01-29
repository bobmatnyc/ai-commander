//! Adapter handlers.

use axum::{extract::State, Json};

use crate::state::AppState;
use crate::types::{AdapterListResponse, AdapterSummary};

/// GET /api/adapters - List all available adapters.
pub async fn list_adapters(State(state): State<AppState>) -> Json<AdapterListResponse> {
    let adapter_ids = state.adapter_registry.list();
    let adapters: Vec<AdapterSummary> = adapter_ids
        .iter()
        .filter_map(|id| state.adapter_registry.get(id))
        .map(|adapter| AdapterSummary::from(adapter.info()))
        .collect();

    let total = adapters.len();

    Json(AdapterListResponse { adapters, total })
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
    async fn test_list_adapters() {
        let state = make_test_state();
        let response = list_adapters(State(state)).await;

        // Should have at least claude-code and mpm adapters
        assert!(response.total >= 2);
        assert!(response.adapters.iter().any(|a| a.id == "claude-code"));
        assert!(response.adapters.iter().any(|a| a.id == "mpm"));
    }
}
