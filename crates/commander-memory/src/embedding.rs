//! Embedding generation for semantic search.
//!
//! Supports multiple embedding providers with fallback to hash-based embeddings
//! when no API key is available (useful for testing).

use crate::error::{MemoryError, Result};
use crate::memory::DEFAULT_EMBEDDING_DIM;
use tracing::{debug, warn};

/// Environment variable for OpenAI API key.
pub const OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";

/// Environment variable for OpenRouter API key (fallback).
pub const OPENROUTER_API_KEY_ENV: &str = "OPENROUTER_API_KEY";

/// Default embedding model.
pub const DEFAULT_MODEL: &str = "text-embedding-3-small";

/// OpenAI embedding API endpoint.
const OPENAI_API_URL: &str = "https://api.openai.com/v1/embeddings";

/// OpenRouter embedding API endpoint.
const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/embeddings";

/// Embedding provider configuration.
#[derive(Debug, Clone)]
pub enum EmbeddingProvider {
    /// Use OpenAI API directly.
    OpenAI { api_key: String, model: String },
    /// Use OpenRouter API.
    OpenRouter { api_key: String, model: String },
    /// Use hash-based fake embeddings (for testing only).
    HashBased { dimension: usize },
}

impl EmbeddingProvider {
    /// Create provider from environment variables.
    ///
    /// Priority:
    /// 1. OPENAI_API_KEY -> OpenAI
    /// 2. OPENROUTER_API_KEY -> OpenRouter
    /// 3. None -> HashBased fallback
    pub fn from_env() -> Self {
        if let Ok(api_key) = std::env::var(OPENAI_API_KEY_ENV) {
            debug!("Using OpenAI embedding provider");
            return Self::OpenAI {
                api_key,
                model: DEFAULT_MODEL.to_string(),
            };
        }

        if let Ok(api_key) = std::env::var(OPENROUTER_API_KEY_ENV) {
            debug!("Using OpenRouter embedding provider");
            return Self::OpenRouter {
                api_key,
                model: format!("openai/{}", DEFAULT_MODEL),
            };
        }

        warn!("No embedding API key found, using hash-based fallback");
        Self::HashBased {
            dimension: DEFAULT_EMBEDDING_DIM,
        }
    }

    /// Check if this provider uses real embeddings (API-based).
    pub fn is_real(&self) -> bool {
        !matches!(self, Self::HashBased { .. })
    }

    /// Get the embedding dimension for this provider.
    pub fn dimension(&self) -> usize {
        match self {
            Self::OpenAI { .. } | Self::OpenRouter { .. } => DEFAULT_EMBEDDING_DIM,
            Self::HashBased { dimension } => *dimension,
        }
    }
}

/// Generate embeddings for text content.
#[derive(Clone)]
pub struct EmbeddingGenerator {
    provider: EmbeddingProvider,
    client: reqwest::Client,
}

impl EmbeddingGenerator {
    /// Create a new embedding generator with the given provider.
    pub fn new(provider: EmbeddingProvider) -> Self {
        Self {
            provider,
            client: reqwest::Client::new(),
        }
    }

    /// Create a generator from environment variables.
    pub fn from_env() -> Self {
        Self::new(EmbeddingProvider::from_env())
    }

    /// Check if using real embeddings (not hash-based).
    pub fn is_real(&self) -> bool {
        self.provider.is_real()
    }

    /// Get the embedding dimension.
    pub fn dimension(&self) -> usize {
        self.provider.dimension()
    }

