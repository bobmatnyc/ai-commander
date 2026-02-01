//! Ngrok tunnel management for exposing the webhook endpoint.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::error::{Result, TelegramError};

/// Manages an ngrok tunnel subprocess.
pub struct NgrokTunnel {
    /// The ngrok child process.
    process: Child,
    /// The public URL of the tunnel.
    public_url: String,
    /// The local port being tunneled.
    local_port: u16,
}

impl NgrokTunnel {
    /// Start a new ngrok tunnel to the specified local port.
    ///
    /// Requires `NGROK_AUTHTOKEN` environment variable to be set.
    pub async fn start(port: u16) -> Result<Self> {
        // Check if ngrok is available
        let ngrok_path = Self::find_ngrok()?;
        debug!(path = %ngrok_path, "ngrok found");

        // Check for auth token
        if std::env::var("NGROK_AUTHTOKEN").is_err() {
            return Err(TelegramError::NgrokNoAuthToken);
        }

        // Start ngrok process
        info!(port = port, "Starting ngrok tunnel");
        let process = Command::new(&ngrok_path)
            .args(["http", &port.to_string(), "--log", "stdout"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TelegramError::NgrokError(format!("Failed to start ngrok: {}", e)))?;

        // Give ngrok time to start up
        sleep(Duration::from_secs(2)).await;

        // Get the public URL from ngrok's API
        let public_url = Self::fetch_tunnel_url().await?;
        info!(url = %public_url, "ngrok tunnel established");

        Ok(Self {
            process,
            public_url,
            local_port: port,
        })
    }

    /// Find ngrok binary in PATH.
    fn find_ngrok() -> Result<String> {
        let output = Command::new("which")
            .arg("ngrok")
            .output()
            .map_err(|_| TelegramError::NgrokNotFound)?;

        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                return Err(TelegramError::NgrokNotFound);
            }
            Ok(path)
        } else {
            Err(TelegramError::NgrokNotFound)
        }
    }

    /// Fetch the tunnel URL from ngrok's local API.
    async fn fetch_tunnel_url() -> Result<String> {
        let client = reqwest::Client::new();

        // ngrok exposes a local API on port 4040
        let api_url = "http://127.0.0.1:4040/api/tunnels";

        // Retry a few times as ngrok may still be starting
        for attempt in 1..=5 {
            debug!(attempt = attempt, "Fetching ngrok tunnel URL");

            match client.get(api_url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let json: serde_json::Value = response.json().await.map_err(|e| {
                            TelegramError::NgrokError(format!("Failed to parse ngrok API: {}", e))
                        })?;

                        // Extract the public URL from the tunnels array
                        if let Some(tunnels) = json["tunnels"].as_array() {
                            for tunnel in tunnels {
                                // Prefer https tunnel
                                if let Some(url) = tunnel["public_url"].as_str() {
                                    if url.starts_with("https://") {
                                        return Ok(url.to_string());
                                    }
                                }
                            }
                            // Fall back to any tunnel
                            if let Some(tunnel) = tunnels.first() {
                                if let Some(url) = tunnel["public_url"].as_str() {
                                    return Ok(url.to_string());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    debug!(error = %e, "ngrok API not ready yet");
                }
            }

            sleep(Duration::from_secs(1)).await;
        }

        Err(TelegramError::NgrokError(
            "Failed to get tunnel URL from ngrok API".to_string(),
        ))
    }

    /// Get the public URL of the tunnel.
    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    /// Get the local port being tunneled.
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// Check if the tunnel is still healthy.
    pub async fn health_check(&self) -> bool {
        let client = reqwest::Client::new();
        let api_url = "http://127.0.0.1:4040/api/tunnels";

        match client.get(api_url).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    /// Restart the tunnel if it's unhealthy.
    pub async fn ensure_healthy(&mut self) -> Result<()> {
        if !self.health_check().await {
            warn!("ngrok tunnel unhealthy, restarting...");
            self.restart().await?;
        }
        Ok(())
    }

    /// Restart the ngrok tunnel.
    async fn restart(&mut self) -> Result<()> {
        // Kill the old process
        let _ = self.process.kill();
        let _ = self.process.wait();

        // Start a new ngrok process
        let ngrok_path = Self::find_ngrok()?;
        let process = Command::new(&ngrok_path)
            .args(["http", &self.local_port.to_string(), "--log", "stdout"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TelegramError::NgrokError(format!("Failed to restart ngrok: {}", e)))?;

        // Give ngrok time to start up
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Get the new public URL
        let public_url = Self::fetch_tunnel_url().await?;

        self.process = process;
        self.public_url = public_url;

        Ok(())
    }

    /// Stop the ngrok tunnel.
    pub fn stop(&mut self) -> Result<()> {
        info!("Stopping ngrok tunnel");
        self.process
            .kill()
            .map_err(|e| TelegramError::NgrokError(format!("Failed to stop ngrok: {}", e)))?;
        let _ = self.process.wait();
        Ok(())
    }
}

impl Drop for NgrokTunnel {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            error!(error = %e, "Failed to stop ngrok on drop");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_ngrok() {
        // This test just ensures the function doesn't panic
        let result = NgrokTunnel::find_ngrok();
        // Either ngrok is found or not - both are valid
        assert!(result.is_ok() || matches!(result, Err(TelegramError::NgrokNotFound)));
    }
}
