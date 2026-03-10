//! Memory and process monitoring for daemon sessions.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::time;
use tracing::{debug, warn};

use crate::error::{DaemonError, Result};

/// Memory configuration for monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum memory usage in MB
    pub max_memory_mb: u64,
    /// Warning threshold (0.0 - 1.0)
    pub warning_threshold: f32,
    /// Cleanup threshold (0.0 - 1.0)
    pub cleanup_threshold: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: 1024, // 1GB default
            warning_threshold: 0.8,
            cleanup_threshold: 0.9,
        }
    }
}

/// Cleanup policy for memory management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CleanupPolicy {
    /// Compact memory stores
    CompactMemory,
    /// Terminate oldest session
    TerminateOldestSession,
    /// Purge cache files
    PurgeCache,
    /// Notify user about high memory usage
    NotifyUser,
}

/// Memory usage statistics for a process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUsage {
    /// RSS memory in bytes
    pub rss_bytes: u64,
    /// Virtual memory in bytes
    pub virtual_bytes: u64,
    /// Memory usage percentage
    pub percentage: f32,
    /// Last updated timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Session memory information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemoryInfo {
    /// Session ID
    pub session_id: String,
    /// Process ID
    pub pid: Option<u32>,
    /// Memory usage
    pub memory_usage: Option<MemoryUsage>,
    /// Memory configuration
    pub config: MemoryConfig,
    /// Last activity timestamp (serialized as ISO string)
    #[serde(with = "instant_as_iso_string")]
    pub last_activity: Instant,
}

mod instant_as_iso_string {
    use std::time::Instant;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(instant: &Instant, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // For serialization, we'll use elapsed time since start as seconds
        let elapsed = instant.elapsed().as_secs();
        serializer.serialize_u64(elapsed)
    }

    pub fn deserialize<'de, D>(_deserializer: D) -> Result<Instant, D::Error>
    where
        D: Deserializer<'de>,
    {
        // For deserialization, just return current instant
        // In real implementation, this would need proper handling
        Ok(Instant::now())
    }
}

/// Memory monitor for tracking session resource usage.
pub struct MemoryMonitor {
    /// Per-session configurations
    session_configs: HashMap<String, MemoryConfig>,
    /// Global memory limits
    global_config: MemoryConfig,
    /// Cleanup policies
    cleanup_policies: Vec<CleanupPolicy>,
    /// Session memory tracking
    session_memory: HashMap<String, SessionMemoryInfo>,
    /// Monitoring task handle
    monitor_handle: Option<tokio::task::JoinHandle<()>>,
}

impl MemoryMonitor {
    /// Create a new memory monitor with default configuration.
    pub fn new() -> Self {
        Self {
            session_configs: HashMap::new(),
            global_config: MemoryConfig::default(),
            cleanup_policies: vec![
                CleanupPolicy::NotifyUser,
                CleanupPolicy::CompactMemory,
                CleanupPolicy::TerminateOldestSession,
            ],
            session_memory: HashMap::new(),
            monitor_handle: None,
        }
    }

    /// Start monitoring with the specified interval.
    pub fn start_monitoring(&mut self, interval: Duration) {
        if self.monitor_handle.is_some() {
            warn!("Memory monitoring already started");
            return;
        }

        let mut session_memory = self.session_memory.clone();
        let global_config = self.global_config.clone();
        let cleanup_policies = self.cleanup_policies.clone();

        let handle = tokio::spawn(async move {
            let mut interval_timer = time::interval(interval);

            loop {
                interval_timer.tick().await;

                // Update memory usage for all sessions
                for (session_id, info) in session_memory.iter_mut() {
                    if let Some(pid) = info.pid {
                        match get_process_memory(pid).await {
                            Ok(memory_usage) => {
                                debug!(
                                    session_id = %session_id,
                                    rss_mb = memory_usage.rss_bytes / 1024 / 1024,
                                    percentage = memory_usage.percentage,
                                    "Updated session memory usage"
                                );
                                info.memory_usage = Some(memory_usage);
                            }
                            Err(e) => {
                                warn!(
                                    session_id = %session_id,
                                    pid = pid,
                                    error = %e,
                                    "Failed to get memory usage"
                                );
                                info.memory_usage = None;
                            }
                        }
                    }
                }

                // Check for cleanup triggers
                if let Err(e) = check_cleanup_triggers(&session_memory, &global_config, &cleanup_policies).await {
                    warn!(error = %e, "Failed to process cleanup triggers");
                }
            }
        });

        self.monitor_handle = Some(handle);
        debug!("Started memory monitoring with interval: {:?}", interval);
    }

    /// Stop monitoring.
    pub fn stop_monitoring(&mut self) {
        if let Some(handle) = self.monitor_handle.take() {
            handle.abort();
            debug!("Stopped memory monitoring");
        }
    }

    /// Register a session for monitoring.
    pub fn register_session(&mut self, session_id: String, pid: Option<u32>) -> Result<()> {
        let config = self.session_configs
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| self.global_config.clone());

        let session_info = SessionMemoryInfo {
            session_id: session_id.clone(),
            pid,
            memory_usage: None,
            config,
            last_activity: Instant::now(),
        };

