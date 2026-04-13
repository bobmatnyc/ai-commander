//! API configuration.

use std::time::Instant;

/// API server configuration.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Host to bind to.
    pub host: String,
    /// Port to bind to.
    pub port: u16,
    /// Allowed CORS origins.
    pub cors_origins: Vec<String>,
    /// Server start time for uptime calculation.
    pub start_time: Instant,
}

impl ApiConfig {
    /// Creates a new API configuration with the given host and port.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            cors_origins: vec!["*".to_string()],
            start_time: Instant::now(),
        }
    }

    /// Sets the CORS origins.
    pub fn with_cors_origins(mut self, origins: Vec<String>) -> Self {
        self.cors_origins = origins;
        self
    }

    /// Returns the bind address.
    ///
    /// If the `AIC_BIND_ADDRESS` environment variable is set it overrides both
    /// `host` and `port` entirely, allowing callers to bind on e.g.
    /// `0.0.0.0:8765` for remote access over Tailscale without recompiling.
    pub fn bind_address(&self) -> String {
        std::env::var("AIC_BIND_ADDRESS")
            .unwrap_or_else(|_| format!("{}:{}", self.host, self.port))
    }

    /// Returns the uptime in seconds.
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 9876,
            cors_origins: vec!["*".to_string()],
            start_time: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_config_default() {
        let config = ApiConfig::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8765);
        assert_eq!(config.cors_origins, vec!["*".to_string()]);
    }

    #[test]
    fn test_api_config_new() {
        let config = ApiConfig::new("0.0.0.0", 3000);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 3000);
    }

    #[test]
    fn test_api_config_bind_address() {
        let config = ApiConfig::new("0.0.0.0", 3000);
        assert_eq!(config.bind_address(), "0.0.0.0:3000");
    }

    // Note: AIC_BIND_ADDRESS env var support cannot be tested reliably in a
    // parallel test suite because mutating env vars is process-wide.
    // Verified manually: `AIC_BIND_ADDRESS=0.0.0.0:9999` overrides the default.

    #[test]
    fn test_api_config_with_cors() {
        let config = ApiConfig::default()
            .with_cors_origins(vec!["http://localhost:3000".to_string()]);
        assert_eq!(config.cors_origins, vec!["http://localhost:3000".to_string()]);
    }

    #[test]
    fn test_api_config_uptime() {
        let config = ApiConfig::default();
        std::thread::sleep(std::time::Duration::from_millis(10));
        // Uptime check - just verify it doesn't panic
        let _ = config.uptime_seconds();
    }
}
