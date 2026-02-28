//! Session management for the daemon.
//!
//! This module provides session lifecycle management, building on the
//! existing commander-orchestrator patterns but with daemon-centric
//! persistence and monitoring.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use commander_orchestrator::{AgentOrchestrator, OrchestratorError};

use crate::error::{DaemonError, Result};
use crate::ipc::protocol::SessionInfo;
use crate::monitoring::{MemoryMonitor, SessionMemoryInfo};

/// Session status enumeration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Session is being created
    Creating,
    /// Session is active and ready
    Active,
    /// Session is idle (no recent activity)
    Idle,
    /// Session is terminating
    Terminating,
    /// Session has terminated
    Terminated,
    /// Session encountered an error
    Error,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::Active => write!(f, "active"),
            Self::Idle => write!(f, "idle"),
            Self::Terminating => write!(f, "terminating"),
            Self::Terminated => write!(f, "terminated"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Managed session information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedSession {
    /// Unique session ID
    pub id: String,
    /// Optional human-readable name
    pub name: Option<String>,
    /// Adapter type (claude-code, mpm, etc.)
    pub adapter: String,
    /// Project path associated with session
    pub project_path: Option<PathBuf>,
    /// Current status
    pub status: SessionStatus,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
    /// Process ID if available
    pub pid: Option<u32>,
    /// Error message if status is Error
    pub error_message: Option<String>,
}

impl ManagedSession {
    /// Create a new managed session.
    fn new(
        name: Option<String>,
        adapter: String,
        project_path: Option<PathBuf>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            adapter,
            project_path,
            status: SessionStatus::Creating,
            created_at: now,
            last_activity: now,
            pid: None,
            error_message: None,
        }
    }

    /// Update activity timestamp.
    fn touch(&mut self) {
        self.last_activity = Utc::now();
    }

    /// Convert to protocol SessionInfo.
    pub fn to_session_info(&self, memory_usage: Option<crate::monitoring::MemoryUsage>) -> SessionInfo {
        SessionInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            adapter: self.adapter.clone(),
            project_path: self.project_path.clone(),
            status: self.status.to_string(),
            created_at: self.created_at,
            last_activity: self.last_activity,
            memory_usage,
        }
    }
}

/// Session manager for the daemon.
pub struct SessionManager {
    /// Active sessions
    sessions: HashMap<String, ManagedSession>,
    /// Agent orchestrator instances by session ID
    orchestrators: HashMap<String, Arc<RwLock<AgentOrchestrator>>>,
    /// Memory monitor
    memory_monitor: MemoryMonitor,
    /// Session persistence (future: could save/restore sessions)
    persistence_path: Option<PathBuf>,
}

impl SessionManager {
    /// Create a new session manager.
    pub async fn new() -> Result<Self> {
        let mut memory_monitor = MemoryMonitor::new();
        memory_monitor.start_monitoring(std::time::Duration::from_secs(30));

        Ok(Self {
            sessions: HashMap::new(),
            orchestrators: HashMap::new(),
            memory_monitor,
            persistence_path: None,
        })
    }

    /// Create a new session.
    pub async fn create_session(
        &mut self,
        project_path: Option<PathBuf>,
        adapter: Option<String>,
        name: Option<String>,
    ) -> Result<String> {
        let adapter = adapter.unwrap_or_else(|| "generic".to_string());

        // Create session record
        let mut session = ManagedSession::new(name, adapter.clone(), project_path);
        let session_id = session.id.clone();

        info!(
            session_id = %session_id,
            adapter = %adapter,
            project_path = ?session.project_path,
            "Creating new session"
        );

        // Create agent orchestrator
        let orchestrator = match AgentOrchestrator::new().await {
            Ok(orch) => Arc::new(RwLock::new(orch)),
            Err(OrchestratorError::Configuration(msg)) => {
                session.status = SessionStatus::Error;
                session.error_message = Some(msg.clone());
                self.sessions.insert(session_id.clone(), session);
                return Err(DaemonError::Configuration(msg));
            }
            Err(e) => {
                let error_msg = e.to_string();
                session.status = SessionStatus::Error;
                session.error_message = Some(error_msg.clone());
                self.sessions.insert(session_id.clone(), session);
                return Err(DaemonError::Orchestrator(e));
            }
        };

        // Update session status
        session.status = SessionStatus::Active;
        session.touch();

        // Store session and orchestrator
        self.sessions.insert(session_id.clone(), session);
        self.orchestrators.insert(session_id.clone(), orchestrator);

        // Register with memory monitor
        self.memory_monitor.register_session(session_id.clone(), None)?;

        info!(session_id = %session_id, "Session created successfully");
        Ok(session_id)
    }

