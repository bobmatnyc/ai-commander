//! Runtime configuration.

use std::time::Duration;

/// Configuration for the runtime.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// How often to poll for output changes.
    pub poll_interval: Duration,
    /// How long before marking an instance as idle.
    pub idle_timeout: Duration,
    /// Maximum concurrent instances allowed.
    pub max_instances: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(500),
            idle_timeout: Duration::from_secs(30),
            max_instances: 10,
        }
    }
}

impl RuntimeConfig {
    /// Creates a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the poll interval.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Sets the idle timeout.
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Sets the maximum number of instances.
    pub fn with_max_instances(mut self, max: usize) -> Self {
        self.max_instances = max;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RuntimeConfig::default();

        assert_eq!(config.poll_interval, Duration::from_millis(500));
        assert_eq!(config.idle_timeout, Duration::from_secs(30));
        assert_eq!(config.max_instances, 10);
    }

    #[test]
    fn test_config_builder() {
        let config = RuntimeConfig::new()
            .with_poll_interval(Duration::from_millis(100))
            .with_idle_timeout(Duration::from_secs(60))
            .with_max_instances(5);

        assert_eq!(config.poll_interval, Duration::from_millis(100));
        assert_eq!(config.idle_timeout, Duration::from_secs(60));
        assert_eq!(config.max_instances, 5);
    }
}
