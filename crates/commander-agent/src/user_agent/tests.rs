//! Tests for the User Agent module.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use commander_memory::{Memory, MemoryStore, SearchResult};

use crate::client::OpenRouterClient;
use crate::completion_driver::{BlockerType, CompletionDriver};
use crate::context::AgentContext;
use crate::error::AgentError;
use commander_memory::EmbeddingGenerator;

use super::tools::{default_tools, format_search_results};
use super::UserAgent;

/// Mock memory store for testing.
pub(crate) struct MockMemoryStore {
    memories: RwLock<Vec<Memory>>,
}

impl MockMemoryStore {
    pub fn new() -> Self {
        Self {
            memories: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl MemoryStore for MockMemoryStore {
    async fn store(&self, memory: Memory) -> commander_memory::Result<()> {
        let mut memories = self.memories.write().await;
        memories.push(memory);
        Ok(())
    }

    async fn search(
        &self,
        _query_embedding: &[f32],
        agent_id: &str,
        limit: usize,
    ) -> commander_memory::Result<Vec<SearchResult>> {
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
    ) -> commander_memory::Result<Vec<SearchResult>> {
        let memories = self.memories.read().await;
        Ok(memories
            .iter()
            .take(limit)
            .map(|m| SearchResult::new(m.clone(), 0.9))
            .collect())
    }

    async fn delete(&self, id: &str) -> commander_memory::Result<()> {
        let mut memories = self.memories.write().await;
        memories.retain(|m| m.id != id);
        Ok(())
    }

    async fn get(&self, id: &str) -> commander_memory::Result<Option<Memory>> {
        let memories = self.memories.read().await;
        Ok(memories.iter().find(|m| m.id == id).cloned())
    }

    async fn list(&self, agent_id: &str, limit: usize) -> commander_memory::Result<Vec<Memory>> {
        let memories = self.memories.read().await;
        Ok(memories
            .iter()
            .filter(|m| m.agent_id == agent_id)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn count(&self, agent_id: &str) -> commander_memory::Result<usize> {
        let memories = self.memories.read().await;
        Ok(memories.iter().filter(|m| m.agent_id == agent_id).count())
    }

    async fn clear_agent(&self, agent_id: &str) -> commander_memory::Result<()> {
        let mut memories = self.memories.write().await;
        memories.retain(|m| m.agent_id != agent_id);
        Ok(())
    }
}

/// Helper to create a UserAgent struct for testing helper methods.
/// This avoids needing a real API key.
fn create_test_agent_struct() -> UserAgent {
    UserAgent {
        id: "test-user-agent".to_string(),
        config: UserAgent::default_config(),
        memory: Arc::new(MockMemoryStore::new()),
        embedder: EmbeddingGenerator::from_env(),
        tools: default_tools(),
        client: OpenRouterClient::new("fake-key-for-testing"),
        context: AgentContext::new(),
        completion_driver: None,
    }
}

#[test]
fn test_default_config() {
    let config = UserAgent::default_config();
    assert_eq!(config.model, "anthropic/claude-opus-4");
    assert_eq!(config.max_tokens, 4096);
    assert_eq!(config.temperature, 0.7);
}

#[test]
fn test_default_tools() {
    let tools = default_tools();
    assert_eq!(tools.len(), 4);

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"search_all_memories"));
    assert!(tool_names.contains(&"search_memories"));
    assert!(tool_names.contains(&"delegate_to_session"));
    assert!(tool_names.contains(&"get_session_status"));
}

#[test]
fn test_format_search_results_empty() {
    let results: Vec<SearchResult> = vec![];
    let output = format_search_results(&results);
    assert_eq!(output, "No relevant memories found.");
}

#[test]
fn test_format_search_results() {
    let memory = Memory::new("agent-1", "Test memory content", vec![0.1; 64]);
    let results = vec![SearchResult::new(memory, 0.95)];

    let output = format_search_results(&results);
    assert!(output.contains("Found 1 relevant memories"));
    assert!(output.contains("Test memory content"));
    assert!(output.contains("0.95"));
}

#[test]
fn test_user_agent_id() {
    // We can't create a full UserAgent without API key, but we can test the default_tools
    let tools = default_tools();
    assert!(!tools.is_empty());
}

#[tokio::test]
async fn test_mock_memory_store() {
    let store = MockMemoryStore::new();
    let memory = Memory::new("test-agent", "Test content", vec![0.1; 64]);

    store.store(memory).await.unwrap();

    let count = store.count("test-agent").await.unwrap();
    assert_eq!(count, 1);

    let results = store.search_all(&[0.1; 64], 10).await.unwrap();
    assert_eq!(results.len(), 1);
}

// ==================== Autonomous Behavior Tests ====================

#[test]
fn test_extract_blocker_reason() {
    // Create a minimal agent for testing helper methods
    let agent = create_test_agent_struct();

    // Test [BLOCKED] marker extraction
    let content = "[BLOCKED] Need database credentials to proceed";
    let reason = agent.extract_blocker_reason(content);
    assert!(reason.contains("credentials") || reason.contains("database"));

    // Test "need" phrase extraction
    let content = "I need your input on which approach to use";
    let reason = agent.extract_blocker_reason(content);
    assert!(reason.contains("need"));

    // Test fallback
    let content = "Something without clear markers";
    let reason = agent.extract_blocker_reason(content);
    assert_eq!(reason, "User input needed to proceed");
}

#[test]
fn test_classify_blocker_type() {
    let agent = create_test_agent_struct();

    assert_eq!(
        agent.classify_blocker_type("Please make a decision"),
        BlockerType::DecisionNeeded
    );
    assert_eq!(
        agent.classify_blocker_type("I need an API key"),
        BlockerType::ExternalDependency
    );
    assert_eq!(
        agent.classify_blocker_type("An error occurred"),
        BlockerType::ErrorRequiresJudgment
    );
    assert_eq!(
        agent.classify_blocker_type("The requirements are unclear"),
        BlockerType::AmbiguousRequirements
    );
    assert_eq!(
        agent.classify_blocker_type("I need some details"),
        BlockerType::InformationNeeded
    );
}

#[test]
fn test_extract_options() {
    let agent = create_test_agent_struct();

    let content = r#"Options:
1. Use approach A
2. Use approach B
3. Skip this step"#;
    let options = agent.extract_options(content);
    assert_eq!(options.len(), 3);
    assert_eq!(options[0], "Use approach A");
    assert_eq!(options[1], "Use approach B");
    assert_eq!(options[2], "Skip this step");

    // Test with parentheses style
    let content = r#"1) First option
2) Second option"#;
    let options = agent.extract_options(content);
    assert_eq!(options.len(), 2);
}

#[test]
fn test_classify_error_as_blocker() {
    let agent = create_test_agent_struct();

    // Configuration error should create a blocker
    let err = AgentError::Configuration("Missing API key".to_string());
    let blocker = agent.classify_error_as_blocker(&err);
    assert!(blocker.is_some());
    assert_eq!(
        blocker.unwrap().blocker_type,
        BlockerType::ExternalDependency
    );

    // Max iterations should create a blocker
    let err = AgentError::MaxIterationsExceeded(10);
    let blocker = agent.classify_error_as_blocker(&err);
    assert!(blocker.is_some());

    // Tool not found error should create a blocker
    let err = AgentError::ToolExecution {
        tool_name: "test".to_string(),
        message: "file not found".to_string(),
    };
    let blocker = agent.classify_error_as_blocker(&err);
    assert!(blocker.is_some());

    // Generic model invocation error should not create a blocker (recoverable)
    let err = AgentError::ModelInvocation("temporary failure".to_string());
    let blocker = agent.classify_error_as_blocker(&err);
    assert!(blocker.is_none());
}

#[test]
fn test_completion_driver_accessors() {
    let mut agent = create_test_agent_struct();

    // Initially no driver
    assert!(agent.completion_driver().is_none());

    // Set a driver
    let driver = CompletionDriver::new();
    agent.set_completion_driver(driver);
    assert!(agent.completion_driver().is_some());

    // Clear the driver
    agent.clear_completion_driver();
    assert!(agent.completion_driver().is_none());
}
