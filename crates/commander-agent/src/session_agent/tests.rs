//! Tests for SessionAgent module.

use super::*;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use commander_memory::{Memory, MemoryStore, SearchResult};

use crate::context_manager::{ContextAction, ContextManager, ContextStrategy, CriticalAction};
use crate::template::AgentTemplate;
use super::tools::format_search_results;

/// Mock memory store for testing.
pub struct MockMemoryStore {
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

#[test]
fn test_session_state_default() {
    let state = SessionState::new();

    assert!(state.goals.is_empty());
    assert!(state.current_task.is_none());
    assert_eq!(state.progress, 0.0);
    assert!(state.blockers.is_empty());
    assert!(state.files_modified.is_empty());
    assert!(state.last_output.is_none());
}

#[test]
fn test_session_state_updates() {
    let mut state = SessionState::new();

    state.add_goal("Implement feature X");
    assert_eq!(state.goals.len(), 1);

    state.set_current_task("Writing tests");
    assert_eq!(state.current_task, Some("Writing tests".to_string()));

    state.set_progress(0.5);
    assert_eq!(state.progress, 0.5);

    state.set_progress(1.5); // Should clamp
    assert_eq!(state.progress, 1.0);

    state.add_blocker("API error");
    assert_eq!(state.blockers.len(), 1);

    state.clear_blockers();
    assert!(state.blockers.is_empty());

    state.add_modified_file("src/main.rs");
    state.add_modified_file("src/main.rs"); // Duplicate - should not add
    assert_eq!(state.files_modified.len(), 1);
}

#[test]
fn test_output_analysis_default() {
    let analysis = OutputAnalysis::new();

    assert!(!analysis.detected_completion);
    assert!(!analysis.waiting_for_input);
    assert!(analysis.error_detected.is_none());
    assert!(analysis.files_changed.is_empty());
    assert!(analysis.summary.is_empty());
}

#[test]
fn test_output_analysis_with_summary() {
    let analysis = OutputAnalysis::with_summary("Task completed successfully");

    assert_eq!(analysis.summary, "Task completed successfully");
}

#[test]
fn test_default_config() {
    let template = AgentTemplate::generic();
    let config = SessionAgent::default_config(&template);

    assert_eq!(config.model, "anthropic/claude-haiku-4");
    assert_eq!(config.max_tokens, 2048);
    assert_eq!(config.temperature, 0.5);
}

#[test]
fn test_builtin_tools() {
    let tools = SessionAgent::builtin_tools();

    assert_eq!(tools.len(), 4);

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"search_memories"));
    assert!(tool_names.contains(&"update_session_state"));
    assert!(tool_names.contains(&"report_to_user"));
    assert!(tool_names.contains(&"analyze_output"));
}

#[test]
fn test_format_search_results_empty() {
    let results: Vec<SearchResult> = vec![];
    let output = format_search_results(&results);
    assert_eq!(output, "No relevant memories found.");
}

#[test]
fn test_format_search_results() {
    let memory = Memory::new("session-agent-1", "Test memory content", vec![0.1; 64]);
    let results = vec![SearchResult::new(memory, 0.95)];

    let output = format_search_results(&results);
    assert!(output.contains("Found 1 relevant memories"));
    assert!(output.contains("Test memory content"));
    assert!(output.contains("0.95"));
}

#[tokio::test]
async fn test_mock_memory_isolation() {
    let store = Arc::new(MockMemoryStore::new());

    // Store memories for different agents
    let memory1 = Memory::new("session-agent-1", "Memory for agent 1", vec![0.1; 64]);
    let memory2 = Memory::new("session-agent-2", "Memory for agent 2", vec![0.1; 64]);

    store.store(memory1).await.unwrap();
    store.store(memory2).await.unwrap();

    // Search should only return memories for the specified agent
    let results1 = store.search(&[0.1; 64], "session-agent-1", 10).await.unwrap();
    assert_eq!(results1.len(), 1);
    assert_eq!(results1[0].memory.agent_id, "session-agent-1");

    let results2 = store.search(&[0.1; 64], "session-agent-2", 10).await.unwrap();
    assert_eq!(results2.len(), 1);
    assert_eq!(results2[0].memory.agent_id, "session-agent-2");
}

