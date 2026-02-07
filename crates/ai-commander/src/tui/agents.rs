//! Agent orchestration integration for TUI (feature-gated).

#[cfg(feature = "agents")]
use std::sync::Arc;
#[cfg(feature = "agents")]
use tokio::runtime::Handle as TokioHandle;

#[cfg(feature = "agents")]
use commander_orchestrator::AgentOrchestrator;

#[cfg(feature = "agents")]
use super::app::{App, Message};

#[cfg(feature = "agents")]
impl App {
    /// Set the tokio runtime handle for async operations.
    pub fn set_runtime_handle(&mut self, handle: Arc<TokioHandle>) {
        self.runtime_handle = Some(handle);
    }

    /// Initialize the agent orchestrator (when agents feature is enabled).
    ///
    /// This should be called during app setup to enable agent-based processing.
    /// If initialization fails, the app continues to work without agents.
    pub async fn init_orchestrator(&mut self) -> Result<(), String> {
        match AgentOrchestrator::new().await {
            Ok(orchestrator) => {
                self.orchestrator = Some(orchestrator);
                self.messages.push(Message::system("Agent orchestrator initialized"));
                Ok(())
            }
            Err(e) => {
                let msg = format!("Failed to initialize orchestrator: {}", e);
                self.messages.push(Message::system(&msg));
                Err(msg)
            }
        }
    }

    /// Initialize the agent orchestrator synchronously using the runtime handle.
    ///
    /// This wraps the async initialization for use in sync contexts like TUI startup.
    /// Returns true if initialization succeeded.
    pub fn init_orchestrator_sync(&mut self) -> bool {
        let handle = match &self.runtime_handle {
            Some(h) => h.clone(),
            None => {
                self.messages.push(Message::system(
                    "Cannot initialize orchestrator: no tokio runtime",
                ));
                return false;
            }
        };

        // Block on the async initialization
        match handle.block_on(AgentOrchestrator::new()) {
            Ok(orchestrator) => {
                self.orchestrator = Some(orchestrator);
                self.messages.push(Message::system("Agent orchestrator initialized"));
                true
            }
            Err(e) => {
                let msg = format!("Agent orchestrator disabled: {}", e);
                self.messages.push(Message::system(&msg));
                false
            }
        }
    }

    /// Process user input through the agent orchestrator (if available).
    ///
    /// Returns the processed response, or the original input if no orchestrator.
    pub async fn process_with_agent(&mut self, input: &str) -> Result<String, String> {
        if let Some(ref mut orchestrator) = self.orchestrator {
            orchestrator
                .process_user_input(input)
                .await
                .map_err(|e| e.to_string())
        } else {
            // No orchestrator - return input unchanged
            Ok(input.to_string())
        }
    }

    /// Check if the orchestrator is initialized.
    pub fn has_orchestrator(&self) -> bool {
        self.orchestrator.is_some()
    }

    /// Get a reference to the orchestrator (if available).
    pub fn orchestrator(&self) -> Option<&AgentOrchestrator> {
        self.orchestrator.as_ref()
    }

    /// Get a mutable reference to the orchestrator (if available).
    pub fn orchestrator_mut(&mut self) -> Option<&mut AgentOrchestrator> {
        self.orchestrator.as_mut()
    }

    /// Get the runtime handle if available.
    pub fn runtime_handle(&self) -> Option<&Arc<TokioHandle>> {
        self.runtime_handle.as_ref()
    }
}
