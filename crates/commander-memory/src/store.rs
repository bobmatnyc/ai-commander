//! MemoryStore trait definition for vector database backends.
//!
//! This module defines the interface that all memory storage backends must implement,
//! allowing for different vector database implementations (Qdrant, etc.).
//!
//! # Access Control
//!
//! The module provides an [`AccessLevel`] system to enforce memory isolation:
//! - Session agents use `AccessLevel::Own` - can only access their own memories
//! - User agent uses `AccessLevel::All` - can access all memories across agents
//!
//! Use [`AccessControlledStore`] to wrap any `MemoryStore` with automatic access
//! level enforcement.

use async_trait::async_trait;
use std::sync::Arc;

use crate::error::Result;
use crate::memory::{Memory, SearchResult};

/// Access level for memory operations.
///
/// Controls which memories an agent can access during search operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessLevel {
    /// Can only access own memories (session agents).
    /// Search operations are filtered to only return memories
    /// belonging to the requesting agent.
    Own,
    /// Can access all memories (user agent).
    /// Search operations return memories from all agents,
    /// enabling cross-agent context gathering.
    All,
}

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

    /// Search with explicit access control.
    ///
    /// This method enforces access control based on the provided `AccessLevel`:
    /// - `AccessLevel::Own`: Only returns memories belonging to `agent_id`
    /// - `AccessLevel::All`: Returns memories from all agents
    ///
    /// # Arguments
    /// * `query_embedding` - The embedding vector to search for
    /// * `agent_id` - The requesting agent's ID
    /// * `access` - The access level to enforce
    /// * `limit` - Maximum number of results to return
    ///
    /// # Returns
    /// A vector of search results ordered by similarity (highest first).
    async fn search_with_access(
        &self,
        query_embedding: &[f32],
        agent_id: &str,
        access: AccessLevel,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        tracing::debug!(
            agent_id = %agent_id,
            access = ?access,
            limit = limit,
            "Memory search with access control"
        );

        match access {
            AccessLevel::Own => self.search(query_embedding, agent_id, limit).await,
            AccessLevel::All => self.search_all(query_embedding, limit).await,
        }
    }
}

/// Wrapper that enforces access control on memory operations.
///
/// This struct wraps any `MemoryStore` implementation and provides
/// automatic access level enforcement. It binds an agent ID and access
/// level at construction time, ensuring consistent access control.
///
/// # Example
///
/// ```no_run
/// use commander_memory::{LocalStore, AccessControlledStore, AccessLevel};
/// use std::sync::Arc;
///
/// # async fn example() -> commander_memory::Result<()> {
/// let store = Arc::new(LocalStore::default().await?);
///
/// // Create an access-controlled wrapper for a session agent (isolated)
/// let session_store = AccessControlledStore::new(
///     store.clone(),
///     "session-agent-1".to_string(),
///     AccessLevel::Own,
/// );
///
/// // Create an access-controlled wrapper for the user agent (privileged)
/// let user_store = AccessControlledStore::new(
///     store,
///     "user-agent".to_string(),
///     AccessLevel::All,
/// );
/// # Ok(())
/// # }
/// ```
pub struct AccessControlledStore<S: MemoryStore> {
    inner: Arc<S>,
    agent_id: String,
    access_level: AccessLevel,
}

impl<S: MemoryStore> AccessControlledStore<S> {
    /// Create a new access-controlled store wrapper.
    ///
    /// # Arguments
    /// * `inner` - The underlying memory store
    /// * `agent_id` - The agent ID for access control
    /// * `access_level` - The access level to enforce
    pub fn new(inner: Arc<S>, agent_id: String, access_level: AccessLevel) -> Self {
        tracing::debug!(
            agent_id = %agent_id,
            access = ?access_level,
            "Creating access-controlled store"
        );
        Self {
            inner,
            agent_id,
            access_level,
        }
    }

    /// Get the agent ID.
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Get the access level.
    pub fn access_level(&self) -> AccessLevel {
        self.access_level
    }

    /// Search memories respecting the configured access level.
    ///
    /// This method automatically applies the correct access control:
    /// - For `AccessLevel::Own`: Only searches this agent's memories
    /// - For `AccessLevel::All`: Searches across all agents
    pub async fn search(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        self.inner
            .search_with_access(query_embedding, &self.agent_id, self.access_level, limit)
            .await
    }

    /// Store a memory (always stores with this agent's ID).
    pub async fn store(&self, mut memory: Memory) -> Result<()> {
        // Ensure the memory is tagged with this agent's ID
        memory.agent_id = self.agent_id.clone();
        self.inner.store(memory).await
    }

    /// Get a memory by ID.
    ///
    /// Note: This performs access control validation - agents with `AccessLevel::Own`
    /// can only retrieve their own memories.
    pub async fn get(&self, id: &str) -> Result<Option<Memory>> {
        let memory = self.inner.get(id).await?;

        // Enforce access control on get
        if let Some(ref mem) = memory {
            if self.access_level == AccessLevel::Own && mem.agent_id != self.agent_id {
                tracing::warn!(
                    agent_id = %self.agent_id,
                    target_agent = %mem.agent_id,
                    memory_id = %id,
                    "Access violation: agent attempted to access another agent's memory"
                );
                return Ok(None);
            }
        }

        Ok(memory)
    }

