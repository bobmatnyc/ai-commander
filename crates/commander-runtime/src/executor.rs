//! Runtime executor for managing running instances.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info};

use commander_adapters::RuntimeAdapter;
use commander_models::{Project, ProjectId, ProjectState};
use commander_tmux::TmuxOrchestrator;

use crate::config::RuntimeConfig;
use crate::error::{Result, RuntimeError};
use crate::event::RuntimeEvent;

/// A running instance of an AI tool.
pub struct RunningInstance {
    /// Project ID.
    pub project_id: ProjectId,
    /// Tmux session name.
    pub session_name: String,
    /// The runtime adapter being used.
    pub adapter: Arc<dyn RuntimeAdapter>,
    /// When the instance was started.
    pub started_at: DateTime<Utc>,
    /// Last captured output.
    pub last_output: Option<String>,
    /// Current state.
    pub state: ProjectState,
}

impl fmt::Debug for RunningInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunningInstance")
            .field("project_id", &self.project_id)
            .field("session_name", &self.session_name)
            .field("adapter", &self.adapter.info().id)
            .field("started_at", &self.started_at)
            .field("last_output", &self.last_output.is_some())
            .field("state", &self.state)
            .finish()
    }
}

impl RunningInstance {
    /// Creates a new running instance.
    pub fn new(
        project_id: ProjectId,
        session_name: String,
        adapter: Arc<dyn RuntimeAdapter>,
    ) -> Self {
        Self {
            project_id,
            session_name,
            adapter,
            started_at: Utc::now(),
            last_output: None,
            state: ProjectState::Idle,
        }
    }
}

/// Manages running instances and their lifecycle.
pub struct RuntimeExecutor {
    /// Configuration.
    config: RuntimeConfig,
    /// Tmux orchestrator.
    tmux: TmuxOrchestrator,
    /// Running instances keyed by project ID.
    instances: Arc<RwLock<HashMap<String, RunningInstance>>>,
    /// Event broadcast channel.
    event_tx: broadcast::Sender<RuntimeEvent>,
}

impl RuntimeExecutor {
    /// Creates a new runtime executor.
    pub fn new(config: RuntimeConfig) -> Result<Self> {
        let tmux = TmuxOrchestrator::new()?;
        let (event_tx, _) = broadcast::channel(256);

        Ok(Self {
            config,
            tmux,
            instances: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        })
    }

    /// Creates a new runtime executor with a provided tmux orchestrator.
    pub fn with_tmux(config: RuntimeConfig, tmux: TmuxOrchestrator) -> Result<Self> {
        let (event_tx, _) = broadcast::channel(256);

        Ok(Self {
            config,
            tmux,
            instances: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        })
    }

    /// Returns the configuration.
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Returns a reference to the tmux orchestrator.
    pub fn tmux(&self) -> &TmuxOrchestrator {
        &self.tmux
    }

    /// Returns the instances map for internal use.
    pub(crate) fn instances(&self) -> Arc<RwLock<HashMap<String, RunningInstance>>> {
        Arc::clone(&self.instances)
    }

    /// Start an instance for a project.
    pub async fn start(
        &self,
        project: &Project,
        adapter: Arc<dyn RuntimeAdapter>,
    ) -> Result<()> {
        let project_id_str = project.id.as_str().to_string();

        // Check if instance already exists
        {
            let instances = self.instances.read().await;
            if instances.contains_key(&project_id_str) {
                return Err(RuntimeError::InstanceExists(project_id_str));
            }
        }

        // Check max instances
        {
            let instances = self.instances.read().await;
            if instances.len() >= self.config.max_instances {
                return Err(RuntimeError::MaxInstancesReached(self.config.max_instances));
            }
        }

        // Generate session name from project
        let session_name = format!("cmd-{}", project.name.replace([' ', '.', '/'], "-"));

        // Get launch command
        let (cmd, args) = adapter.launch_command(&project.path);
        debug!(
            project_id = %project.id,
            session = %session_name,
            cmd = %cmd,
            args = ?args,
            "starting instance"
        );

        // Create tmux session
        self.tmux.create_session(&session_name)?;

        // Send launch command to the session
        let full_command = if args.is_empty() {
            cmd.clone()
        } else {
            format!("{} {}", cmd, args.join(" "))
        };
        self.tmux.send_line(&session_name, None, &full_command)?;

        // Create running instance
        let instance = RunningInstance::new(
            project.id.clone(),
            session_name.clone(),
            adapter,
        );

        // Add to instances map
        {
            let mut instances = self.instances.write().await;
            instances.insert(project_id_str.clone(), instance);
        }

        info!(
            project_id = %project.id,
            session = %session_name,
            "instance started"
        );

        // Emit event
        self.emit_event(RuntimeEvent::InstanceStarted {
            project_id: project.id.clone(),
            session: session_name,
        });

        Ok(())
    }

