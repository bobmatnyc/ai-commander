//! Agent orchestrator for coordinating the multi-agent system.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tracing::{debug, info};

use commander_agent::{
    template::AdapterType, AutoEval, FeedbackSummary, OutputAnalysis, SessionAgent,
    UserAgent,
};
use commander_memory::{LocalStore, MemoryStore};

use crate::error::{OrchestratorError, Result};

/// Agent orchestrator that coordinates the User Agent and Session Agents.
///
/// This provides a simple API for UI layers to interact with the multi-agent system.
pub struct AgentOrchestrator {
    /// User Agent for processing user input.
    user_agent: UserAgent,

    /// Session Agents indexed by session ID.
    session_agents: HashMap<String, SessionAgent>,

    /// Shared memory store.
    memory_store: Arc<dyn MemoryStore>,

    /// Auto-eval for feedback tracking.
    auto_eval: AutoEval,
}

impl AgentOrchestrator {
    /// Create a new orchestrator with default configuration.
    ///
    /// Uses the default Commander data directory for memory and feedback storage.
    pub async fn new() -> Result<Self> {
        // Ensure directories exist
        commander_core::config::ensure_all_dirs()
            .map_err(|e| OrchestratorError::Configuration(e.to_string()))?;

        let data_dir = commander_core::config::state_dir();
        Self::with_data_dir(data_dir).await
    }

    /// Create a new orchestrator with a custom data directory.
    pub async fn with_data_dir(data_dir: PathBuf) -> Result<Self> {
        info!(data_dir = %data_dir.display(), "Initializing AgentOrchestrator");

        // Create memory store
        let memory_path = data_dir.join("memory");
        let memory_store: Arc<dyn MemoryStore> =
            Arc::new(LocalStore::new(memory_path).await.map_err(OrchestratorError::Memory)?);

        // Create user agent
        let user_agent = UserAgent::new(Arc::clone(&memory_store))
            .map_err(OrchestratorError::Agent)?;

        // Create auto-eval
        let feedback_path = data_dir.join("feedback");
        let auto_eval =
            AutoEval::new(feedback_path).map_err(OrchestratorError::Agent)?;

        Ok(Self {
            user_agent,
            session_agents: HashMap::new(),
            memory_store,
            auto_eval,
        })
    }

    /// Process user input through the User Agent.
    ///
    /// Returns the agent's response text.
    pub async fn process_user_input(&mut self, input: &str) -> Result<String> {
        debug!(input_len = input.len(), "Processing user input");

        let context = self.user_agent.context().clone();
        let response = self
            .user_agent
            .process(input, &context)
            .await
            .map_err(OrchestratorError::Agent)?;

        // Track feedback
        let _ = self
            .auto_eval
            .process_turn(
                self.user_agent.id(),
                input,
                &response.content,
                None,
                None,
            )
            .await;

        Ok(response.content)
    }

    /// Get or create a session agent for the given session.
    ///
    /// # Arguments
    /// - `session_id`: Unique identifier for the session (e.g., tmux session name)
    /// - `adapter_type`: Type of adapter (e.g., "claude_code", "mpm", "generic")
    pub fn get_session_agent(
        &mut self,
        session_id: &str,
        adapter_type: &str,
    ) -> Result<&mut SessionAgent> {
        if !self.session_agents.contains_key(session_id) {
            let adapter = adapter_type
                .parse::<AdapterType>()
                .unwrap_or(AdapterType::Generic);

            info!(
                session_id = %session_id,
                adapter_type = %adapter_type,
                "Creating new session agent"
            );

            let agent = SessionAgent::new(session_id, adapter, Arc::clone(&self.memory_store))
                .map_err(OrchestratorError::Agent)?;

            self.session_agents.insert(session_id.to_string(), agent);
        }

        self.session_agents
            .get_mut(session_id)
            .ok_or_else(|| OrchestratorError::SessionNotFound(session_id.to_string()))
    }

