//! Core daemon service implementation.
//!
//! This module provides the main daemon service that coordinates all
//! subsystems including session management, IPC server, memory monitoring,
//! and pairing code management.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, Mutex};
use tokio::time;
use tracing::{debug, info, warn};

use crate::error::{DaemonError, Result};
use crate::ipc::{IpcConfig, IpcServer, protocol::{HealthStatusResponse, MemoryStatusResponse, SystemInfo, SessionInfo}};
use crate::monitoring::MemoryUsage;
use crate::pairing::{PairingManager, PairingEntry};
use crate::sessions::SessionManager;

/// Daemon service configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// IPC configuration
    pub ipc: IpcConfig,
    /// Session cleanup interval (in seconds)
    pub cleanup_interval_secs: u64,
    /// Idle session threshold (in seconds)
    pub idle_threshold_secs: u64,
    /// Log level
    pub log_level: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            ipc: IpcConfig::default(),
            cleanup_interval_secs: 300, // 5 minutes
            idle_threshold_secs: 3600,  // 1 hour
            log_level: "info".to_string(),
        }
    }
}

/// Daemon status information.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonStatus {
    /// Whether daemon is running
    pub running: bool,
    /// Process ID if running
    pub pid: Option<u32>,
    /// Start time
    pub started_at: Option<DateTime<Utc>>,
    /// Uptime in seconds
    pub uptime_seconds: Option<u64>,
    /// Number of active sessions
    pub active_sessions: usize,
    /// Configuration
    pub config: DaemonConfig,
    /// Version information
    pub version: String,
}

/// Service handle for IPC operations (to avoid self-referencing).
pub struct DaemonServiceHandle {
    /// Session manager
    session_manager: Arc<RwLock<SessionManager>>,
    /// Pairing manager
    pairing_manager: Arc<Mutex<PairingManager>>,
    /// Service start time
    started_at: Instant,
}

impl DaemonServiceHandle {
    /// Create session via handle.
    pub async fn create_session(
        &self,
        project_path: Option<PathBuf>,
        adapter: Option<String>,
        name: Option<String>,
    ) -> Result<String> {
        let mut session_manager = self.session_manager.write().await;
        session_manager.create_session(project_path, adapter, name).await
    }

    /// List sessions via handle.
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let session_manager = self.session_manager.read().await;
        session_manager.list_sessions().await
    }

    /// Get session via handle.
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionInfo>> {
        let session_manager = self.session_manager.read().await;
        session_manager.get_session(session_id).await
    }

    /// Terminate session via handle.
    pub async fn terminate_session(&self, session_id: &str) -> Result<()> {
        let mut session_manager = self.session_manager.write().await;
        session_manager.terminate_session(session_id).await
    }

    /// Send to session via handle.
    pub async fn send_to_session(&self, session_id: &str, message: &str) -> Result<String> {
        let mut session_manager = self.session_manager.write().await;
        session_manager.send_to_session(session_id, message).await
    }

    /// Generate pairing code via handle.
    pub async fn generate_pairing_code(
        &self,
        session_id: Option<String>,
        project_path: Option<PathBuf>,
    ) -> Result<(String, DateTime<Utc>)> {
        let mut pairing_manager = self.pairing_manager.lock().await;
        let code = pairing_manager.generate_code(session_id, project_path)?;

        let entry = pairing_manager.get_entry(&code)
            .ok_or_else(|| DaemonError::Pairing("Failed to retrieve generated code".to_string()))?;

        Ok((code, entry.expires_at))
    }

    /// Validate pairing code via handle.
    pub async fn validate_pairing_code(
        &self,
        code: &str,
        client_info: Option<String>,
    ) -> Result<Option<PairingEntry>> {
        let mut pairing_manager = self.pairing_manager.lock().await;
        pairing_manager.validate_code(code, client_info)
    }

    /// Get health status via handle.
    pub async fn get_health_status(&self) -> Result<HealthStatusResponse> {
        let session_manager = self.session_manager.read().await;
        let sessions = session_manager.list_sessions().await?;
        let memory_stats = session_manager.get_memory_statistics();

        let memory_usage: std::collections::HashMap<String, MemoryUsage> = memory_stats
            .into_iter()
            .filter_map(|(id, info)| info.memory_usage.map(|usage| (id, usage)))
            .collect();

        Ok(HealthStatusResponse {
            status: "healthy".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: self.started_at.elapsed().as_secs(),
            active_sessions: sessions.len(),
            memory_usage,
            system_info: get_system_info(),
        })
    }

    /// Get memory status via handle.
    pub async fn get_memory_status(&self) -> Result<MemoryStatusResponse> {
        let session_manager = self.session_manager.read().await;
        let sessions = session_manager.get_memory_statistics();

        let total_usage_mb: u64 = sessions
            .values()
            .filter_map(|info| info.memory_usage.as_ref())
            .map(|usage| usage.rss_bytes / 1024 / 1024)
            .sum();

        let max_memory_mb = 2048u64; // 2GB default
        let usage_percentage = (total_usage_mb as f32 / max_memory_mb as f32) * 100.0;

        Ok(MemoryStatusResponse {
            total_usage_mb,
            max_memory_mb,
            usage_percentage,
            sessions,
            cleanup_triggered: usage_percentage > 90.0,
        })
    }

    /// Shutdown via handle.
    pub async fn shutdown(&self) -> Result<()> {
        // Terminate all sessions
        let session_ids: Vec<String> = {
            let session_manager = self.session_manager.read().await;
            let sessions = session_manager.list_sessions().await?;
            sessions.into_iter().map(|s| s.id).collect()
        };

        for session_id in session_ids {
            if let Err(e) = self.terminate_session(&session_id).await {
                warn!(session_id = %session_id, error = %e, "Failed to terminate session during shutdown");
            }
        }

        Ok(())
    }

    /// Restart via handle.
    pub async fn restart(&self) -> Result<()> {
        // In a real implementation, this would coordinate with the main service
        Err(DaemonError::Configuration("Restart not implemented via handle".to_string()))
    }
}

