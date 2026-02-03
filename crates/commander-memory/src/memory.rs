//! Memory data model for agent memory storage.
//!
//! The `Memory` struct represents a single piece of stored information
//! with its vector embedding for semantic search.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default embedding dimension for OpenAI text-embedding-3-small.
pub const DEFAULT_EMBEDDING_DIM: usize = 1536;

/// A single memory entry stored in the vector database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// Unique identifier for this memory.
    pub id: String,

    /// Agent ID for isolation (memories are scoped to agents).
    pub agent_id: String,

    /// Original text content of the memory.
    pub content: String,

    /// Vector embedding for semantic search.
    /// Typically 1536 dimensions for OpenAI text-embedding-3-small.
    pub embedding: Vec<f32>,

    /// Additional metadata stored with the memory.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,

    /// Timestamp when this memory was created.
    pub created_at: DateTime<Utc>,
}

impl Memory {
    /// Create a new memory with generated ID and timestamp.
    ///
    /// # Arguments
    /// * `agent_id` - The agent this memory belongs to
    /// * `content` - The text content to store
    /// * `embedding` - The vector embedding for semantic search
    pub fn new(agent_id: impl Into<String>, content: impl Into<String>, embedding: Vec<f32>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            content: content.into(),
            embedding,
            metadata: HashMap::new(),
            created_at: Utc::now(),
        }
    }

    /// Create a new memory with a specific ID.
    pub fn with_id(
        id: impl Into<String>,
        agent_id: impl Into<String>,
        content: impl Into<String>,
        embedding: Vec<f32>,
    ) -> Self {
        Self {
            id: id.into(),
            agent_id: agent_id.into(),
            content: content.into(),
            embedding,
            metadata: HashMap::new(),
            created_at: Utc::now(),
        }
    }

    /// Add metadata to the memory.
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Get a metadata value by key.
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }
}

/// A memory search result with relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The matched memory.
    pub memory: Memory,

    /// Similarity score (0.0 to 1.0, higher is more similar).
    pub score: f32,
}

impl SearchResult {
    /// Create a new search result.
    pub fn new(memory: Memory, score: f32) -> Self {
        Self { memory, score }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_new() {
        let embedding = vec![0.1; DEFAULT_EMBEDDING_DIM];
        let memory = Memory::new("agent-1", "test content", embedding.clone());

        assert!(!memory.id.is_empty());
        assert_eq!(memory.agent_id, "agent-1");
        assert_eq!(memory.content, "test content");
        assert_eq!(memory.embedding.len(), DEFAULT_EMBEDDING_DIM);
        assert!(memory.metadata.is_empty());
    }

    #[test]
    fn test_memory_with_id() {
        let embedding = vec![0.1; 10];
        let memory = Memory::with_id("custom-id", "agent-1", "content", embedding);

        assert_eq!(memory.id, "custom-id");
    }

    #[test]
    fn test_memory_with_metadata() {
        let embedding = vec![0.1; 10];
        let memory = Memory::new("agent-1", "content", embedding)
            .with_metadata("source", serde_json::json!("test"))
            .with_metadata("count", serde_json::json!(42));

        assert_eq!(
            memory.get_metadata("source"),
            Some(&serde_json::json!("test"))
        );
        assert_eq!(memory.get_metadata("count"), Some(&serde_json::json!(42)));
        assert_eq!(memory.get_metadata("missing"), None);
    }

    #[test]
    fn test_search_result() {
        let embedding = vec![0.1; 10];
        let memory = Memory::new("agent-1", "content", embedding);
        let result = SearchResult::new(memory.clone(), 0.95);

        assert_eq!(result.score, 0.95);
        assert_eq!(result.memory.id, memory.id);
    }
}