    /// Process output from a session through its Session Agent.
    ///
    /// Returns an analysis of the output including completion status,
    /// error detection, and file changes.
    pub async fn process_session_output(
        &mut self,
        session_id: &str,
        adapter_type: &str,
        output: &str,
    ) -> Result<OutputAnalysis> {
        debug!(
            session_id = %session_id,
            output_len = output.len(),
            "Processing session output"
        );

        let agent = self.get_session_agent(session_id, adapter_type)?;
        let analysis = agent
            .analyze_output(output)
            .await
            .map_err(OrchestratorError::Agent)?;

        Ok(analysis)
    }

    /// Get reference to the User Agent.
    pub fn user_agent(&self) -> &UserAgent {
        &self.user_agent
    }

    /// Get mutable reference to the User Agent.
    pub fn user_agent_mut(&mut self) -> &mut UserAgent {
        &mut self.user_agent
    }

    /// Get feedback summary for the User Agent.
    pub fn feedback_summary(&self) -> FeedbackSummary {
        self.auto_eval.summary(self.user_agent.id())
    }

    /// Get feedback summary for a specific agent.
    pub fn feedback_summary_for(&self, agent_id: &str) -> FeedbackSummary {
        self.auto_eval.summary(agent_id)
    }

    /// List all active session IDs.
    pub fn session_ids(&self) -> Vec<&str> {
        self.session_agents.keys().map(|s| s.as_str()).collect()
    }

    /// Remove a session agent.
    pub fn remove_session(&mut self, session_id: &str) -> Option<SessionAgent> {
        self.session_agents.remove(session_id)
    }

    /// Get the memory store.
    pub fn memory_store(&self) -> &Arc<dyn MemoryStore> {
        &self.memory_store
    }
}

// Implement traits that might be needed for the User Agent
use commander_agent::Agent;

impl AgentOrchestrator {
    /// Get the User Agent's ID.
    pub fn user_agent_id(&self) -> &str {
        self.user_agent.id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let result = AgentOrchestrator::with_data_dir(temp_dir.path().to_path_buf()).await;

        // May fail without API keys, but should not panic
        match result {
            Ok(orchestrator) => {
                assert_eq!(orchestrator.user_agent_id(), "user-agent");
                assert!(orchestrator.session_ids().is_empty());
            }
            Err(OrchestratorError::Agent(_)) => {
                // Expected if no API key
            }
            Err(e) => {
                panic!("Unexpected error: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_session_agent_creation() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        if let Ok(mut orchestrator) =
            AgentOrchestrator::with_data_dir(temp_dir.path().to_path_buf()).await
        {
            // Create session agent
            let result = orchestrator.get_session_agent("test-session", "generic");

            match result {
                Ok(agent) => {
                    assert_eq!(agent.session_id(), "test-session");
                    assert!(orchestrator.session_ids().contains(&"test-session"));
                }
                Err(OrchestratorError::Agent(_)) => {
                    // Expected if no API key
                }
                Err(e) => {
                    panic!("Unexpected error: {}", e);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_feedback_summary() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        if let Ok(orchestrator) =
            AgentOrchestrator::with_data_dir(temp_dir.path().to_path_buf()).await
        {
            let summary = orchestrator.feedback_summary();
            // New orchestrator should have no feedback
            assert_eq!(summary.total, 0);
        }
    }

    #[tokio::test]
    async fn test_remove_session() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        if let Ok(mut orchestrator) =
            AgentOrchestrator::with_data_dir(temp_dir.path().to_path_buf()).await
        {
            // Create and remove session
            if orchestrator
                .get_session_agent("test-session", "generic")
                .is_ok()
            {
                assert!(orchestrator.session_ids().contains(&"test-session"));

                let removed = orchestrator.remove_session("test-session");
                assert!(removed.is_some());
                assert!(!orchestrator.session_ids().contains(&"test-session"));
            }
        }
    }
}