/// Main daemon service.
pub struct DaemonService {
    /// Service configuration
    config: DaemonConfig,
    /// Session manager
    session_manager: Arc<RwLock<SessionManager>>,
    /// Pairing manager
    pairing_manager: Arc<Mutex<PairingManager>>,
    /// IPC server
    ipc_server: Option<IpcServer>,
    /// Service start time
    started_at: Instant,
    /// Cleanup task handle
    cleanup_handle: Option<tokio::task::JoinHandle<()>>,
    /// Shutdown signal
    shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
}

impl DaemonService {
    /// Create a new daemon service.
    pub async fn new() -> Result<Self> {
        Self::with_config(DaemonConfig::default()).await
    }

    /// Create a new daemon service with custom configuration.
    pub async fn with_config(config: DaemonConfig) -> Result<Self> {
        info!("Initializing daemon service");

        // Ensure all directories exist
        commander_core::config::ensure_all_dirs()
            .map_err(|e| DaemonError::Configuration(format!("Failed to create directories: {}", e)))?;

        // Initialize session manager
        let session_manager = Arc::new(RwLock::new(
            SessionManager::new().await?
        ));

        // Initialize pairing manager
        let pairing_manager = Arc::new(Mutex::new(
            PairingManager::new()?
        ));

        Ok(Self {
            config,
            session_manager,
            pairing_manager,
            ipc_server: None,
            started_at: Instant::now(),
            cleanup_handle: None,
            shutdown_tx: None,
        })
    }

    /// Run the daemon service (foreground mode).
    pub async fn run(mut self) -> Result<()> {
        info!("Starting daemon service in foreground mode");

        // Start IPC server
        self.start_ipc_server().await?;

        // Start cleanup task
        self.start_cleanup_task();

        // Setup signal handling
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Handle SIGTERM/SIGINT gracefully
        #[cfg(unix)]
        {
            use signal_hook::consts::signal::*;
            use signal_hook_tokio::Signals;
            use futures::stream::StreamExt;

            let mut signals = Signals::new(&[SIGTERM, SIGINT])
                .map_err(|e| DaemonError::Configuration(format!("Failed to setup signals: {}", e)))?;

            let shutdown_tx = self.shutdown_tx.clone().unwrap();

            tokio::spawn(async move {
                while let Some(signal) = signals.next().await {
                    match signal {
                        SIGTERM | SIGINT => {
                            info!(signal = signal, "Received shutdown signal");
                            let _ = shutdown_tx.send(());
                            break;
                        }
                        _ => {}
                    }
                }
            });
        }

        info!("Daemon service started successfully");

        // Wait for shutdown signal
        let _ = shutdown_rx.recv().await;

        info!("Shutting down daemon service");
        self.shutdown().await?;

        Ok(())
    }

    /// Daemonize the service (background mode).
    pub async fn daemonize(self) -> Result<()> {
        info!("Starting daemon service in background mode");

        // For now, just run in foreground
        // In a real implementation, this would fork and detach
        // or use a proper daemonization library
        self.run().await
    }

