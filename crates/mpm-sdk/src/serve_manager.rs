//! Lifecycle manager for the `claude-mpm serve` daemon process.

use std::path::PathBuf;
use std::time::Duration;

use crate::serve_client::UiServiceClient;
use crate::types::MpmError;

/// Manages starting, stopping and querying the `claude-mpm serve` daemon.
pub struct ServeManager {
    binary: String,
    port: u16,
    host: String,
}

impl ServeManager {
    /// Create a manager for the given binary path and port.
    pub fn new(binary: impl Into<String>, port: u16) -> Self {
        Self {
            binary: binary.into(),
            port,
            host: "127.0.0.1".to_string(),
        }
    }

    /// Discover the `claude-mpm` binary on PATH and create a manager for the given port.
    pub fn discover(port: u16) -> Result<Self, MpmError> {
        let binary = which::which("claude-mpm")
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|_| MpmError::BinaryNotFound("claude-mpm not found on PATH".to_string()))?;
        Ok(Self::new(binary, port))
    }

    /// Start the serve daemon in the background.
    /// Runs: `claude-mpm serve start --port <port>`
    pub async fn start(&self) -> Result<(), MpmError> {
        let status = tokio::process::Command::new(&self.binary)
            .args(["serve", "start", "--port", &self.port.to_string()])
            .status()
            .await?;

        if !status.success() {
            return Err(MpmError::AgentError(format!(
                "claude-mpm serve start exited with {}",
                status
            )));
        }
        Ok(())
    }

    /// Stop the serve daemon.
    /// Runs: `claude-mpm serve stop --port <port>`
    pub async fn stop(&self) -> Result<(), MpmError> {
        let status = tokio::process::Command::new(&self.binary)
            .args(["serve", "stop", "--port", &self.port.to_string()])
            .status()
            .await?;

        if !status.success() {
            return Err(MpmError::AgentError(format!(
                "claude-mpm serve stop exited with {}",
                status
            )));
        }
        Ok(())
    }

    /// Check whether the daemon is running.
    /// Polls `GET /api/v1/health`; returns true if healthy.
    pub async fn status(&self) -> Result<bool, MpmError> {
        Ok(self.client().health().await.unwrap_or(false))
    }

    /// Poll `GET /api/v1/health` until it returns 200 or the timeout expires.
    pub async fn wait_ready(&self, timeout_secs: u64) -> Result<(), MpmError> {
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(timeout_secs);
        let client = self.client();

        loop {
            if client.health().await.unwrap_or(false) {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(MpmError::Timeout(timeout_secs));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Start the daemon and wait until it is ready, then return a connected client.
    pub async fn start_and_wait(&self, timeout_secs: u64) -> Result<UiServiceClient, MpmError> {
        self.start().await?;
        self.wait_ready(timeout_secs).await?;
        Ok(self.client())
    }

    /// Return a `UiServiceClient` connected to this daemon's host:port.
    pub fn client(&self) -> UiServiceClient {
        UiServiceClient::with_host(&self.host, self.port)
    }

    /// Path to the PID file for this port: `~/.claude-mpm/serve-{port}.pid`.
    fn pid_path(&self) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".claude-mpm")
            .join(format!("serve-{}.pid", self.port))
    }
}
