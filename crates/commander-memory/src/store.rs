//! MemoryStore trait definition for vector database backends.
//!
//! This module defines the interface that all memory storage backends must implement,
//! allowing for different vector database implementations (Qdrant, etc.).

use async_trait::async_trait;

use crate::error::Result;
use crate::memory::{Memory, SearchResult};

/// Trait for memory storage backends.
///
/// Implementations must support basic CRUD operations plus semantic search.
/// All operations are async to support both local and remote backends.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Store a memory in the database.
    ///
    /// If a memory with the same ID already exists, it will be updated.
    ///
    /// # Arguments
    /// * `memory` - The memory to store
    async fn store(&self, memory: Memory) -> Result<()>;

    /// Search for similar memories within a specific agent's context.
    ///
    /// # Arguments
    /// * `query_embedding` - The embedding vector to search for
    /// * `agent_id` - Filter results to only this agent's memories
    /// * `limit` - Maximum number of results to return
    ///
    /// # Returns
    /// A vector of search results ordered by similarity (highest first).
    async fn search(
        &self,
        query_embedding: &[f32],
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>>;

    /// Search for similar memories across all agents.
    ///
    /// This is useful for the "user" agent that needs global context.
    ///
    /// # Arguments
    /// * `query_embedding` - The embedding vector to search for
    /// * `limit` - Maximum number of results to return
    ///
    /// # Returns
    /// A vector of search results ordered by similarity (highest first).
    async fn search_all(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<SearchResult>>;

    /// Delete a memory by ID.
    ///
    /// # Arguments
    /// * `id` - The memory ID to delete
    ///
    /// # Returns
    /// `Ok(())` if deleted successfully, or if the memory didn't exist.
    async fn delete(&self, id: &str) -> Result<()>;

    /// Get a specific memory by ID.
    ///
    /// # Arguments
    /// * `id` - The memory ID to retrieve
    ///
    /// # Returns
    /// The memory if found, `None` otherwise.
    async fn get(&self, id: &str) -> Result<Option<Memory>>;

    /// List all memories for a specific agent.
    ///
    /// # Arguments
    /// * `agent_id` - The agent whose memories to list
    /// * `limit` - Maximum number of results
    ///
    /// # Returns
    /// A vector of memories (not sorted by similarity).
    async fn list(&self, agent_id: &str, limit: usize) -> Result<Vec<Memory>>;

    /// Count memories for a specific agent.
    ///
    /// # Arguments
    /// * `agent_id` - The agent whose memories to count
    async fn count(&self, agent_id: &str) -> Result<usize>;

    /// Delete all memories for a specific agent.
    ///
    /// # Arguments
    /// * `agent_id` - The agent whose memories to delete
    async fn clear_agent(&self, agent_id: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::DEFAULT_EMBEDDING_DIM;
    use std::sync::Arc;

    /// Mock implementation for testing the trait interface.
    struct MockStore {
        memories: tokio::sync::RwLock<Vec<Memory>>,
    }

    impl MockStore {
        fn new() -> Self {
            Self {
                memories: tokio::sync::RwLock::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl MemoryStore for MockStore {
        async fn store(&self, memory: Memory) -> Result<()> {
            let mut memories = self.memories.write().await;
            memories.retain(|m| m.id != memory.id);
            memories.push(memory);
            Ok(())
        }

        async fn search(
            &self,
            _query_embedding: &[f32],
            agent_id: &str,
            limit: usize,
        ) -> Result<Vec<SearchResult>> {
            let memories = self.memories.read().await;
            Ok(memories
                .iter()
                .filter(|m| m.agent_id == agent_id)
                .take(limit)
                .map(|m| SearchResult::new(m.clone(), 0.9))
                .collect())
        }

        async fn search_all(
            &self,
            _query_embedding: &[f32],
            limit: usize,
        ) -> Result<Vec<SearchResult>> {
            let memories = self.memories.read().await;
            Ok(memories
                .iter()
                .take(limit)
                .map(|m| SearchResult::new(m.clone(), 0.9))
                .collect())
        }

        async fn delete(&self, id: &str) -> Result<()> {
            let mut memories = self.memories.write().await;
            memories.retain(|m| m.id != id);
            Ok(())
        }

        async fn get(&self, id: &str) -> Result<Option<Memory>> {
            let memories = self.memories.read().await;
            Ok(memories.iter().find(|m| m.id == id).cloned())
        }

        async fn list(&self, agent_id: &str, limit: usize) -> Result<Vec<Memory>> {
            let memories = self.memories.read().await;
            Ok(memories
                .iter()
                .filter(|m| m.agent_id == agent_id)
                .take(limit)
                .cloned()
                .collect())
        }

        async fn count(&self, agent_id: &str) -> Result<usize> {
            let memories = self.memories.read().await;
            Ok(memories.iter().filter(|m| m.agent_id == agent_id).count())
        }

        async fn clear_agent(&self, agent_id: &str) -> Result<()> {
            let mut memories = self.memories.write().await;
            memories.retain(|m| m.agent_id != agent_id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_mock_store_basic_operations() {
        let store: Arc<dyn MemoryStore> = Arc::new(MockStore::new());
        let embedding = vec![0.1; DEFAULT_EMBEDDING_DIM];

        // Store
        let memory = Memory::new("agent-1", "test content", embedding.clone());
        let id = memory.id.clone();
        store.store(memory).await.unwrap();

        // Get
        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "test content");

        // Count
        assert_eq!(store.count("agent-1").await.unwrap(), 1);

        // Delete
        store.delete(&id).await.unwrap();
        assert!(store.get(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_store_search() {
        let store = MockStore::new();
        let embedding = vec![0.1; 10];

        store
            .store(Memory::new("agent-1", "memory 1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-1", "memory 2", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-2", "memory 3", embedding.clone()))
            .await
            .unwrap();

        // Search specific agent
        let results = store.search(&embedding, "agent-1", 10).await.unwrap();
        assert_eq!(results.len(), 2);

        // Search all
        let results = store.search_all(&embedding, 10).await.unwrap();
        assert_eq!(results.len(), 3);
    }
}
