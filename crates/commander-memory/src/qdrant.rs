//! Qdrant vector database backend for memory storage.
//!
//! This module provides a Qdrant-based implementation of the MemoryStore trait.
//! Requires a running Qdrant server (local or remote).
//!
//! For local development without a server, see the `local` module which provides
//! a file-based implementation.

use async_trait::async_trait;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter, GetPointsBuilder,
    PointStruct, ScrollPointsBuilder, SearchPointsBuilder, UpsertPointsBuilder, Value,
    VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use std::collections::HashMap;
use tracing::{debug, info};

use crate::error::{MemoryError, Result};
use crate::memory::{Memory, SearchResult, DEFAULT_EMBEDDING_DIM};
use crate::store::MemoryStore;

/// Default collection name for memories.
const COLLECTION_NAME: &str = "memories";

/// Payload field names.
const FIELD_AGENT_ID: &str = "agent_id";
const FIELD_CONTENT: &str = "content";
const FIELD_METADATA: &str = "metadata";
const FIELD_CREATED_AT: &str = "created_at";

/// Qdrant-based memory store.
///
/// Requires a running Qdrant server. For configuration:
/// - Set QDRANT_URL env var (default: http://localhost:6334)
/// - Optionally set QDRANT_API_KEY for authentication
pub struct QdrantStore {
    client: Qdrant,
    collection: String,
    dimension: usize,
}

impl QdrantStore {
    /// Create a new Qdrant store connecting to the specified URL.
    pub async fn new(url: &str, api_key: Option<&str>) -> Result<Self> {
        let mut builder = Qdrant::from_url(url);
        if let Some(key) = api_key {
            builder = builder.api_key(key);
        }

        let client = builder
            .build()
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        let store = Self {
            client,
            collection: COLLECTION_NAME.to_string(),
            dimension: DEFAULT_EMBEDDING_DIM,
        };

        store.ensure_collection().await?;
        Ok(store)
    }

    /// Create a store from environment variables.
    ///
    /// Uses:
    /// - QDRANT_URL (default: http://localhost:6334)
    /// - QDRANT_API_KEY (optional)
    pub async fn from_env() -> Result<Self> {
        let url =
            std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
        let api_key = std::env::var("QDRANT_API_KEY").ok();
        Self::new(&url, api_key.as_deref()).await
    }

    /// Create a store with custom collection name and dimension.
    pub async fn with_config(
        url: &str,
        api_key: Option<&str>,
        collection: &str,
        dimension: usize,
    ) -> Result<Self> {
        let mut builder = Qdrant::from_url(url);
        if let Some(key) = api_key {
            builder = builder.api_key(key);
        }

        let client = builder
            .build()
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        let store = Self {
            client,
            collection: collection.to_string(),
            dimension,
        };

        store.ensure_collection().await?;
        Ok(store)
    }

    async fn ensure_collection(&self) -> Result<()> {
        // Check if collection exists
        let collections = self
            .client
            .list_collections()
            .await
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        let exists = collections
            .collections
            .iter()
            .any(|c| c.name == self.collection);

        if !exists {
            info!(collection = %self.collection, "Creating Qdrant collection");
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection).vectors_config(
                        VectorParamsBuilder::new(self.dimension as u64, Distance::Cosine),
                    ),
                )
                .await
                .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;
        }

        Ok(())
    }

    fn memory_to_point(&self, memory: &Memory) -> PointStruct {
        let mut payload: HashMap<String, Value> = HashMap::new();
        payload.insert(FIELD_AGENT_ID.to_string(), memory.agent_id.clone().into());
        payload.insert(FIELD_CONTENT.to_string(), memory.content.clone().into());
        payload.insert(
            FIELD_CREATED_AT.to_string(),
            memory.created_at.to_rfc3339().into(),
        );
        payload.insert(
            FIELD_METADATA.to_string(),
            serde_json::to_string(&memory.metadata)
                .unwrap_or_default()
                .into(),
        );

        PointStruct::new(memory.id.clone(), memory.embedding.clone(), payload)
    }

    fn point_to_memory(&self, point: &qdrant_client::qdrant::ScoredPoint) -> Option<Memory> {
        let payload = &point.payload;
        let id = point.id.as_ref()?.point_id_options.as_ref()?;
        let id_str = match id {
            qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u) => u.clone(),
            qdrant_client::qdrant::point_id::PointIdOptions::Num(n) => n.to_string(),
        };

        let agent_id = payload
            .get(FIELD_AGENT_ID)?
            .as_str()
            .map(|s| s.to_string())?;
        let content = payload
            .get(FIELD_CONTENT)?
            .as_str()
            .map(|s| s.to_string())?;
        let created_at_str = payload.get(FIELD_CREATED_AT)?.as_str()?;
        let created_at = chrono::DateTime::parse_from_rfc3339(created_at_str)
            .ok()?
            .with_timezone(&chrono::Utc);

        let metadata: HashMap<String, serde_json::Value> = payload
            .get(FIELD_METADATA)
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        // Extract embedding from vectors - handle the VectorsOutput type
        let embedding = point
            .vectors
            .as_ref()
            .and_then(|v| {
                // VectorsOutput has vectors_options field
                match &v.vectors_options {
                    Some(qdrant_client::qdrant::vectors_output::VectorsOptions::Vector(vec)) => {
                        #[allow(deprecated)]
                        Some(vec.data.clone())
                    }
                    _ => None,
                }
            })
            .unwrap_or_default();

        Some(Memory {
            id: id_str,
            agent_id,
            content,
            embedding,
            metadata,
            created_at,
        })
    }

    fn retrieved_to_memory(&self, point: &qdrant_client::qdrant::RetrievedPoint) -> Option<Memory> {
        let payload = &point.payload;
        let id = point.id.as_ref()?.point_id_options.as_ref()?;
        let id_str = match id {
            qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u) => u.clone(),
            qdrant_client::qdrant::point_id::PointIdOptions::Num(n) => n.to_string(),
        };

        let agent_id = payload
            .get(FIELD_AGENT_ID)?
            .as_str()
            .map(|s| s.to_string())?;
        let content = payload
            .get(FIELD_CONTENT)?
            .as_str()
            .map(|s| s.to_string())?;
        let created_at_str = payload.get(FIELD_CREATED_AT)?.as_str()?;
        let created_at = chrono::DateTime::parse_from_rfc3339(created_at_str)
            .ok()?
            .with_timezone(&chrono::Utc);

        let metadata: HashMap<String, serde_json::Value> = payload
            .get(FIELD_METADATA)
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        let embedding = point
            .vectors
            .as_ref()
            .and_then(|v| match &v.vectors_options {
                Some(qdrant_client::qdrant::vectors_output::VectorsOptions::Vector(vec)) => {
                    #[allow(deprecated)]
                    Some(vec.data.clone())
                }
                _ => None,
            })
            .unwrap_or_default();

        Some(Memory {
            id: id_str,
            agent_id,
            content,
            embedding,
            metadata,
            created_at,
        })
    }

    fn make_agent_filter(&self, agent_id: &str) -> Filter {
        Filter::must([Condition::matches(FIELD_AGENT_ID, agent_id.to_string())])
    }
}

