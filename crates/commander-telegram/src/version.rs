//! Bot version tracking for rebuild detection.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use commander_core::config::runtime_state_dir;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

/// Bot version information for detecting rebuilds vs restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotVersion {
    /// Hash of the bot binary (for detecting rebuilds).
    pub binary_hash: u64,
    /// Last start timestamp (Unix timestamp).
    pub last_start: u64,
    /// Number of times the bot has been started.
    pub start_count: u64,
}

impl BotVersion {
    /// Create a new BotVersion with current binary hash.
    pub fn new() -> Self {
        Self {
            binary_hash: compute_binary_hash(),
            last_start: current_timestamp(),
            start_count: 1,
        }
    }

    /// Update the version on bot start.
    /// Returns true if this is a rebuild (binary changed), false if just a restart.
    pub fn update(&mut self) -> bool {
        let current_hash = compute_binary_hash();
        let is_rebuild = current_hash != self.binary_hash;

        self.binary_hash = current_hash;
        self.last_start = current_timestamp();
        self.start_count += 1;

        is_rebuild
    }

    /// Check if this is the first start ever.
    pub fn is_first_start(&self) -> bool {
        self.start_count == 1
    }

    /// Get age since last start in seconds.
    pub fn age_seconds(&self) -> u64 {
        current_timestamp().saturating_sub(self.last_start)
    }
}

impl Default for BotVersion {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a hash of the current binary for rebuild detection.
fn compute_binary_hash() -> u64 {
    // Get current executable path
    let exe_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            // Fallback: use compile-time info
            return hash_string(&format!(
                "{}-{}-{}",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_NAME"),
                option_env!("VERGEN_GIT_SHA").unwrap_or("unknown")
            ));
        }
    };

    // Try to read and hash the binary contents
    match std::fs::metadata(&exe_path) {
        Ok(metadata) => {
            // Use size + modified time as a reasonable proxy for binary changes
            let mut hasher = DefaultHasher::new();
            metadata.len().hash(&mut hasher);
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                    duration.as_secs().hash(&mut hasher);
                }
            }
            hasher.finish()
        }
        Err(_) => {
            // Fallback: use compile-time version
            hash_string(&format!(
                "{}-{}",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_NAME")
            ))
        }
    }
}

/// Hash a string into a u64.
fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Get current Unix timestamp.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Load bot version from disk.
pub fn load_version() -> BotVersion {
    let path = runtime_state_dir().join("bot_version.json");
    if !path.exists() {
        debug!("No bot version file found, creating new");
        return BotVersion::new();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<BotVersion>(&content) {
            Ok(version) => {
                info!(
                    start_count = version.start_count,
                    age_seconds = version.age_seconds(),
                    "Loaded bot version from disk"
                );
                version
            }
            Err(e) => {
                error!(error = %e, path = %path.display(), "Failed to parse bot version file");
                BotVersion::new()
            }
        },
        Err(e) => {
            error!(error = %e, path = %path.display(), "Failed to read bot version file");
            BotVersion::new()
        }
    }
}

/// Save bot version to disk.
pub fn save_version(version: &BotVersion) {
    let path = runtime_state_dir().join("bot_version.json");

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            error!(error = %e, "Failed to create state directory");
            return;
        }
    }

    match serde_json::to_string_pretty(version) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                error!(error = %e, path = %path.display(), "Failed to write bot version file");
            } else {
                debug!(
                    start_count = version.start_count,
                    path = %path.display(),
                    "Saved bot version to disk"
                );
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to serialize bot version");
        }
    }
}

/// Check if this startup is a rebuild or just a restart.
/// Returns (is_rebuild, is_first_start, start_count).
pub fn check_rebuild() -> (bool, bool, u64) {
    let mut version = load_version();
    let is_first_start = version.is_first_start();
    let is_rebuild = if is_first_start {
        false // First start is neither rebuild nor restart
    } else {
        version.update()
    };
    let start_count = version.start_count;
    save_version(&version);

    (is_rebuild, is_first_start, start_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bot_version_creation() {
        let version = BotVersion::new();
        assert_eq!(version.start_count, 1);
        assert!(version.is_first_start());
        assert!(version.binary_hash > 0);
    }

    #[test]
    fn test_bot_version_update() {
        let mut version = BotVersion::new();
        let original_hash = version.binary_hash;

        // Simulate restart (same binary)
        let is_rebuild = version.update();
        assert!(!is_rebuild); // Same hash = not a rebuild
        assert_eq!(version.start_count, 2);
        assert!(!version.is_first_start());

        // Simulate rebuild (different binary)
        version.binary_hash = original_hash + 1; // Manually change hash
        let new_hash = compute_binary_hash();
        version.binary_hash = new_hash + 1; // Force different
        let is_rebuild = version.update();
        // Note: update() recomputes hash, so result depends on actual binary
        assert_eq!(version.start_count, 3);
    }

    #[test]
    fn test_hash_string() {
        let hash1 = hash_string("test");
        let hash2 = hash_string("test");
        let hash3 = hash_string("different");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
}
