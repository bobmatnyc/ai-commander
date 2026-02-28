//! Pairing code management for client connections.
//!
//! This module handles generating and validating pairing codes that allow
//! clients (TUI, GUI, Telegram bot) to connect to specific sessions.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{DaemonError, Result};

/// Duration for pairing code validity (5 minutes).
const PAIRING_CODE_DURATION: chrono::Duration = chrono::Duration::minutes(5);

/// A pairing code entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingEntry {
    /// Unique pairing code
    pub code: String,
    /// Session ID to pair with
    pub session_id: Option<String>,
    /// Project path associated with the pairing
    pub project_path: Option<PathBuf>,
    /// When the code was created
    pub created_at: DateTime<Utc>,
    /// When the code expires
    pub expires_at: DateTime<Utc>,
    /// Whether the code has been used
    pub used: bool,
    /// Client information that used this code
    pub client_info: Option<String>,
}

impl PairingEntry {
    /// Create a new pairing entry.
    pub fn new(session_id: Option<String>, project_path: Option<PathBuf>) -> Self {
        let code = generate_pairing_code();
        let created_at = Utc::now();
        let expires_at = created_at + PAIRING_CODE_DURATION;

        Self {
            code,
            session_id,
            project_path,
            created_at,
            expires_at,
            used: false,
            client_info: None,
        }
    }

    /// Check if the pairing code is valid (not expired and not used).
    pub fn is_valid(&self) -> bool {
        !self.used && Utc::now() < self.expires_at
    }

    /// Mark the pairing code as used.
    pub fn mark_used(&mut self, client_info: Option<String>) {
        self.used = true;
        self.client_info = client_info;
    }
}

/// Pairing code storage format.
#[derive(Debug, Serialize, Deserialize)]
struct PairingStorage {
    /// All pairing entries by code
    entries: HashMap<String, PairingEntry>,
    /// Version for compatibility
    version: u32,
}

impl Default for PairingStorage {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            version: 1,
        }
    }
}

/// Manager for pairing codes.
pub struct PairingManager {
    /// Path to the pairing file
    file_path: PathBuf,
    /// In-memory pairing storage
    storage: PairingStorage,
}

impl PairingManager {
    /// Create a new pairing manager.
    pub fn new() -> Result<Self> {
        let file_path = commander_core::config::pairing_file();
        let storage = Self::load_storage(&file_path)?;

        Ok(Self {
            file_path,
            storage,
        })
    }

    /// Generate a new pairing code.
    pub fn generate_code(
        &mut self,
        session_id: Option<String>,
        project_path: Option<PathBuf>,
    ) -> Result<String> {
        // Clean expired codes before generating new ones
        self.cleanup_expired()?;

        let entry = PairingEntry::new(session_id, project_path);
        let code = entry.code.clone();

        self.storage.entries.insert(code.clone(), entry);
        self.save_storage()?;

        Ok(code)
    }

    /// Validate and consume a pairing code.
    pub fn validate_code(
        &mut self,
        code: &str,
        client_info: Option<String>,
    ) -> Result<Option<PairingEntry>> {
        // Clean expired codes first
        self.cleanup_expired()?;

        // Check if entry exists and is valid
        let is_valid = self.storage.entries
            .get(code)
            .map(|entry| entry.is_valid())
            .unwrap_or(false);

        if is_valid {
            if let Some(entry) = self.storage.entries.get_mut(code) {
                entry.mark_used(client_info);
                let result = entry.clone();
                self.save_storage()?;
                return Ok(Some(result));
            }
        }

        Ok(None)
    }

    /// List all valid pairing codes.
    pub fn list_valid_codes(&self) -> Vec<&PairingEntry> {
        self.storage
            .entries
            .values()
            .filter(|entry| entry.is_valid())
            .collect()
    }

    /// Get a pairing entry by code.
    pub fn get_entry(&self, code: &str) -> Option<&PairingEntry> {
        self.storage.entries.get(code)
    }

    /// Remove a pairing code.
    pub fn remove_code(&mut self, code: &str) -> Result<Option<PairingEntry>> {
        let entry = self.storage.entries.remove(code);
        if entry.is_some() {
            self.save_storage()?;
        }
        Ok(entry)
    }

