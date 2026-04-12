//! Web client token store for session management after pairing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A paired web client with its session token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebClient {
    /// Bearer token used for subsequent API requests.
    pub token: String,
    /// When the client was first paired.
    pub paired_at: DateTime<Utc>,
    /// When the token was last used.
    pub last_seen: DateTime<Utc>,
    /// Optional user-agent or device name supplied at pairing time.
    pub client_info: Option<String>,
}

/// Persisted format for the web clients file.
#[derive(Debug, Serialize, Deserialize)]
struct WebClientStorage {
    /// Map of token → client.
    clients: HashMap<String, WebClient>,
    /// Storage format version.
    version: u32,
}

impl Default for WebClientStorage {
    fn default() -> Self {
        Self {
            clients: HashMap::new(),
            version: 1,
        }
    }
}

/// Thread-safe store for web client session tokens.
///
/// Tokens are persisted to `<storage_dir>/web_clients.json` so that
/// clients survive daemon restarts.
#[derive(Clone)]
pub struct WebClientStore {
    inner: Arc<RwLock<WebClientStorage>>,
    storage_path: PathBuf,
}

impl WebClientStore {
    /// Create a new store, loading existing clients from `storage_dir`.
    pub fn new(storage_dir: &PathBuf) -> Self {
        let storage_path = storage_dir.join("web_clients.json");
        let inner = Self::load(&storage_path).unwrap_or_default();

        Self {
            inner: Arc::new(RwLock::new(inner)),
            storage_path,
        }
    }

    /// Create a new web client, generate a UUID token, persist, and return it.
    pub fn create_client(&self, client_info: Option<String>) -> Result<WebClient, String> {
        let token = Uuid::new_v4().to_string();
        let now = Utc::now();
        let client = WebClient {
            token: token.clone(),
            paired_at: now,
            last_seen: now,
            client_info,
        };

        {
            let mut storage = self
                .inner
                .write()
                .map_err(|e| format!("lock poisoned: {}", e))?;
            storage.clients.insert(token, client.clone());
        }

        self.save()?;
        Ok(client)
    }

    /// Return a copy of the client if the token exists.
    pub fn validate_token(&self, token: &str) -> Option<WebClient> {
        let storage = self.inner.read().ok()?;
        storage.clients.get(token).cloned()
    }

    /// Refresh `last_seen` for an existing token.
    pub fn update_last_seen(&self, token: &str) {
        if let Ok(mut storage) = self.inner.write() {
            if let Some(client) = storage.clients.get_mut(token) {
                client.last_seen = Utc::now();
            }
        }
        let _ = self.save();
    }

    /// Load storage from disk, returning `None` on any error.
    fn load(path: &PathBuf) -> Option<WebClientStorage> {
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Persist storage to disk.
    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create storage dir: {}", e))?;
        }

        let storage = self
            .inner
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;

        let content = serde_json::to_string_pretty(&*storage)
            .map_err(|e| format!("failed to serialize web clients: {}", e))?;

        std::fs::write(&self.storage_path, content)
            .map_err(|e| format!("failed to write web clients file: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_validate_client() {
        let dir = tempdir().unwrap();
        let store = WebClientStore::new(&dir.path().to_path_buf());

        let client = store
            .create_client(Some("test-agent".to_string()))
            .unwrap();

        assert!(!client.token.is_empty());
        assert_eq!(client.client_info, Some("test-agent".to_string()));

        let found = store.validate_token(&client.token);
        assert!(found.is_some());
        assert_eq!(found.unwrap().client_info, Some("test-agent".to_string()));
    }

    #[test]
    fn test_validate_unknown_token() {
        let dir = tempdir().unwrap();
        let store = WebClientStore::new(&dir.path().to_path_buf());
        assert!(store.validate_token("does-not-exist").is_none());
    }

    #[test]
    fn test_update_last_seen() {
        let dir = tempdir().unwrap();
        let store = WebClientStore::new(&dir.path().to_path_buf());

        let client = store.create_client(None).unwrap();
        let before = client.last_seen;

        std::thread::sleep(std::time::Duration::from_millis(10));
        store.update_last_seen(&client.token);

        let updated = store.validate_token(&client.token).unwrap();
        assert!(updated.last_seen >= before);
    }

    #[test]
    fn test_persistence_across_store_instances() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let token = {
            let store = WebClientStore::new(&path);
            store.create_client(None).unwrap().token
        };

        // Create a fresh store pointing at the same directory.
        let store2 = WebClientStore::new(&path);
        assert!(store2.validate_token(&token).is_some());
    }
}