// ==========================================================================
// Context Manager Tests
// ==========================================================================

#[test]
fn test_context_manager_initialization() {
    // Test that templates get the correct context strategy
    let claude_template = AgentTemplate::claude_code();
    assert!(matches!(
        claude_template.context_strategy,
        Some(ContextStrategy::Compaction)
    ));

    let mpm_template = AgentTemplate::mpm();
    assert!(matches!(
        mpm_template.context_strategy,
        Some(ContextStrategy::PauseResume { .. })
    ));

    let generic_template = AgentTemplate::generic();
    assert!(matches!(
        generic_template.context_strategy,
        Some(ContextStrategy::WarnAndContinue)
    ));
}

#[test]
fn test_context_manager_thresholds() {
    let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);

    // Test Continue (50% used = 50% remaining)
    let action = manager.update(50_000);
    assert!(matches!(action, ContextAction::Continue));

    // Test Warning (85% used = 15% remaining)
    let action = manager.update(85_000);
    assert!(matches!(action, ContextAction::Warn { .. }));

    // Test Critical (95% used = 5% remaining)
    let action = manager.update(95_000);
    assert!(matches!(action, ContextAction::Critical { .. }));
}

#[test]
fn test_context_manager_strategies() {
    // Test Compaction strategy
    let mut compaction_manager = ContextManager::new(ContextStrategy::Compaction, 100_000);
    let action = compaction_manager.update(95_000);
    match action {
        ContextAction::Critical { action } => {
            assert!(matches!(action, CriticalAction::Compact { .. }));
        }
        _ => panic!("Expected Critical action with Compact"),
    }

    // Test PauseResume strategy
    let mut pause_manager = ContextManager::new(
        ContextStrategy::PauseResume {
            pause_command: "/pause".to_string(),
            resume_command: "/resume".to_string(),
        },
        100_000,
    );
    let action = pause_manager.update(95_000);
    match action {
        ContextAction::Critical { action } => {
            assert!(matches!(action, CriticalAction::Pause { .. }));
        }
        _ => panic!("Expected Critical action with Pause"),
    }

    // Test WarnAndContinue strategy
    let mut warn_manager = ContextManager::new(ContextStrategy::WarnAndContinue, 100_000);
    let action = warn_manager.update(95_000);
    match action {
        ContextAction::Critical { action } => {
            assert!(matches!(action, CriticalAction::Alert { .. }));
        }
        _ => panic!("Expected Critical action with Alert"),
    }
}

#[test]
fn test_generate_pause_state() {
    let mut state = SessionState::new();
    state.add_goal("Implement feature X");
    state.set_current_task("Writing tests");
    state.set_progress(0.5);
    state.add_blocker("Waiting for API");
    state.add_modified_file("src/main.rs");

    // Create a minimal context manager to test state generation format
    let manager = ContextManager::new(ContextStrategy::PauseResume {
        pause_command: "/pause".to_string(),
        resume_command: "/resume".to_string(),
    }, 100_000);

    // The state should contain key fields
    let state_debug = format!("{:?}", state);
    assert!(state_debug.contains("Implement feature X"));
    assert!(state_debug.contains("Writing tests"));
    assert!(state_debug.contains("Waiting for API"));
    assert!(state_debug.contains("src/main.rs"));

    // Verify manager has correct strategy
    assert!(matches!(
        manager.strategy(),
        ContextStrategy::PauseResume { .. }
    ));
}

#[test]
fn test_context_manager_remaining_percent() {
    let mut manager = ContextManager::new(ContextStrategy::Compaction, 200_000);

    // Initial state: 0% used = 100% remaining
    assert!((manager.remaining_percent() - 1.0).abs() < 0.001);

    // 50% used
    manager.update(100_000);
    assert!((manager.remaining_percent() - 0.5).abs() < 0.001);

    // 90% used = 10% remaining (exactly at critical)
    manager.update(180_000);
    assert!((manager.remaining_percent() - 0.1).abs() < 0.001);
}
