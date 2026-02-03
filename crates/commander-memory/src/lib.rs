//! Vector database memory storage with semantic search for AI agents.
//!
//! This crate provides memory storage capabilities for AI agents using vector
//! embeddings for semantic search. It supports multiple backends:
//!
//! - **LocalStore**: File-based storage for development and small-scale use
//! - **QdrantStore**: Qdrant vector database for production use
//!
//! # Storage Location
//!
//! By default, memories are stored in `~/.ai-commander/db/chroma/` (configurable
//! via `COMMANDER_DB_DIR` environment variable).
//!
//! # Example
//!
//! ```no_run
//! use commander_memory::{Memory, LocalStore, MemoryStore, EmbeddingGenerator};
//!
//! # async fn example() -> commander_memory::Result<()> {
//! // Create a local store (uses default path)
//! let store = LocalStore::default().await?;
//!
//! // Create an embedding generator (uses API if available, falls back to hash)
//! let embedder = EmbeddingGenerator::from_env();
//!
//! // Generate embedding for content
//! let embedding = embedder.embed("Important information to remember").await?;
//!
//! // Create and store a memory
//! let memory = Memory::new("agent-1", "Important information to remember", embedding);
//! store.store(memory).await?;
//!
//! // Search for similar memories
//! let query_embedding = embedder.embed("What was the important info?").await?;
//! let results = store.search(&query_embedding, "agent-1", 5).await?;
//!
//! for result in results {
//!     println!("Score: {:.2}, Content: {}", result.score, result.memory.content);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Embedding Providers
//!
//! The crate supports multiple embedding providers with automatic fallback:
//!
//! 1. **OpenAI** (set `OPENAI_API_KEY`): Uses `text-embedding-3-small`
//! 2. **OpenRouter** (set `OPENROUTER_API_KEY`): Uses `openai/text-embedding-3-small`
//! 3. **Hash-based** (no API key): Deterministic hash-based embeddings for testing
//!
//! # Agent Isolation and Access Control
//!
//! Memories are tagged with an `agent_id` for isolation. The crate provides
//! explicit access control through the [`AccessLevel`] enum:
//!
//! - `AccessLevel::Own`: Session agents can only access their own memories
//! - `AccessLevel::All`: User agent has privileged access to all memories
//!
//! Use [`AccessControlledStore`] to wrap any `MemoryStore` with automatic
//! access control enforcement:
//!
//! ```no_run
//! use commander_memory::{LocalStore, AccessControlledStore, AccessLevel};
//! use std::sync::Arc;
//!
//! # async fn example() -> commander_memory::Result<()> {
//! let store = Arc::new(LocalStore::default().await?);
//!
//! // Session agent with isolated access
//! let session_store = AccessControlledStore::new(
//!     store.clone(),
//!     "session-agent-1".to_string(),
//!     AccessLevel::Own,
//! );
//!
//! // User agent with privileged access
//! let user_store = AccessControlledStore::new(
//!     store,
//!     "user-agent".to_string(),
//!     AccessLevel::All,
//! );
//! # Ok(())
//! # }
//! ```

pub mod embedding;
pub mod error;
pub mod local;
pub mod memory;
pub mod qdrant;
pub mod store;

// Re-export commonly used items
pub use embedding::{cosine_similarity, EmbeddingGenerator, EmbeddingProvider};
pub use error::{MemoryError, Result};
pub use local::LocalStore;
pub use memory::{Memory, SearchResult, DEFAULT_EMBEDDING_DIM};
pub use qdrant::QdrantStore;
pub use store::{AccessControlledStore, AccessLevel, MemoryStore};

/// Create the default memory store.
///
/// This creates a LocalStore using the default Commander data directory.
/// For production with larger collections, use QdrantStore instead.
pub async fn create_default_store() -> Result<LocalStore> {
    LocalStore::default().await
}

