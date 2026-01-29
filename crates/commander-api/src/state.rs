//! Application state shared across handlers.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use commander_adapters::AdapterRegistry;
use commander_events::EventManager;
use commander_models::Project;
use commander_runtime::Runtime;
use commander_work::WorkQueue;

use crate::config::ApiConfig;

/// Application state shared across all handlers.
#[derive(Clone)]
pub struct AppState {
    /// API configuration.
    pub config: Arc<ApiConfig>,
    /// Runtime for managing instances (optional - may be None in tests).
    pub runtime: Option<Arc<RwLock<Runtime>>>,
    /// Event manager.
    pub event_manager: Arc<EventManager>,
    /// Work queue.
    pub work_queue: Arc<WorkQueue>,
    /// Adapter registry.
    pub adapter_registry: Arc<AdapterRegistry>,
    /// In-memory project store.
    pub projects: Arc<RwLock<HashMap<String, Project>>>,
}

impl AppState {
    /// Creates a new AppState with all components.
    pub fn new(
        config: ApiConfig,
        runtime: Option<Runtime>,
        event_manager: EventManager,
        work_queue: WorkQueue,
        adapter_registry: AdapterRegistry,
    ) -> Self {
        Self {
            config: Arc::new(config),
            runtime: runtime.map(|r| Arc::new(RwLock::new(r))),
            event_manager: Arc::new(event_manager),
            work_queue: Arc::new(work_queue),
            adapter_registry: Arc::new(adapter_registry),
            projects: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Gets a project by ID.
    pub async fn get_project(&self, id: &str) -> Option<Project> {
        let projects = self.projects.read().await;
        projects.get(id).cloned()
    }

    /// Saves a project.
    pub async fn save_project(&self, project: Project) {
        let mut projects = self.projects.write().await;
        projects.insert(project.id.as_str().to_string(), project);
    }

    /// Removes a project by ID.
    pub async fn remove_project(&self, id: &str) -> Option<Project> {
        let mut projects = self.projects.write().await;
        projects.remove(id)
    }

    /// Lists all projects.
    pub async fn list_projects(&self) -> Vec<Project> {
        let projects = self.projects.read().await;
        projects.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_persistence::{EventStore, WorkStore};
    use tempfile::tempdir;

    fn make_test_state() -> AppState {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);

        let event_store = EventStore::new(&path);
        let work_store = WorkStore::new(&path);

        AppState::new(
            ApiConfig::default(),
            None, // No runtime in tests
            EventManager::new(event_store),
            WorkQueue::new(work_store),
            AdapterRegistry::new(),
        )
    }

    #[tokio::test]
    async fn test_app_state_project_crud() {
        let state = make_test_state();

        // Initially empty
        assert!(state.list_projects().await.is_empty());

        // Save a project
        let project = Project::new("/path/to/project", "test-project");
        let project_id = project.id.as_str().to_string();
        state.save_project(project).await;

        // Can retrieve
        let retrieved = state.get_project(&project_id).await.unwrap();
        assert_eq!(retrieved.name, "test-project");

        // List contains it
        assert_eq!(state.list_projects().await.len(), 1);

        // Can remove
        let removed = state.remove_project(&project_id).await.unwrap();
        assert_eq!(removed.name, "test-project");

        // Now empty
        assert!(state.list_projects().await.is_empty());
    }

    #[tokio::test]
    async fn test_app_state_get_nonexistent() {
        let state = make_test_state();
        assert!(state.get_project("nonexistent").await.is_none());
    }
}
