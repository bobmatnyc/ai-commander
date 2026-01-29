//! Output poller for monitoring tmux sessions.

use std::sync::Arc;

use tokio::sync::watch;
use tokio::time::interval;
use tracing::{debug, trace, warn};

use commander_adapters::RuntimeState;
use commander_models::{ProjectId, ProjectState};

use crate::event::RuntimeEvent;
use crate::executor::RuntimeExecutor;

/// Polls tmux sessions for output changes.
pub struct OutputPoller {
    /// The executor to poll.
    executor: Arc<RuntimeExecutor>,
    /// Shutdown signal receiver.
    shutdown: watch::Receiver<bool>,
}

impl OutputPoller {
    /// Creates a new output poller.
    pub fn new(executor: Arc<RuntimeExecutor>, shutdown: watch::Receiver<bool>) -> Self {
        Self { executor, shutdown }
    }

    /// Run the polling loop until shutdown signal.
    pub async fn run(&mut self) {
        let poll_interval = self.executor.config().poll_interval;
        let mut ticker = interval(poll_interval);

        debug!(
            poll_interval_ms = poll_interval.as_millis(),
            "starting output poller"
        );

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.poll_all().await;
                }
                _ = self.shutdown.changed() => {
                    if *self.shutdown.borrow() {
                        debug!("poller received shutdown signal");
                        break;
                    }
                }
            }
        }

        debug!("output poller stopped");
    }

    /// Poll all instances for output changes.
    async fn poll_all(&self) {
        // Collect state changes to process after releasing the lock
        let mut state_changes: Vec<(ProjectId, ProjectState)> = Vec::new();

        {
            let instances = self.executor.instances();
            let instances = instances.read().await;

            for (project_id_str, instance) in instances.iter() {
                trace!(
                    project_id = %project_id_str,
                    session = %instance.session_name,
                    "polling instance"
                );

                // Capture current output
                let output = match self.executor.tmux().capture_output(
                    &instance.session_name,
                    None,
                    Some(50),
                ) {
                    Ok(o) => o,
                    Err(e) => {
                        warn!(
                            project_id = %project_id_str,
                            error = %e,
                            "failed to capture output"
                        );
                        continue;
                    }
                };

                // Check if output changed
                let changed = match &instance.last_output {
                    Some(last) => last != &output,
                    None => true,
                };

                if changed {
                    trace!(
                        project_id = %project_id_str,
                        "output changed"
                    );

                    // Emit output received event
                    self.executor.emit_event(RuntimeEvent::OutputReceived {
                        project_id: instance.project_id.clone(),
                        output: output.clone(),
                    });

                    // Analyze output for state changes
                    let analysis = instance.adapter.analyze_output(&output);
                    let new_state = match analysis.state {
                        RuntimeState::Idle => ProjectState::Idle,
                        RuntimeState::Working => ProjectState::Working,
                        RuntimeState::Error => ProjectState::Error,
                        RuntimeState::Starting => ProjectState::Working,
                        RuntimeState::Stopped => ProjectState::Idle,
                    };

                    if new_state != instance.state {
                        state_changes.push((instance.project_id.clone(), new_state));
                    }
                }
            }
        } // Release read lock here

        // Process state changes outside the lock
        for (project_id, new_state) in state_changes {
            self.executor.update_state(&project_id, new_state).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuntimeConfig;
    use std::collections::HashMap;
    use std::time::Duration;
    use commander_adapters::{AdapterInfo, OutputAnalysis, RuntimeAdapter};

    #[allow(dead_code)]
    struct MockAdapter;

    impl RuntimeAdapter for MockAdapter {
        fn info(&self) -> &AdapterInfo {
            static INFO: std::sync::OnceLock<AdapterInfo> = std::sync::OnceLock::new();
            INFO.get_or_init(|| AdapterInfo {
                id: "mock".to_string(),
                name: "Mock".to_string(),
                description: "Mock adapter".to_string(),
                command: "echo".to_string(),
                default_args: vec![],
            })
        }

        fn launch_command(&self, _project_path: &str) -> (String, Vec<String>) {
            ("echo".to_string(), vec!["hello".to_string()])
        }

        fn analyze_output(&self, _output: &str) -> OutputAnalysis {
            OutputAnalysis {
                state: RuntimeState::Idle,
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

    #[tokio::test]
    async fn test_poller_shutdown() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::new()
            .with_poll_interval(Duration::from_millis(10));
        let executor = Arc::new(RuntimeExecutor::new(config).unwrap());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let mut poller = OutputPoller::new(Arc::clone(&executor), shutdown_rx);

        // Spawn poller in background
        let handle = tokio::spawn(async move {
            poller.run().await;
        });

        // Let it run for a bit
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Send shutdown signal
        shutdown_tx.send(true).unwrap();

        // Wait for poller to stop
        let result = tokio::time::timeout(Duration::from_millis(100), handle).await;
        assert!(result.is_ok(), "poller should stop after shutdown signal");
    }
}
