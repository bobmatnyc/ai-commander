//! Application state shared across handlers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{broadcast, RwLock};

use commander_adapters::AdapterRegistry;
use commander_core::config;
use commander_events::EventManager;
use commander_models::Project;
use commander_runtime::Runtime;
use commander_tmux::TmuxOrchestrator;
use commander_work::WorkQueue;

use crate::config::ApiConfig;
use crate::web_clients::WebClientStore;

/// Cached GitHub statistics for a project repository.
#[derive(Debug, Clone, Serialize, Default)]
pub struct GitHubStats {
    /// Number of open issues (excludes pull requests).
    pub open_issues: u32,
    /// Number of open pull requests.
    pub open_prs: u32,
    /// Repository in "owner/repo" format.
    pub repo: String,
}

/// An event broadcast to SSE clients about session activity.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SessionEvent {
    /// Name of the tmux session.
    pub session_name: String,
    /// Event type: "interpretation", "status_change", or "error".
    pub event_type: String,
    /// The event content (interpreted output, status message, etc.).
    pub content: String,
    /// Unix epoch seconds.
    pub timestamp: u64,
    /// Adapter nickname: "claude", "mpm", "auggie", "codex", "shell".
    pub adapter: String,
    /// When true, the frontend should update the last interpretation rather than
    /// appending a new message (prevents repeated "Processing..." bubbles).
    #[serde(default)]
    pub is_update: bool,
}

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
    /// Web client token store for browser-based auth.
    pub web_clients: WebClientStore,
    /// Tmux orchestrator (optional - unavailable when tmux is not installed).
    pub tmux: Option<Arc<TmuxOrchestrator>>,
    /// Broadcast channel for SSE session events.
    pub event_tx: broadcast::Sender<SessionEvent>,
    /// Maps session name → adapter nickname (e.g. "claude", "mpm").
    pub session_adapters: Arc<RwLock<HashMap<String, String>>>,
    /// Cached GitHub stats per project directory name.
    pub github_stats: Arc<RwLock<HashMap<String, GitHubStats>>>,
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
        // Default storage directory for web clients (same dir as other runtime state).
        let storage_dir = config::runtime_state_dir();
        Self::new_with_storage(config, runtime, event_manager, work_queue, adapter_registry, storage_dir)
    }

    /// Creates a new AppState with an explicit storage directory (useful in tests).
    pub fn new_with_storage(
        config: ApiConfig,
        runtime: Option<Runtime>,
        event_manager: EventManager,
        work_queue: WorkQueue,
        adapter_registry: AdapterRegistry,
        storage_dir: PathBuf,
    ) -> Self {
        let tmux = TmuxOrchestrator::new().ok().map(Arc::new);
        let (event_tx, _rx) = broadcast::channel(64);

        Self {
            config: Arc::new(config),
            runtime: runtime.map(|r| Arc::new(RwLock::new(r))),
            event_manager: Arc::new(event_manager),
            work_queue: Arc::new(work_queue),
            adapter_registry: Arc::new(adapter_registry),
            projects: Arc::new(RwLock::new(HashMap::new())),
            web_clients: WebClientStore::new(&storage_dir),
            tmux,
            event_tx,
            session_adapters: Arc::new(RwLock::new(HashMap::new())),
            github_stats: Arc::new(RwLock::new(HashMap::new())),
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

        AppState::new_with_storage(
            ApiConfig::default(),
            None, // No runtime in tests
            EventManager::new(event_store),
            WorkQueue::new(work_store),
            AdapterRegistry::new(),
            path,
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