        self.session_memory.insert(session_id.clone(), session_info);
        debug!(session_id = %session_id, pid = ?pid, "Registered session for monitoring");
        Ok(())
    }

    /// Unregister a session from monitoring.
    pub fn unregister_session(&mut self, session_id: &str) {
        if self.session_memory.remove(session_id).is_some() {
            debug!(session_id = %session_id, "Unregistered session from monitoring");
        }
    }

    /// Update session activity timestamp.
    pub fn update_activity(&mut self, session_id: &str) {
        if let Some(info) = self.session_memory.get_mut(session_id) {
            info.last_activity = Instant::now();
        }
    }

    /// Get memory statistics for all sessions.
    pub fn get_statistics(&self) -> HashMap<String, SessionMemoryInfo> {
        self.session_memory.clone()
    }

    /// Get memory usage for a specific session.
    pub fn get_session_memory(&self, session_id: &str) -> Option<&SessionMemoryInfo> {
        self.session_memory.get(session_id)
    }

    /// Set memory configuration for a session.
    pub fn set_session_config(&mut self, session_id: String, config: MemoryConfig) {
        self.session_configs.insert(session_id, config);
    }

    /// Set global memory configuration.
    pub fn set_global_config(&mut self, config: MemoryConfig) {
        self.global_config = config;
    }

    /// Set cleanup policies.
    pub fn set_cleanup_policies(&mut self, policies: Vec<CleanupPolicy>) {
        self.cleanup_policies = policies;
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MemoryMonitor {
    fn drop(&mut self) {
        self.stop_monitoring();
    }
}

/// Get memory usage for a process.
async fn get_process_memory(_pid: u32) -> Result<MemoryUsage> {
    tokio::task::spawn_blocking(move || {
        #[cfg(feature = "psutil")]
        {
            use psutil::process::Process;

            let process = Process::new(pid)
                .map_err(|e| DaemonError::Memory(format!("Failed to get process {}: {}", pid, e)))?;

            let memory_info = process.memory_info()
                .map_err(|e| DaemonError::Memory(format!("Failed to get memory info: {}", e)))?;

            let memory_percent = process.memory_percent()
                .map_err(|e| DaemonError::Memory(format!("Failed to get memory percent: {}", e)))?;

            Ok(MemoryUsage {
                rss_bytes: memory_info.rss(),
                virtual_bytes: memory_info.vms(),
                percentage: memory_percent,
                timestamp: chrono::Utc::now(),
            })
        }

        #[cfg(not(feature = "psutil"))]
        {
            // Fallback implementation without psutil
            warn!("Memory monitoring not available without psutil feature");
            Ok(MemoryUsage {
                rss_bytes: 0,
                virtual_bytes: 0,
                percentage: 0.0,
                timestamp: chrono::Utc::now(),
            })
        }
    }).await
    .map_err(|e| DaemonError::Memory(format!("Task execution error: {}", e)))?
}

/// Check for cleanup triggers and execute policies.
async fn check_cleanup_triggers(
    session_memory: &HashMap<String, SessionMemoryInfo>,
    global_config: &MemoryConfig,
    cleanup_policies: &[CleanupPolicy],
) -> Result<()> {
    let total_memory_mb: u64 = session_memory
        .values()
        .filter_map(|info| info.memory_usage.as_ref())
        .map(|usage| usage.rss_bytes / 1024 / 1024)
        .sum();

    let memory_ratio = total_memory_mb as f32 / global_config.max_memory_mb as f32;

    if memory_ratio > global_config.cleanup_threshold {
        warn!(
            total_memory_mb = total_memory_mb,
            threshold = global_config.cleanup_threshold,
            "Memory usage exceeded cleanup threshold, executing policies"
        );

        for policy in cleanup_policies {
            match execute_cleanup_policy(policy, session_memory).await {
                Ok(_) => debug!(policy = ?policy, "Executed cleanup policy"),
                Err(e) => warn!(policy = ?policy, error = %e, "Failed to execute cleanup policy"),
            }
        }
    } else if memory_ratio > global_config.warning_threshold {
        warn!(
            total_memory_mb = total_memory_mb,
            threshold = global_config.warning_threshold,
            "Memory usage exceeded warning threshold"
        );
    }

    Ok(())
}

/// Execute a cleanup policy.
async fn execute_cleanup_policy(
    policy: &CleanupPolicy,
    session_memory: &HashMap<String, SessionMemoryInfo>,
) -> Result<()> {
    match policy {
        CleanupPolicy::NotifyUser => {
            // Log warning - in real implementation, this could send notifications
            warn!("High memory usage detected. Consider reducing session count or memory usage.");
        }
        CleanupPolicy::CompactMemory => {
            // Trigger memory compaction for sessions
            debug!("Triggering memory compaction for sessions");
            // Implementation would call session compaction APIs
        }
        CleanupPolicy::TerminateOldestSession => {
            // Find and terminate the oldest session
            if let Some((oldest_session, _)) = session_memory
                .iter()
                .min_by_key(|(_, info)| info.last_activity)
            {
                warn!(
                    session_id = %oldest_session,
                    "Terminating oldest session due to memory pressure"
                );
                // Implementation would call session termination API
            }
        }
        CleanupPolicy::PurgeCache => {
            // Purge cache directories
            debug!("Purging cache directories");
            // Implementation would clean up cache files
        }
    }

    Ok(())
}