#[async_trait]
impl MemoryStore for QdrantStore {
    async fn store(&self, memory: Memory) -> Result<()> {
        let point = self.memory_to_point(&memory);
        debug!(id = %memory.id, agent_id = %memory.agent_id, "Storing memory");

        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]))
            .await
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    async fn search(
        &self,
        query_embedding: &[f32],
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let filter = self.make_agent_filter(agent_id);

        let results = self
            .client
            .search_points(
                SearchPointsBuilder::new(&self.collection, query_embedding.to_vec(), limit as u64)
                    .filter(filter)
                    .with_payload(true)
                    .with_vectors(true),
            )
            .await
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        Ok(results
            .result
            .iter()
            .filter_map(|point| {
                self.point_to_memory(point)
                    .map(|m| SearchResult::new(m, point.score))
            })
            .collect())
    }

    async fn search_all(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        let results = self
            .client
            .search_points(
                SearchPointsBuilder::new(&self.collection, query_embedding.to_vec(), limit as u64)
                    .with_payload(true)
                    .with_vectors(true),
            )
            .await
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        Ok(results
            .result
            .iter()
            .filter_map(|point| {
                self.point_to_memory(point)
                    .map(|m| SearchResult::new(m, point.score))
            })
            .collect())
    }

    async fn delete(&self, id: &str) -> Result<()> {
        use qdrant_client::qdrant::PointsIdsList;

        let points_selector = PointsIdsList {
            ids: vec![id.into()],
        };

        self.client
            .delete_points(DeletePointsBuilder::new(&self.collection).points(points_selector))
            .await
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Memory>> {
        let result = self
            .client
            .get_points(
                GetPointsBuilder::new(&self.collection, vec![id.into()])
                    .with_payload(true)
                    .with_vectors(true),
            )
            .await
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        if let Some(point) = result.result.first() {
            return Ok(self.retrieved_to_memory(point));
        }

        Ok(None)
    }

    async fn list(&self, agent_id: &str, limit: usize) -> Result<Vec<Memory>> {
        let filter = self.make_agent_filter(agent_id);

        let result = self
            .client
            .scroll(
                ScrollPointsBuilder::new(&self.collection)
                    .filter(filter)
                    .limit(limit as u32)
                    .with_payload(true)
                    .with_vectors(true),
            )
            .await
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        Ok(result
            .result
            .iter()
            .filter_map(|point| self.retrieved_to_memory(point))
            .collect())
    }

    async fn count(&self, agent_id: &str) -> Result<usize> {
        use qdrant_client::qdrant::CountPointsBuilder;

        let filter = self.make_agent_filter(agent_id);

        let result = self
            .client
            .count(CountPointsBuilder::new(&self.collection).filter(filter))
            .await
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        Ok(result.result.map(|r| r.count as usize).unwrap_or(0))
    }

    async fn clear_agent(&self, agent_id: &str) -> Result<()> {
        let filter = self.make_agent_filter(agent_id);

        self.client
            .delete_points(DeletePointsBuilder::new(&self.collection).points(filter))
            .await
            .map_err(|e| MemoryError::DatabaseError(e.to_string()))?;

        Ok(())
    }
}

// Note: Integration tests require a running Qdrant server
// Run with: cargo test --features qdrant-tests -- --ignored
#[cfg(test)]
mod tests {
    #[test]
    fn test_collection_name() {
        assert_eq!(super::COLLECTION_NAME, "memories");
    }
}
