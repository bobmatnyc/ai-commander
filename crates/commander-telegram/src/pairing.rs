//! Shared pairing file for CLI-Telegram communication.
//!
//! Pairings are stored in `~/.commander/pairings.json` so that:
//! - The CLI can generate pairing codes and write them
//! - The Telegram bot can read and consume them

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Pairing code expiry time in seconds (5 minutes).
const PAIRING_EXPIRY_SECS: u64 = 300;

/// Character set for pairing codes (no ambiguous characters: I, O, 0, 1).
const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // pragma: allowlist secret

/// A pending pairing stored in the shared file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePairing {
    pub project_name: String,
    pub session_name: String,
    pub created_at: u64, // Unix timestamp
}

impl FilePairing {
    /// Check if this pairing has expired.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.created_at) > PAIRING_EXPIRY_SECS
    }
}

/// Get the path to the pairings file.
fn pairings_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".commander").join("pairings.json"))
}

/// Generate a random 6-character pairing code.
pub fn generate_code() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Simple random generation using system time and process id
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let pid = std::process::id() as u64;

    let mut state = seed.wrapping_mul(pid).wrapping_add(0x9E3779B97F4A7C15);

    (0..6)
        .map(|_| {
            // Simple xorshift-like PRNG
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let idx = (state.wrapping_mul(0x2545F4914F6CDD1D) as usize) % CHARSET.len();
            CHARSET[idx] as char
        })
        .collect()
}

/// Load all pairings from the shared file.
pub fn load_pairings() -> HashMap<String, FilePairing> {
    let Some(path) = pairings_file_path() else {
        warn!("Could not determine pairings file path");
        return HashMap::new();
    };

    if !path.exists() {
        return HashMap::new();
    }

    match fs::read_to_string(&path) {
        Ok(content) => {
            serde_json::from_str(&content).unwrap_or_else(|e| {
                warn!(error = %e, "Failed to parse pairings file");
                HashMap::new()
            })
        }
        Err(e) => {
            warn!(error = %e, "Failed to read pairings file");
            HashMap::new()
        }
    }
}

/// Save all pairings to the shared file.
fn save_pairings(pairings: &HashMap<String, FilePairing>) -> Result<(), std::io::Error> {
    let Some(path) = pairings_file_path() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine pairings file path",
        ));
    };

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(pairings)?;
    fs::write(&path, content)?;
    debug!(path = %path.display(), "Saved pairings file");
    Ok(())
}

/// Create a new pairing and save it to the shared file.
/// Returns the generated pairing code.
pub fn create_pairing(project_name: &str, session_name: &str) -> Result<String, std::io::Error> {
    let code = generate_code();

    let pairing = FilePairing {
        project_name: project_name.to_string(),
        session_name: session_name.to_string(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };

    let mut pairings = load_pairings();

    // Clean up expired pairings first
    pairings.retain(|_, p| !p.is_expired());

    pairings.insert(code.clone(), pairing);
    save_pairings(&pairings)?;

    debug!(code = %code, project = %project_name, "Created pairing");
    Ok(code)
}

/// Validate and consume a pairing code.
/// Returns (project_name, session_name) on success.
pub fn consume_pairing(code: &str) -> Option<(String, String)> {
    let code = code.to_uppercase();
    let mut pairings = load_pairings();

    // Clean up expired pairings
    pairings.retain(|_, p| !p.is_expired());

    // Try to remove the pairing
    let pairing = pairings.remove(&code)?;

    // Save updated pairings (without the consumed one)
    if let Err(e) = save_pairings(&pairings) {
        warn!(error = %e, "Failed to save pairings after consumption");
    }

    if pairing.is_expired() {
        return None;
    }

    debug!(code = %code, project = %pairing.project_name, "Consumed pairing");
    Some((pairing.project_name, pairing.session_name))
}

/// Check if a pairing code exists and is not expired.
pub fn pairing_exists(code: &str) -> bool {
    let code = code.to_uppercase();
    let pairings = load_pairings();
    pairings.get(&code).map(|p| !p.is_expired()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_code() {
        let code1 = generate_code();
        let code2 = generate_code();

        // Codes should be 6 characters
        assert_eq!(code1.len(), 6);
        assert_eq!(code2.len(), 6);

        // All characters should be from charset
        for c in code1.chars() {
            assert!(CHARSET.contains(&(c as u8)));
        }
    }

    #[test]
    fn test_file_pairing_expiry() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Fresh pairing
        let fresh = FilePairing {
            project_name: "test".to_string(),
            session_name: "commander-test".to_string(),
            created_at: now,
        };
        assert!(!fresh.is_expired());

        // Expired pairing (6 minutes ago)
        let expired = FilePairing {
            project_name: "test".to_string(),
            session_name: "commander-test".to_string(),
            created_at: now - 360,
        };
        assert!(expired.is_expired());
    }
}