    /// Delete a memory by ID.
    ///
    /// Note: Agents with `AccessLevel::Own` can only delete their own memories.
    pub async fn delete(&self, id: &str) -> Result<()> {
        // Check access control before delete
        if self.access_level == AccessLevel::Own {
            if let Some(memory) = self.inner.get(id).await? {
                if memory.agent_id != self.agent_id {
                    tracing::warn!(
                        agent_id = %self.agent_id,
                        target_agent = %memory.agent_id,
                        memory_id = %id,
                        "Access violation: agent attempted to delete another agent's memory"
                    );
                    return Ok(()); // Silent no-op for unauthorized delete
                }
            }
        }
        self.inner.delete(id).await
    }

    /// List memories for this agent.
    pub async fn list(&self, limit: usize) -> Result<Vec<Memory>> {
        self.inner.list(&self.agent_id, limit).await
    }

    /// Count this agent's memories.
    pub async fn count(&self) -> Result<usize> {
        self.inner.count(&self.agent_id).await
    }

    /// Clear this agent's memories.
    pub async fn clear(&self) -> Result<()> {
        self.inner.clear_agent(&self.agent_id).await
    }

    /// Get the inner store (for advanced operations).
    pub fn inner(&self) -> &S {
        &self.inner
    }
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

    #[tokio::test]
    async fn test_search_with_access_own() {
        let store = MockStore::new();
        let embedding = vec![0.1; 10];

        store
            .store(Memory::new("agent-1", "memory 1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-2", "memory 2", embedding.clone()))
            .await
            .unwrap();

        // AccessLevel::Own should only return own agent's memories
        let results = store
            .search_with_access(&embedding, "agent-1", AccessLevel::Own, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.agent_id, "agent-1");
    }

    #[tokio::test]
    async fn test_search_with_access_all() {
        let store = MockStore::new();
        let embedding = vec![0.1; 10];

        store
            .store(Memory::new("agent-1", "memory 1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-2", "memory 2", embedding.clone()))
            .await
            .unwrap();

        // AccessLevel::All should return all agents' memories
        let results = store
            .search_with_access(&embedding, "agent-1", AccessLevel::All, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_access_controlled_store_own() {
        let store = Arc::new(MockStore::new());
        let embedding = vec![0.1; 10];

        // Store memories for different agents
        store
            .store(Memory::with_id("mem-1", "agent-1", "secret 1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::with_id("mem-2", "agent-2", "secret 2", embedding.clone()))
            .await
            .unwrap();

        // Create access-controlled wrapper with Own access
        let controlled = AccessControlledStore::new(
            store.clone(),
            "agent-1".to_string(),
            AccessLevel::Own,
        );

        // Should only find own memories
        let results = controlled.search(&embedding, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.content, "secret 1");

        // Should be able to get own memory
        let own_mem = controlled.get("mem-1").await.unwrap();
        assert!(own_mem.is_some());

        // Should NOT be able to get other agent's memory
        let other_mem = controlled.get("mem-2").await.unwrap();
        assert!(other_mem.is_none());
    }

    #[tokio::test]
    async fn test_access_controlled_store_all() {
        let store = Arc::new(MockStore::new());
        let embedding = vec![0.1; 10];

        // Store memories for different agents
        store
            .store(Memory::with_id("mem-1", "agent-1", "secret 1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::with_id("mem-2", "agent-2", "secret 2", embedding.clone()))
            .await
            .unwrap();

        // Create access-controlled wrapper with All access (user agent)
        let controlled = AccessControlledStore::new(
            store.clone(),
            "user-agent".to_string(),
            AccessLevel::All,
        );

        // Should find all memories
        let results = controlled.search(&embedding, 10).await.unwrap();
        assert_eq!(results.len(), 2);

        // Should be able to get any memory
        let mem1 = controlled.get("mem-1").await.unwrap();
        assert!(mem1.is_some());

        let mem2 = controlled.get("mem-2").await.unwrap();
        assert!(mem2.is_some());
    }

    #[tokio::test]
    async fn test_access_controlled_store_delete_isolation() {
        let store = Arc::new(MockStore::new());
        let embedding = vec![0.1; 10];

        // Store memories for different agents
        store
            .store(Memory::with_id("mem-1", "agent-1", "secret 1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::with_id("mem-2", "agent-2", "secret 2", embedding.clone()))
            .await
            .unwrap();

        // Create access-controlled wrapper with Own access
        let controlled = AccessControlledStore::new(
            store.clone(),
            "agent-1".to_string(),
            AccessLevel::Own,
        );

        // Should NOT be able to delete other agent's memory
        controlled.delete("mem-2").await.unwrap();

        // Other agent's memory should still exist
        let other_mem = store.get("mem-2").await.unwrap();
        assert!(other_mem.is_some());

        // Should be able to delete own memory
        controlled.delete("mem-1").await.unwrap();
        let own_mem = store.get("mem-1").await.unwrap();
        assert!(own_mem.is_none());
    }

    #[test]
    fn test_access_level_equality() {
        assert_eq!(AccessLevel::Own, AccessLevel::Own);
        assert_eq!(AccessLevel::All, AccessLevel::All);
        assert_ne!(AccessLevel::Own, AccessLevel::All);
    }
}
