//! Local file-based memory store for development and testing.
//!
//! This module provides a simple file-based implementation of the MemoryStore trait
//! that persists memories to JSON files. It uses brute-force cosine similarity search,
//! which is suitable for small collections (< 10,000 memories).
//!
//! For production use with larger collections, use the Qdrant backend.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::embedding::cosine_similarity;
use crate::error::{MemoryError, Result};
use crate::memory::{Memory, SearchResult};
use crate::store::MemoryStore;

/// Local file-based memory store.
///
/// Stores memories in a JSON file and uses brute-force cosine similarity
/// for search operations. Suitable for development and small-scale use.
pub struct LocalStore {
    /// Path to the storage directory.
    storage_dir: PathBuf,
    /// In-memory cache of all memories, keyed by ID.
    memories: RwLock<HashMap<String, Memory>>,
}

impl LocalStore {
    /// Create a new local store at the specified path.
    ///
    /// # Arguments
    /// * `storage_dir` - Directory to store memory data
    pub async fn new(storage_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&storage_dir)?;

        let store = Self {
            storage_dir,
            memories: RwLock::new(HashMap::new()),
        };

        store.load().await?;
        Ok(store)
    }

    /// Create a store using the default Commander data directory.
    ///
    /// Stores data in `~/.ai-commander/db/chroma/` (using the configured path).
    pub async fn default() -> Result<Self> {
        let path = commander_core::chroma_dir();
        info!(path = %path.display(), "Initializing local memory store");
        Self::new(path).await
    }

    fn data_file(&self) -> PathBuf {
        self.storage_dir.join("memories.json")
    }

    async fn load(&self) -> Result<()> {
        let file = self.data_file();
        if !file.exists() {
            debug!(path = %file.display(), "No existing memories file");
            return Ok(());
        }

        let data = std::fs::read_to_string(&file)?;
        let memories: Vec<Memory> =
            serde_json::from_str(&data).map_err(MemoryError::SerializationError)?;

        let mut store = self.memories.write().await;
        for memory in memories {
            store.insert(memory.id.clone(), memory);
        }

        info!(count = store.len(), "Loaded memories from disk");
        Ok(())
    }

    async fn save(&self) -> Result<()> {
        let memories = self.memories.read().await;
        let data: Vec<&Memory> = memories.values().collect();
        let json = serde_json::to_string_pretty(&data)?;

        // Atomic write via temp file
        let file = self.data_file();
        let temp_file = file.with_extension("json.tmp");
        std::fs::write(&temp_file, json)?;
        std::fs::rename(&temp_file, &file)?;

        debug!(count = memories.len(), "Saved memories to disk");
        Ok(())
    }
}

#[async_trait]
impl MemoryStore for LocalStore {
    async fn store(&self, memory: Memory) -> Result<()> {
        {
            let mut memories = self.memories.write().await;
            debug!(id = %memory.id, agent_id = %memory.agent_id, "Storing memory");
            memories.insert(memory.id.clone(), memory);
        }
        self.save().await
    }