    /// Stop an instance.
    pub async fn stop(&self, project_id: &ProjectId, force: bool) -> Result<()> {
        let project_id_str = project_id.as_str().to_string();

        // Get and remove instance
        let instance = {
            let mut instances = self.instances.write().await;
            instances.remove(&project_id_str)
        };

        let instance = match instance {
            Some(i) => i,
            None => return Err(RuntimeError::InstanceNotFound(project_id_str)),
        };

        debug!(
            project_id = %project_id,
            session = %instance.session_name,
            force = force,
            "stopping instance"
        );

        // If not forcing, try to send exit command first
        if !force {
            // Try to send Ctrl+C first
            let _ = self.tmux.send_keys(&instance.session_name, None, "C-c");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Destroy the tmux session
        if self.tmux.session_exists(&instance.session_name) {
            self.tmux.destroy_session(&instance.session_name)?;
        }

        info!(
            project_id = %project_id,
            session = %instance.session_name,
            "instance stopped"
        );

        // Emit event
        self.emit_event(RuntimeEvent::InstanceStopped {
            project_id: project_id.clone(),
        });

        Ok(())
    }

    /// Get current state of an instance.
    pub async fn get_state(&self, project_id: &ProjectId) -> Option<ProjectState> {
        let project_id_str = project_id.as_str();
        let instances = self.instances.read().await;
        instances.get(project_id_str).map(|i| i.state)
    }

    /// Subscribe to runtime events.
    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.event_tx.subscribe()
    }

    /// List all running instance project IDs.
    pub async fn list_instances(&self) -> Vec<ProjectId> {
        let instances = self.instances.read().await;
        instances.values().map(|i| i.project_id.clone()).collect()
    }

    /// Check if an instance is running for the given project.
    pub async fn has_instance(&self, project_id: &ProjectId) -> bool {
        let project_id_str = project_id.as_str();
        let instances = self.instances.read().await;
        instances.contains_key(project_id_str)
    }

    /// Emit an event to all subscribers.
    pub fn emit_event(&self, event: RuntimeEvent) {
        // Ignore send errors (no receivers)
        let _ = self.event_tx.send(event);
    }

    /// Update state for an instance and emit event.
    pub async fn update_state(&self, project_id: &ProjectId, state: ProjectState) {
        let project_id_str = project_id.as_str();

        let changed = {
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(project_id_str) {
                if instance.state != state {
                    instance.state = state;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if changed {
            self.emit_event(RuntimeEvent::StateChanged {
                project_id: project_id.clone(),
                state,
            });
        }
    }

    /// Capture output from an instance.
    pub async fn capture_output(&self, project_id: &ProjectId) -> Result<Option<String>> {
        let project_id_str = project_id.as_str();

        let session_name = {
            let instances = self.instances.read().await;
            instances.get(project_id_str).map(|i| i.session_name.clone())
        };

        let session_name = match session_name {
            Some(s) => s,
            None => return Err(RuntimeError::InstanceNotFound(project_id_str.to_string())),
        };

        let output = self.tmux.capture_output(&session_name, None, Some(50))?;

        // Update last output
        {
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(project_id_str) {
                instance.last_output = Some(output.clone());
            }
        }

        Ok(Some(output))
    }

    /// Get instance count.
    pub async fn instance_count(&self) -> usize {
        self.instances.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_adapters::{AdapterInfo, OutputAnalysis, RuntimeState};

    // Mock adapter for testing
    struct MockAdapter {
        info: AdapterInfo,
    }

    impl MockAdapter {
        fn new() -> Self {
            Self {
                info: AdapterInfo {
                    id: "mock".to_string(),
                    name: "Mock Adapter".to_string(),
                    description: "A mock adapter for testing".to_string(),
                    command: "echo".to_string(),
                    default_args: vec!["hello".to_string()],
                },
            }
        }
    }

    impl RuntimeAdapter for MockAdapter {
        fn info(&self) -> &AdapterInfo {
            &self.info
        }

        fn launch_command(&self, _project_path: &str) -> (String, Vec<String>) {
            (self.info.command.clone(), self.info.default_args.clone())
        }

        fn analyze_output(&self, output: &str) -> OutputAnalysis {
            OutputAnalysis {
                state: if output.contains(">") {
                    RuntimeState::Idle
                } else {
                    RuntimeState::Working
                },
                confidence: 1.0,
                errors: vec![],
                data: HashMap::new(),
            }
        }

        fn idle_patterns(&self) -> &[&str] {
            &[">"]
        }

        fn error_patterns(&self) -> &[&str] {
            &["error"]
        }
    }

    #[test]
    fn test_running_instance_new() {
        let project_id = ProjectId::from_string("test-project");
        let adapter = Arc::new(MockAdapter::new());
        let instance = RunningInstance::new(
            project_id.clone(),
            "test-session".to_string(),
            adapter,
        );

        assert_eq!(instance.project_id, project_id);
        assert_eq!(instance.session_name, "test-session");
        assert_eq!(instance.state, ProjectState::Idle);
        assert!(instance.last_output.is_none());
    }

    #[tokio::test]
    async fn test_executor_list_instances_empty() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = RuntimeExecutor::new(config).unwrap();

        let instances = executor.list_instances().await;
        assert!(instances.is_empty());
    }

    #[tokio::test]
    async fn test_executor_instance_count() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = RuntimeExecutor::new(config).unwrap();

        assert_eq!(executor.instance_count().await, 0);
    }

    #[tokio::test]
    async fn test_executor_has_instance() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = RuntimeExecutor::new(config).unwrap();
        let project_id = ProjectId::from_string("nonexistent");

        assert!(!executor.has_instance(&project_id).await);
    }

    #[tokio::test]
    async fn test_executor_get_state_nonexistent() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = RuntimeExecutor::new(config).unwrap();
        let project_id = ProjectId::from_string("nonexistent");

        assert!(executor.get_state(&project_id).await.is_none());
    }