/// Create an embedding generator from environment.
///
/// Checks for API keys in order: OPENAI_API_KEY, OPENROUTER_API_KEY.
/// Falls back to hash-based embeddings if no key is found.
pub fn create_embedder() -> EmbeddingGenerator {
    EmbeddingGenerator::from_env()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_embedder() {
        let embedder = create_embedder();
        // Should work regardless of API keys
        assert!(embedder.dimension() > 0);
    }

    #[tokio::test]
    async fn test_full_workflow() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let store = LocalStore::new(temp_dir.path().to_path_buf()).await.unwrap();
        let embedder =
            EmbeddingGenerator::new(EmbeddingProvider::HashBased { dimension: 64 });

        // Store some memories
        let content1 = "The sky is blue during clear days";
        let content2 = "Grass is green in spring";
        let content3 = "The ocean is blue like the sky";

        let e1 = embedder.embed(content1).await.unwrap();
        let e2 = embedder.embed(content2).await.unwrap();
        let e3 = embedder.embed(content3).await.unwrap();

        store
            .store(Memory::new("agent-1", content1, e1))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-1", content2, e2))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-1", content3, e3))
            .await
            .unwrap();

        // Search for "blue"
        let query_embedding = embedder.embed("blue sky").await.unwrap();
        let results = store.search(&query_embedding, "agent-1", 2).await.unwrap();

        assert_eq!(results.len(), 2);
        // Top results should be about blue things (sky or ocean)
        // Note: Hash-based embeddings won't give semantic results,
        // but the API works correctly
    }

    #[tokio::test]
    async fn test_agent_isolation() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let store = LocalStore::new(temp_dir.path().to_path_buf()).await.unwrap();

        let embedding = vec![0.5; 64];

        store
            .store(Memory::new("agent-1", "secret for agent 1", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::new("agent-2", "secret for agent 2", embedding.clone()))
            .await
            .unwrap();

        // Agent 1 should only see its own memory
        let results1 = store.search(&embedding, "agent-1", 10).await.unwrap();
        assert_eq!(results1.len(), 1);
        assert_eq!(results1[0].memory.content, "secret for agent 1");

        // Agent 2 should only see its own memory
        let results2 = store.search(&embedding, "agent-2", 10).await.unwrap();
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].memory.content, "secret for agent 2");

        // Search all should see both
        let all = store.search_all(&embedding, 10).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_access_controlled_store_integration() {
        use std::sync::Arc;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let store = Arc::new(LocalStore::new(temp_dir.path().to_path_buf()).await.unwrap());

        let embedding = vec![0.5; 64];

        // Store memories for different agents using the raw store
        store
            .store(Memory::with_id("mem-1", "session-1", "session 1 secret", embedding.clone()))
            .await
            .unwrap();
        store
            .store(Memory::with_id("mem-2", "session-2", "session 2 secret", embedding.clone()))
            .await
            .unwrap();

        // Create access-controlled store for session agent (isolated)
        let session_store = AccessControlledStore::new(
            store.clone(),
            "session-1".to_string(),
            AccessLevel::Own,
        );

        // Session agent should only see its own memory
        let session_results = session_store.search(&embedding, 10).await.unwrap();
        assert_eq!(session_results.len(), 1);
        assert_eq!(session_results[0].memory.content, "session 1 secret");

        // Session agent cannot access other agent's memory by ID
        let blocked = session_store.get("mem-2").await.unwrap();
        assert!(blocked.is_none());

        // Create access-controlled store for user agent (privileged)
        let user_store = AccessControlledStore::new(
            store.clone(),
            "user-agent".to_string(),
            AccessLevel::All,
        );

        // User agent should see all memories
        let user_results = user_store.search(&embedding, 10).await.unwrap();
        assert_eq!(user_results.len(), 2);

        // User agent can access any memory by ID
        let allowed = user_store.get("mem-2").await.unwrap();
        assert!(allowed.is_some());
    }
}