    /// Stop the daemon service.
    pub async fn stop() -> Result<()> {
        let pid_file = daemon_pid_file();

        if !pid_file.exists() {
            return Err(DaemonError::NotRunning);
        }

        let pid_str = std::fs::read_to_string(&pid_file)
            .map_err(|e| DaemonError::StopFailed(format!("Failed to read PID file: {}", e)))?;

        let pid = pid_str.trim().parse::<u32>()
            .map_err(|e| DaemonError::StopFailed(format!("Invalid PID in file: {}", e)))?;

        // Check if process is running
        if !is_process_running(pid) {
            std::fs::remove_file(&pid_file).ok();
            return Err(DaemonError::NotRunning);
        }

        // Send SIGTERM
        #[cfg(unix)]
        {
            use std::process::Command;

            Command::new("kill")
                .arg(pid.to_string())
                .output()
                .map_err(|e| DaemonError::StopFailed(format!("Failed to send SIGTERM: {}", e)))?;
        }

        // Wait for process to exit
        for _ in 0..100 { // Wait up to 10 seconds
            if !is_process_running(pid) {
                std::fs::remove_file(&pid_file).ok();
                info!("Daemon stopped successfully");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Force kill if still running
        warn!("Daemon did not stop gracefully, force killing");
        #[cfg(unix)]
        {
            use std::process::Command;

            Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output()
                .map_err(|e| DaemonError::StopFailed(format!("Failed to send SIGKILL: {}", e)))?;
        }

        std::fs::remove_file(&pid_file).ok();
        Ok(())
    }

    /// Get daemon status.
    pub async fn status() -> Result<DaemonStatus> {
        let pid_file = daemon_pid_file();
        let config = DaemonConfig::default();

        if !pid_file.exists() {
            return Ok(DaemonStatus {
                running: false,
                pid: None,
                started_at: None,
                uptime_seconds: None,
                active_sessions: 0,
                config,
                version: env!("CARGO_PKG_VERSION").to_string(),
            });
        }

        let pid_str = std::fs::read_to_string(&pid_file)
            .map_err(|e| DaemonError::Io(e))?;

        let pid = pid_str.trim().parse::<u32>()
            .map_err(|_| DaemonError::Configuration("Invalid PID file".to_string()))?;

        let running = is_process_running(pid);

        if !running {
            std::fs::remove_file(&pid_file).ok();
        }

        // TODO: In real implementation, we could query the daemon via IPC
        // to get accurate session counts and other runtime information

        Ok(DaemonStatus {
            running,
            pid: if running { Some(pid) } else { None },
            started_at: None, // Would need to be persisted or queried
            uptime_seconds: None, // Would need to be calculated
            active_sessions: 0, // Would need to be queried via IPC
            config,
            version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }

    /// Restart the daemon service.
    pub async fn restart(&self) -> Result<()> {
        info!("Restarting daemon service");

        // Stop current instance
        Self::stop().await?;

        // Give it time to fully stop
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start new instance
        let new_service = Self::new().await?;
        new_service.daemonize().await?;

        Ok(())
    }

    /// Generate a pairing code.
    pub async fn generate_pairing_code(session_id: Option<&str>) -> Result<String> {
        let mut pairing_manager = PairingManager::new()?;
        let code = pairing_manager.generate_code(session_id.map(|s| s.to_string()), None)?;
        Ok(code)
    }

    /// Session management methods (for IPC handlers).
    pub async fn create_session(
        &self,
        project_path: Option<PathBuf>,
        adapter: Option<String>,
        name: Option<String>,
    ) -> Result<String> {
        let mut session_manager = self.session_manager.write().await;
        session_manager.create_session(project_path, adapter, name).await
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let session_manager = self.session_manager.read().await;
        session_manager.list_sessions().await
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionInfo>> {
        let session_manager = self.session_manager.read().await;
        session_manager.get_session(session_id).await
    }

    pub async fn terminate_session(&self, session_id: &str) -> Result<()> {
        let mut session_manager = self.session_manager.write().await;
        session_manager.terminate_session(session_id).await
    }

    pub async fn send_to_session(&self, session_id: &str, message: &str) -> Result<String> {
        let mut session_manager = self.session_manager.write().await;
        session_manager.send_to_session(session_id, message).await
    }

    /// Pairing management methods.
    pub async fn generate_pairing_code_with_session(
        &self,
        session_id: Option<String>,
        project_path: Option<PathBuf>,
    ) -> Result<(String, DateTime<Utc>)> {
        let mut pairing_manager = self.pairing_manager.lock().await;
        let code = pairing_manager.generate_code(session_id, project_path)?;

        // Get the entry to return expiration time
        let entry = pairing_manager.get_entry(&code)
            .ok_or_else(|| DaemonError::Pairing("Failed to retrieve generated code".to_string()))?;

        Ok((code, entry.expires_at))
    }

    pub async fn validate_pairing_code(
        &self,
        code: &str,
        client_info: Option<String>,
    ) -> Result<Option<PairingEntry>> {
        let mut pairing_manager = self.pairing_manager.lock().await;
        pairing_manager.validate_code(code, client_info)
    }

    /// Status and monitoring methods.
    pub async fn get_health_status(&self) -> Result<HealthStatusResponse> {
        let session_manager = self.session_manager.read().await;
        let sessions = session_manager.list_sessions().await?;
        let memory_stats = session_manager.get_memory_statistics();

        let memory_usage: std::collections::HashMap<String, MemoryUsage> = memory_stats
            .into_iter()
            .filter_map(|(id, info)| info.memory_usage.map(|usage| (id, usage)))
            .collect();

        Ok(HealthStatusResponse {
            status: "healthy".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: self.started_at.elapsed().as_secs(),
            active_sessions: sessions.len(),
            memory_usage,
            system_info: get_system_info(),
        })
    }

    pub async fn get_memory_status(&self) -> Result<MemoryStatusResponse> {
        let session_manager = self.session_manager.read().await;
        let sessions = session_manager.get_memory_statistics();

        let total_usage_mb: u64 = sessions
            .values()
            .filter_map(|info| info.memory_usage.as_ref())
            .map(|usage| usage.rss_bytes / 1024 / 1024)
            .sum();

        // Default max memory from config (would be configurable)
        let max_memory_mb = 2048u64; // 2GB default
        let usage_percentage = (total_usage_mb as f32 / max_memory_mb as f32) * 100.0;

        Ok(MemoryStatusResponse {
            total_usage_mb,
            max_memory_mb,
            usage_percentage,
            sessions,
            cleanup_triggered: usage_percentage > 90.0, // Simple threshold
        })
    }

    /// Shutdown the daemon service.
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down daemon service");

        // Stop IPC server
        if let Some(server) = &mut self.ipc_server {
            server.stop().await?;
        }

        // Stop cleanup task
        if let Some(handle) = self.cleanup_handle.take() {
            handle.abort();
        }

        // Terminate all sessions
        let session_ids: Vec<String> = {
            let session_manager = self.session_manager.read().await;
            let sessions = session_manager.list_sessions().await?;
            sessions.into_iter().map(|s| s.id).collect()
        };

        for session_id in session_ids {
            if let Err(e) = self.terminate_session(&session_id).await {
                warn!(session_id = %session_id, error = %e, "Failed to terminate session during shutdown");
            }
        }

        // Clean up PID file
        let pid_file = daemon_pid_file();
        std::fs::remove_file(&pid_file).ok();

        info!("Daemon service shutdown complete");
        Ok(())
    }

    /// Start the IPC server.
    async fn start_ipc_server(&mut self) -> Result<()> {
        // Create a minimal service handle for IPC
        let service_handle = DaemonServiceHandle {
            session_manager: Arc::clone(&self.session_manager),
            pairing_manager: Arc::clone(&self.pairing_manager),
            started_at: self.started_at,
        };

        let service_ref = Arc::new(RwLock::new(service_handle));
        let mut ipc_server = IpcServer::new(self.config.ipc.clone(), service_ref);
        ipc_server.start().await?;
        self.ipc_server = Some(ipc_server);

        Ok(())
    }

    /// Start the cleanup task.
    fn start_cleanup_task(&mut self) {
        let session_manager = Arc::clone(&self.session_manager);
        let pairing_manager = Arc::clone(&self.pairing_manager);
        let cleanup_interval = Duration::from_secs(self.config.cleanup_interval_secs);
        let idle_threshold = Duration::from_secs(self.config.idle_threshold_secs);

        let handle = tokio::spawn(async move {
            let mut interval = time::interval(cleanup_interval);

            loop {
                interval.tick().await;

                // Clean up idle sessions
                {
                    let mut session_mgr = session_manager.write().await;
                    if let Ok(count) = session_mgr.cleanup_idle_sessions(idle_threshold).await {
                        if count > 0 {
                            debug!(cleaned_up = count, "Cleaned up idle sessions");
                        }
                    }
                }

                // Clean up expired pairing codes
                {
                    let mut pairing_mgr = pairing_manager.lock().await;
                    if let Err(e) = pairing_mgr.cleanup_expired() {
                        warn!(error = %e, "Failed to cleanup expired pairing codes");
                    }
                }
            }
        });

        self.cleanup_handle = Some(handle);
    }
}

/// Get the daemon PID file path.
fn daemon_pid_file() -> PathBuf {
    commander_core::config::runtime_state_dir().join("daemon.pid")
}

/// Check if a process is running.
fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use std::process::Command;

        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        // Fallback for non-Unix systems
        false
    }
}

/// Get system information.
fn get_system_info() -> SystemInfo {
    SystemInfo {
        os: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        total_memory_mb: 0, // Would need system query
        available_memory_mb: 0, // Would need system query
    }
}
