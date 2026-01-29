//! Main runtime manager.

use std::sync::Arc;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{debug, info};

use crate::config::RuntimeConfig;
use crate::error::{Result, RuntimeError};
use crate::executor::RuntimeExecutor;
use crate::poller::OutputPoller;

/// Main runtime manager combining executor and poller.
pub struct Runtime {
    /// The executor for managing instances.
    executor: Arc<RuntimeExecutor>,
    /// Handle to the poller task.
    poller_handle: Option<JoinHandle<()>>,
    /// Shutdown signal sender.
    shutdown_tx: watch::Sender<bool>,
    /// Shutdown signal receiver (for cloning to poller).
    shutdown_rx: watch::Receiver<bool>,
    /// Whether the runtime has been started.
    started: bool,
}

impl Runtime {
    /// Create a new runtime with the given configuration.
    pub async fn new(config: RuntimeConfig) -> Result<Self> {
        let executor = RuntimeExecutor::new(config)?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            executor: Arc::new(executor),
            poller_handle: None,
            shutdown_tx,
            shutdown_rx,
            started: false,
        })
    }

    /// Create a new runtime with a provided executor.
    pub fn with_executor(executor: RuntimeExecutor) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Self {
            executor: Arc::new(executor),
            poller_handle: None,
            shutdown_tx,
            shutdown_rx,
            started: false,
        }
    }

    /// Start the runtime (begins polling).
    pub async fn start(&mut self) -> Result<()> {
        if self.started {
            return Err(RuntimeError::AlreadyStarted);
        }

        info!("starting runtime");

        // Create and spawn the poller
        let executor = Arc::clone(&self.executor);
        let shutdown_rx = self.shutdown_rx.clone();

        let handle = tokio::spawn(async move {
            let mut poller = OutputPoller::new(executor, shutdown_rx);
            poller.run().await;
        });

        self.poller_handle = Some(handle);
        self.started = true;

        debug!("runtime started");

        Ok(())
    }

    /// Stop the runtime gracefully.
    pub async fn shutdown(&mut self) -> Result<()> {
        if !self.started {
            return Err(RuntimeError::NotStarted);
        }

        info!("shutting down runtime");

        // Send shutdown signal
        self.shutdown_tx.send(true).map_err(|e| {
            RuntimeError::Shutdown(format!("failed to send shutdown signal: {}", e))
        })?;

        // Wait for poller to stop
        if let Some(handle) = self.poller_handle.take() {
            debug!("waiting for poller to stop");
            handle.await.map_err(|e| {
                RuntimeError::Shutdown(format!("poller task panicked: {}", e))
            })?;
        }

        // Stop all instances
        let instances = self.executor.list_instances().await;
        for project_id in instances {
            debug!(project_id = %project_id, "stopping instance");
            if let Err(e) = self.executor.stop(&project_id, true).await {
                debug!(
                    project_id = %project_id,
                    error = %e,
                    "failed to stop instance during shutdown"
                );
            }
        }

        self.started = false;

        info!("runtime stopped");

        Ok(())
    }

    /// Get the executor for starting/stopping instances.
    pub fn executor(&self) -> Arc<RuntimeExecutor> {
        Arc::clone(&self.executor)
    }

    /// Check if the runtime has been started.
    pub fn is_started(&self) -> bool {
        self.started
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Send shutdown signal if still running
        if self.started {
            let _ = self.shutdown_tx.send(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_runtime_new() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let runtime = Runtime::new(config).await;
        assert!(runtime.is_ok());

        let runtime = runtime.unwrap();
        assert!(!runtime.is_started());
    }

    #[tokio::test]
    async fn test_runtime_start_stop() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::new()
            .with_poll_interval(Duration::from_millis(10));
        let mut runtime = Runtime::new(config).await.unwrap();

        // Start
        runtime.start().await.unwrap();
        assert!(runtime.is_started());

        // Let it run briefly
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Stop
        runtime.shutdown().await.unwrap();
        assert!(!runtime.is_started());
    }

    #[tokio::test]
    async fn test_runtime_double_start() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let mut runtime = Runtime::new(config).await.unwrap();

        runtime.start().await.unwrap();

        let result = runtime.start().await;
        assert!(matches!(result, Err(RuntimeError::AlreadyStarted)));

        runtime.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_runtime_shutdown_not_started() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let mut runtime = Runtime::new(config).await.unwrap();

        let result = runtime.shutdown().await;
        assert!(matches!(result, Err(RuntimeError::NotStarted)));
    }

    #[tokio::test]
    async fn test_runtime_executor_access() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let runtime = Runtime::new(config).await.unwrap();

        let executor = runtime.executor();
        assert_eq!(executor.instance_count().await, 0);
    }

    #[tokio::test]
    async fn test_runtime_event_subscription() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::new()
            .with_poll_interval(Duration::from_millis(10));
        let mut runtime = Runtime::new(config).await.unwrap();

        // Subscribe before starting
        let mut rx = runtime.executor().subscribe();

        runtime.start().await.unwrap();

        // Emit a test event
        let project_id = commander_models::ProjectId::from_string("test");
        runtime.executor().emit_event(crate::RuntimeEvent::InstanceStopped {
            project_id: project_id.clone(),
        });

        // Should receive the event
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, crate::RuntimeEvent::InstanceStopped { .. }));

        runtime.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_runtime_concurrent_event_receivers() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let runtime = Runtime::new(config).await.unwrap();

        // Multiple subscribers
        let mut rx1 = runtime.executor().subscribe();
        let mut rx2 = runtime.executor().subscribe();

        let project_id = commander_models::ProjectId::from_string("test");
        runtime.executor().emit_event(crate::RuntimeEvent::InstanceStopped {
            project_id: project_id.clone(),
        });

        // Both should receive
        let event1 = rx1.recv().await.unwrap();
        let event2 = rx2.recv().await.unwrap();

        assert!(matches!(event1, crate::RuntimeEvent::InstanceStopped { .. }));
        assert!(matches!(event2, crate::RuntimeEvent::InstanceStopped { .. }));
    }

    #[tokio::test]
    async fn test_runtime_with_executor() {
        // Skip if tmux not available
        if !commander_tmux::TmuxOrchestrator::is_available() {
            return;
        }

        let config = RuntimeConfig::default();
        let executor = crate::RuntimeExecutor::new(config.clone()).unwrap();
        let runtime = Runtime::with_executor(executor);

        assert!(!runtime.is_started());
    }
}