    /// Clean up expired pairing codes.
    pub fn cleanup_expired(&mut self) -> Result<()> {
        let now = Utc::now();
        let initial_count = self.storage.entries.len();

        self.storage.entries.retain(|_, entry| entry.expires_at > now);

        let removed_count = initial_count - self.storage.entries.len();
        if removed_count > 0 {
            self.save_storage()?;
            tracing::debug!(
                removed_count = removed_count,
                remaining_count = self.storage.entries.len(),
                "Cleaned up expired pairing codes"
            );
        }

        Ok(())
    }

    /// Get statistics about pairing codes.
    pub fn get_statistics(&self) -> PairingStatistics {
        let now = Utc::now();
        let mut stats = PairingStatistics::default();

        for entry in self.storage.entries.values() {
            stats.total += 1;

            if entry.expires_at <= now {
                stats.expired += 1;
            } else if entry.used {
                stats.used += 1;
            } else {
                stats.valid += 1;
            }
        }

        stats
    }

    /// Load storage from file.
    fn load_storage(file_path: &PathBuf) -> Result<PairingStorage> {
        if !file_path.exists() {
            return Ok(PairingStorage::default());
        }

        let content = fs::read_to_string(file_path)
            .map_err(|e| DaemonError::Pairing(format!("Failed to read pairing file: {}", e)))?;

        // Try to parse as new format first, then fallback to empty object for migration
        let storage: PairingStorage = match serde_json::from_str(&content) {
            Ok(storage) => storage,
            Err(_) => {
                // Handle legacy format or empty object
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(value) if value.is_object() => {
                        // Legacy format or empty object - create new format
                        PairingStorage::default()
                    }
                    _ => {
                        return Err(DaemonError::Pairing(format!(
                            "Failed to parse pairing file: invalid JSON format"
                        )));
                    }
                }
            }
        };

        Ok(storage)
    }

    /// Save storage to file.
    fn save_storage(&self) -> Result<()> {
        // Ensure the parent directory exists
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DaemonError::Pairing(format!("Failed to create config directory: {}", e)))?;
        }

        let content = serde_json::to_string_pretty(&self.storage)
            .map_err(|e| DaemonError::Pairing(format!("Failed to serialize pairing data: {}", e)))?;

        fs::write(&self.file_path, content)
            .map_err(|e| DaemonError::Pairing(format!("Failed to write pairing file: {}", e)))?;

        Ok(())
    }
}

impl Default for PairingManager {
    fn default() -> Self {
        Self::new().expect("Failed to create pairing manager")
    }
}

/// Statistics about pairing codes.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PairingStatistics {
    /// Total number of pairing codes
    pub total: usize,
    /// Number of valid (unused, non-expired) codes
    pub valid: usize,
    /// Number of used codes
    pub used: usize,
    /// Number of expired codes
    pub expired: usize,
}

/// Generate a random pairing code.
fn generate_pairing_code() -> String {
    // Generate a 6-character alphanumeric code
    let uuid = Uuid::new_v4();
    let hex = uuid.simple().to_string();

    // Take first 6 characters and make uppercase for readability
    hex[..6].to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_pairing_entry_creation() {
        let entry = PairingEntry::new(Some("test-session".to_string()), None);

        assert!(!entry.code.is_empty());
        assert_eq!(entry.session_id, Some("test-session".to_string()));
        assert!(!entry.used);
        assert!(entry.is_valid());
    }

    #[test]
    fn test_pairing_entry_expiration() {
        let mut entry = PairingEntry::new(None, None);

        // Force expiration
        entry.expires_at = Utc::now() - chrono::Duration::seconds(1);

        assert!(!entry.is_valid());
    }

    #[test]
    fn test_pairing_manager() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_pairings.json");

        let mut manager = PairingManager {
            file_path,
            storage: PairingStorage::default(),
        };

        // Generate a code
        let code = manager.generate_code(Some("test-session".to_string()), None).unwrap();
        assert!(!code.is_empty());

        // Validate the code
        let entry = manager.validate_code(&code, Some("test-client".to_string())).unwrap();
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.session_id, Some("test-session".to_string()));
        assert_eq!(entry.client_info, Some("test-client".to_string()));

        // Code should not be valid anymore (used)
        let entry2 = manager.validate_code(&code, None).unwrap();
        assert!(entry2.is_none());
    }

    #[test]
    fn test_generate_pairing_code() {
        let code1 = generate_pairing_code();
        let code2 = generate_pairing_code();

        assert_eq!(code1.len(), 6);
        assert_eq!(code2.len(), 6);
        assert_ne!(code1, code2);
        assert!(code1.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(code2.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