    /// List all sessions.
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let memory_stats = self.memory_monitor.get_statistics();

        let mut sessions: Vec<SessionInfo> = self.sessions
            .values()
            .map(|session| {
                let memory_usage = memory_stats
                    .get(&session.id)
                    .and_then(|stats| stats.memory_usage.clone());

                session.to_session_info(memory_usage)
            })
            .collect();

        // Sort by creation time (newest first)
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(sessions)
    }

    /// Get a specific session.
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionInfo>> {
        if let Some(session) = self.sessions.get(session_id) {
            let memory_usage = self.memory_monitor
                .get_session_memory(session_id)
                .and_then(|stats| stats.memory_usage.clone());

            Ok(Some(session.to_session_info(memory_usage)))
        } else {
            Ok(None)
        }
    }

    /// Terminate a session.
    pub async fn terminate_session(&mut self, session_id: &str) -> Result<()> {
        if let Some(session) = self.sessions.get_mut(session_id) {
            info!(session_id = %session_id, "Terminating session");

            session.status = SessionStatus::Terminating;
            session.touch();

            // Remove from orchestrators (this will drop the orchestrator)
            if let Some(_orchestrator) = self.orchestrators.remove(session_id) {
                debug!(session_id = %session_id, "Dropped orchestrator for session");
            }

            // Unregister from memory monitor
            self.memory_monitor.unregister_session(session_id);

            // Update session status
            session.status = SessionStatus::Terminated;
            session.touch();

            info!(session_id = %session_id, "Session terminated");
            Ok(())
        } else {
            Err(DaemonError::SessionNotFound(session_id.to_string()))
        }
    }

    /// Send a message to a session.
    pub async fn send_to_session(&mut self, session_id: &str, message: &str) -> Result<String> {
        // Update activity
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.touch();
            self.memory_monitor.update_activity(session_id);
        }

        // Get orchestrator
        let orchestrator = self.orchestrators
            .get(session_id)
            .ok_or_else(|| DaemonError::SessionNotFound(session_id.to_string()))?;

        // Process message through orchestrator
        let mut orch = orchestrator.write().await;
        let response = orch
            .process_user_input(message)
            .await
            .map_err(DaemonError::Orchestrator)?;

        Ok(response)
    }

    /// Get memory statistics for all sessions.
    pub fn get_memory_statistics(&self) -> HashMap<String, SessionMemoryInfo> {
        self.memory_monitor.get_statistics()
    }

    /// Update session activity (called from IPC handlers).
    pub fn update_session_activity(&mut self, session_id: &str) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.touch();
            self.memory_monitor.update_activity(session_id);
        }
    }

    /// Clean up idle sessions.
    pub async fn cleanup_idle_sessions(&mut self, idle_threshold: std::time::Duration) -> Result<usize> {
        let now = Utc::now();
        let threshold = chrono::Duration::from_std(idle_threshold)
            .map_err(|e| DaemonError::Configuration(format!("Invalid duration: {}", e)))?;

        let mut to_terminate = Vec::new();

        for session in self.sessions.values() {
            if session.status == SessionStatus::Active {
                let idle_time = now - session.last_activity;
                if idle_time > threshold {
                    warn!(
                        session_id = %session.id,
                        idle_minutes = idle_time.num_minutes(),
                        "Session is idle, marking for termination"
                    );
                    to_terminate.push(session.id.clone());
                }
            }
        }

        let count = to_terminate.len();
        for session_id in to_terminate {
            if let Err(e) = self.terminate_session(&session_id).await {
                warn!(session_id = %session_id, error = %e, "Failed to terminate idle session");
            }
        }

        if count > 0 {
            info!(terminated_count = count, "Cleaned up idle sessions");
        }

        Ok(count)
    }

    /// Get session count by status.
    pub fn get_session_counts(&self) -> HashMap<String, usize> {
        let mut counts = HashMap::new();

        for session in self.sessions.values() {
            let status_str = session.status.to_string();
            *counts.entry(status_str).or_insert(0) += 1;
        }

        counts
    }

    /// Check if session exists and is active.
    pub fn is_session_active(&self, session_id: &str) -> bool {
        self.sessions
            .get(session_id)
            .map(|session| matches!(session.status, SessionStatus::Active))
            .unwrap_or(false)
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        // Best effort cleanup
        for session_id in self.sessions.keys() {
            self.memory_monitor.unregister_session(session_id);
        }
    }
}