    #[tokio::test]
    async fn test_executor_subscribe() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = RuntimeExecutor::new(config).unwrap();

        let mut rx = executor.subscribe();
        let project_id = ProjectId::from_string("test");

        executor.emit_event(RuntimeEvent::InstanceStopped {
            project_id: project_id.clone(),
        });

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, RuntimeEvent::InstanceStopped { .. }));
    }

    #[tokio::test]
    async fn test_executor_stop_nonexistent() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = RuntimeExecutor::new(config).unwrap();
        let project_id = ProjectId::from_string("nonexistent");

        let result = executor.stop(&project_id, false).await;
        assert!(matches!(result, Err(RuntimeError::InstanceNotFound(_))));
    }

    #[tokio::test]
    async fn test_executor_capture_output_nonexistent() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = RuntimeExecutor::new(config).unwrap();
        let project_id = ProjectId::from_string("nonexistent");

        let result = executor.capture_output(&project_id).await;
        assert!(matches!(result, Err(RuntimeError::InstanceNotFound(_))));
    }

    #[tokio::test]
    async fn test_executor_update_state() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = RuntimeExecutor::new(config).unwrap();
        let project_id = ProjectId::from_string("test");

        // Subscribe before updating
        let mut rx = executor.subscribe();

        // Update state for non-existent instance (should not emit event)
        executor.update_state(&project_id, ProjectState::Working).await;

        // Use tokio::select! to check if no event was sent
        tokio::select! {
            _ = rx.recv() => {
                panic!("should not receive event for non-existent instance");
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                // Expected - no event should be sent
            }
        }
    }

    #[tokio::test]
    async fn test_executor_config_access() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::new()
            .with_max_instances(5)
            .with_poll_interval(std::time::Duration::from_millis(100));
        let executor = RuntimeExecutor::new(config).unwrap();

        assert_eq!(executor.config().max_instances, 5);
        assert_eq!(executor.config().poll_interval, std::time::Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_broadcast_multiple_subscribers() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = RuntimeExecutor::new(config).unwrap();

        // Multiple subscribers
        let mut rx1 = executor.subscribe();
        let mut rx2 = executor.subscribe();
        let mut rx3 = executor.subscribe();

        let project_id = ProjectId::from_string("test");

        // Emit event
        executor.emit_event(RuntimeEvent::StateChanged {
            project_id: project_id.clone(),
            state: ProjectState::Working,
        });

        // All should receive
        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        let e3 = rx3.recv().await.unwrap();

        assert!(matches!(e1, RuntimeEvent::StateChanged { state: ProjectState::Working, .. }));
        assert!(matches!(e2, RuntimeEvent::StateChanged { state: ProjectState::Working, .. }));
        assert!(matches!(e3, RuntimeEvent::StateChanged { state: ProjectState::Working, .. }));
    }
}