    /// Generate an embedding for the given text.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        match &self.provider {
            EmbeddingProvider::OpenAI { api_key, model } => {
                self.embed_openai(text, api_key, model).await
            }
            EmbeddingProvider::OpenRouter { api_key, model } => {
                self.embed_openrouter(text, api_key, model).await
            }
            EmbeddingProvider::HashBased { dimension } => Ok(hash_based_embedding(text, *dimension)),
        }
    }

    /// Generate embeddings for multiple texts in a batch.
    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        match &self.provider {
            EmbeddingProvider::OpenAI { api_key, model } => {
                self.embed_batch_openai(texts, api_key, model).await
            }
            EmbeddingProvider::OpenRouter { api_key, model } => {
                self.embed_batch_openrouter(texts, api_key, model).await
            }
            EmbeddingProvider::HashBased { dimension } => Ok(texts
                .iter()
                .map(|t| hash_based_embedding(t, *dimension))
                .collect()),
        }
    }

    async fn embed_openai(&self, text: &str, api_key: &str, model: &str) -> Result<Vec<f32>> {
        let response = self
            .client
            .post(OPENAI_API_URL)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "input": text
            }))
            .send()
            .await
            .map_err(|e| MemoryError::EmbeddingError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(MemoryError::EmbeddingError(format!(
                "OpenAI API error {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| MemoryError::EmbeddingError(e.to_string()))?;

        parse_embedding_response(&json)
    }

    async fn embed_openrouter(&self, text: &str, api_key: &str, model: &str) -> Result<Vec<f32>> {
        let response = self
            .client
            .post(OPENROUTER_API_URL)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "input": text
            }))
            .send()
            .await
            .map_err(|e| MemoryError::EmbeddingError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(MemoryError::EmbeddingError(format!(
                "OpenRouter API error {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| MemoryError::EmbeddingError(e.to_string()))?;

        parse_embedding_response(&json)
    }

    async fn embed_batch_openai(
        &self,
        texts: &[&str],
        api_key: &str,
        model: &str,
    ) -> Result<Vec<Vec<f32>>> {
        let response = self
            .client
            .post(OPENAI_API_URL)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "input": texts
            }))
            .send()
            .await
            .map_err(|e| MemoryError::EmbeddingError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(MemoryError::EmbeddingError(format!(
                "OpenAI API error {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| MemoryError::EmbeddingError(e.to_string()))?;

        parse_batch_embedding_response(&json)
    }

    async fn embed_batch_openrouter(
        &self,
        texts: &[&str],
        api_key: &str,
        model: &str,
    ) -> Result<Vec<Vec<f32>>> {
        let response = self
            .client
            .post(OPENROUTER_API_URL)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "input": texts
            }))
            .send()
            .await
            .map_err(|e| MemoryError::EmbeddingError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(MemoryError::EmbeddingError(format!(
                "OpenRouter API error {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| MemoryError::EmbeddingError(e.to_string()))?;

        parse_batch_embedding_response(&json)
    }
}

fn parse_embedding_response(json: &serde_json::Value) -> Result<Vec<f32>> {
    let embedding = json["data"][0]["embedding"]
        .as_array()
        .ok_or_else(|| MemoryError::EmbeddingError("Invalid response format".to_string()))?;

    embedding
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| MemoryError::EmbeddingError("Invalid embedding value".to_string()))
        })
        .collect()
}

fn parse_batch_embedding_response(json: &serde_json::Value) -> Result<Vec<Vec<f32>>> {
    let data = json["data"]
        .as_array()
        .ok_or_else(|| MemoryError::EmbeddingError("Invalid response format".to_string()))?;

    data.iter()
        .map(|item| {
            let embedding = item["embedding"]
                .as_array()
                .ok_or_else(|| MemoryError::EmbeddingError("Invalid embedding format".to_string()))?;

            embedding
                .iter()
                .map(|v| {
                    v.as_f64().map(|f| f as f32).ok_or_else(|| {
                        MemoryError::EmbeddingError("Invalid embedding value".to_string())
                    })
                })
                .collect()
        })
        .collect()
}

/// Generate a hash-based fake embedding for testing.
///
/// This creates a deterministic embedding based on the hash of the input text.
/// NOT suitable for production - use only for testing when no API key is available.
fn hash_based_embedding(text: &str, dimension: usize) -> Vec<f32> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut embedding = Vec::with_capacity(dimension);
    let mut hasher = DefaultHasher::new();

    for i in 0..dimension {
        // Hash the text with the index to get different values
        text.hash(&mut hasher);
        i.hash(&mut hasher);
        let hash = hasher.finish();

        // Convert to float in range [-1, 1]
        let value = ((hash as f64 / u64::MAX as f64) * 2.0 - 1.0) as f32;
        embedding.push(value);

        // Reset hasher for next iteration
        hasher = DefaultHasher::new();
    }

    // Normalize to unit vector
    let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if magnitude > 0.0 {
        for x in &mut embedding {
            *x /= magnitude;
        }
    }

    embedding
}

/// Calculate cosine similarity between two embeddings.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    dot / (mag_a * mag_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_based_embedding_deterministic() {
        let e1 = hash_based_embedding("test text", 10);
        let e2 = hash_based_embedding("test text", 10);
        assert_eq!(e1, e2);
    }

    #[test]
    fn test_hash_based_embedding_different_texts() {
        let e1 = hash_based_embedding("hello", 10);
        let e2 = hash_based_embedding("world", 10);
        assert_ne!(e1, e2);
    }

    #[test]
    fn test_hash_based_embedding_normalized() {
        let embedding = hash_based_embedding("test", 100);
        let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((magnitude - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_same() {
        let e = vec![0.5, 0.5, 0.5, 0.5];
        assert!((cosine_similarity(&e, &e) - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &b).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_provider_from_env_fallback() {
        // Without env vars, should fall back to hash-based
        std::env::remove_var(OPENAI_API_KEY_ENV);
        std::env::remove_var(OPENROUTER_API_KEY_ENV);
        let provider = EmbeddingProvider::from_env();
        assert!(!provider.is_real());
    }

    #[test]
    fn test_embedding_generator_hash_based() {
        let gen = EmbeddingGenerator::new(EmbeddingProvider::HashBased { dimension: 128 });
        assert!(!gen.is_real());
        assert_eq!(gen.dimension(), 128);
    }

    #[tokio::test]
    async fn test_hash_based_embed() {
        let gen = EmbeddingGenerator::new(EmbeddingProvider::HashBased { dimension: 64 });
        let embedding = gen.embed("test content").await.unwrap();
        assert_eq!(embedding.len(), 64);
    }

    #[tokio::test]
    async fn test_hash_based_embed_batch() {
        let gen = EmbeddingGenerator::new(EmbeddingProvider::HashBased { dimension: 32 });
        let embeddings = gen.embed_batch(&["text1", "text2", "text3"]).await.unwrap();
        assert_eq!(embeddings.len(), 3);
        for e in embeddings {
            assert_eq!(e.len(), 32);
        }
    }
}