    async fn search(
        &self,
        query_embedding: &[f32],
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let memories = self.memories.read().await;

        let mut results: Vec<SearchResult> = memories
            .values()
            .filter(|m| m.agent_id == agent_id)
            .map(|m| {
                let score = cosine_similarity(query_embedding, &m.embedding);
                SearchResult::new(m.clone(), score)
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);

        Ok(results)
    }

    async fn search_all(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        let memories = self.memories.read().await;

        let mut results: Vec<SearchResult> = memories
            .values()
            .map(|m| {
                let score = cosine_similarity(query_embedding, &m.embedding);
                SearchResult::new(m.clone(), score)
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);

        Ok(results)
    }

    async fn delete(&self, id: &str) -> Result<()> {
        {
            let mut memories = self.memories.write().await;
            memories.remove(id);
        }
        self.save().await
    }

    async fn get(&self, id: &str) -> Result<Option<Memory>> {
        let memories = self.memories.read().await;
        Ok(memories.get(id).cloned())
    }

    async fn list(&self, agent_id: &str, limit: usize) -> Result<Vec<Memory>> {
        let memories = self.memories.read().await;
        Ok(memories
            .values()
            .filter(|m| m.agent_id == agent_id)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn count(&self, agent_id: &str) -> Result<usize> {
        let memories = self.memories.read().await;
        Ok(memories.values().filter(|m| m.agent_id == agent_id).count())
    }

    async fn clear_agent(&self, agent_id: &str) -> Result<()> {
        {
            let mut memories = self.memories.write().await;
            memories.retain(|_, m| m.agent_id != agent_id);
        }
        self.save().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_store() -> (LocalStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = LocalStore::new(temp_dir.path().to_path_buf()).await.unwrap();
        (store, temp_dir)
    }

    #[tokio::test]
    async fn test_store_and_get() {
        let (store, _dir) = create_test_store().await;

        let embedding = vec![0.1; 10];
        let memory = Memory::new("agent-1", "test content", embedding);
        let id = memory.id.clone();

        store.store(memory).await.unwrap();

        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "test content");
    }

    #[tokio::test]
    async fn test_delete() {
        let (store, _dir) = create_test_store().await;

        let embedding = vec![0.1; 10];
        let memory = Memory::new("agent-1", "to delete", embedding);
        let id = memory.id.clone();

        store.store(memory).await.unwrap();
        assert!(store.get(&id).await.unwrap().is_some());

        store.delete(&id).await.unwrap();
        assert!(store.get(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_search_by_agent() {
        let (store, _dir) = create_test_store().await;

        // Store memories for different agents
        let embedding1 = vec![1.0, 0.0, 0.0];
        let embedding2 = vec![0.0, 1.0, 0.0];
        let embedding3 = vec![0.9, 0.1, 0.0]; // Similar to embedding1

        store
            .store(Memory::new("agent-1", "memory 1", embedding1.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-2", "memory 2", embedding2.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-1", "memory 3", embedding3.clone()))
            .await
            .unwrap();

        // Search should only return agent-1's memories
        let results = store.search(&embedding1, "agent-1", 10).await.unwrap();
        assert_eq!(results.len(), 2);
        for r in &results {
            assert_eq!(r.memory.agent_id, "agent-1");
        }
    }

    #[tokio::test]
    async fn test_search_all() {
        let (store, _dir) = create_test_store().await;

        let embedding1 = vec![1.0, 0.0, 0.0];
        let embedding2 = vec![0.0, 1.0, 0.0];

        store
            .store(Memory::new("agent-1", "memory 1", embedding1.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-2", "memory 2", embedding2.clone()))
            .await
            .unwrap();

        let results = store.search_all(&embedding1, 10).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_search_ranking() {
        let (store, _dir) = create_test_store().await;

        // Create memories with known similarity to query
        let query = vec![1.0, 0.0, 0.0];
        let exact_match = vec![1.0, 0.0, 0.0];
        let partial_match = vec![0.7, 0.7, 0.0];
        let no_match = vec![0.0, 0.0, 1.0];

        store
            .store(Memory::with_id("exact", "agent-1", "exact match", exact_match))
            .await
            .unwrap();
        store
            .store(Memory::with_id(
                "partial",
                "agent-1",
                "partial match",
                partial_match,
            ))
            .await
            .unwrap();
        store
            .store(Memory::with_id("none", "agent-1", "no match", no_match))
            .await
            .unwrap();

        let results = store.search(&query, "agent-1", 3).await.unwrap();

        // Should be sorted by similarity: exact > partial > none
        assert_eq!(results[0].memory.id, "exact");
        assert!(results[0].score > results[1].score);
        assert!(results[1].score > results[2].score);
    }

    #[tokio::test]
    async fn test_count() {
        let (store, _dir) = create_test_store().await;

        let embedding = vec![0.1; 10];

        store
            .store(Memory::new("agent-1", "m1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-1", "m2", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-2", "m3", embedding.clone()))
            .await
            .unwrap();

        assert_eq!(store.count("agent-1").await.unwrap(), 2);
        assert_eq!(store.count("agent-2").await.unwrap(), 1);
        assert_eq!(store.count("agent-3").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_clear_agent() {
        let (store, _dir) = create_test_store().await;

        let embedding = vec![0.1; 10];

        store
            .store(Memory::new("agent-1", "m1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-1", "m2", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-2", "m3", embedding.clone()))
            .await
            .unwrap();

        store.clear_agent("agent-1").await.unwrap();

        assert_eq!(store.count("agent-1").await.unwrap(), 0);
        assert_eq!(store.count("agent-2").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_path_buf();

        // Create store, add memory
        {
            let store = LocalStore::new(path.clone()).await.unwrap();
            let embedding = vec![0.1; 10];
            store
                .store(Memory::with_id("persist-test", "agent-1", "test", embedding))
                .await
                .unwrap();
        }

        // Create new store, should load from disk
        {
            let store = LocalStore::new(path).await.unwrap();
            let memory = store.get("persist-test").await.unwrap();
            assert!(memory.is_some());
            assert_eq!(memory.unwrap().content, "test");
        }
    }

    #[tokio::test]
    async fn test_list() {
        let (store, _dir) = create_test_store().await;

        let embedding = vec![0.1; 10];

        store
            .store(Memory::new("agent-1", "m1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-1", "m2", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-2", "m3", embedding.clone()))
            .await
            .unwrap();

        let list = store.list("agent-1", 10).await.unwrap();
        assert_eq!(list.len(), 2);
        for m in &list {
            assert_eq!(m.agent_id, "agent-1");
        }
    }
}
