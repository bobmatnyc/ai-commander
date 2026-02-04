//! Tool definitions and execution for the User Agent.
//!
//! Contains the default tools available to the User Agent and their
//! execution implementations.

use serde_json::json;
use tracing::{debug, info};

use commander_memory::SearchResult;

use crate::error::{AgentError, Result};
use crate::tool::{ToolCall, ToolDefinition, ToolResult};

use super::UserAgent;

/// Get the default tools for User Agent.
pub(crate) fn default_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "search_all_memories",
            "Search across all agent memories for relevant information",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query to find relevant memories"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        ),
        ToolDefinition::new(
            "search_memories",
            "Search memories for a specific agent",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query to find relevant memories"
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "The agent ID to search within"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query", "agent_id"]
            }),
        ),
        ToolDefinition::new(
            "delegate_to_session",
            "Send a task to a session agent for execution",
            json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The session ID to delegate to"
                    },
                    "task": {
                        "type": "string",
                        "description": "The task description to execute"
                    },
                    "context": {
                        "type": "string",
                        "description": "Additional context for the task"
                    }
                },
                "required": ["session_id", "task"]
            }),
        ),
        ToolDefinition::new(
            "get_session_status",
            "Query the current status of a session agent",
            json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The session ID to query"
                    }
                },
                "required": ["session_id"]
            }),
        ),
    ]
}

/// Execute the search_all_memories tool.
pub(crate) async fn execute_search_all_memories(
    agent: &UserAgent,
    call: &ToolCall,
) -> Result<ToolResult> {
    let query = call.get_string_arg("query").map_err(|e| {
        AgentError::InvalidArguments {
            tool_name: call.name.clone(),
            message: e,
        }
    })?;

    let limit = call
        .get_arg("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;

    debug!("Searching all memories for: {} (limit: {})", query, limit);

    // Generate embedding for the query
    let embedding = agent
        .embedder
        .embed(query)
        .await
        .map_err(|e| AgentError::ToolExecution {
            tool_name: call.name.clone(),
            message: format!("Failed to generate embedding: {}", e),
        })?;

    // Search memories
    let results = agent
        .memory
        .search_all(&embedding, limit)
        .await
        .map_err(AgentError::Memory)?;

    let output = format_search_results(&results);
    Ok(ToolResult::success(&call.id, output))
}

/// Execute the search_memories tool.
pub(crate) async fn execute_search_memories(
    agent: &UserAgent,
    call: &ToolCall,
) -> Result<ToolResult> {
    let query = call.get_string_arg("query").map_err(|e| {
        AgentError::InvalidArguments {
            tool_name: call.name.clone(),
            message: e,
        }
    })?;

    let agent_id = call.get_string_arg("agent_id").map_err(|e| {
        AgentError::InvalidArguments {
            tool_name: call.name.clone(),
            message: e,
        }
    })?;

    let limit = call
        .get_arg("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;

    debug!(
        "Searching memories for agent '{}': {} (limit: {})",
        agent_id, query, limit
    );

    // Generate embedding for the query
    let embedding = agent
        .embedder
        .embed(query)
        .await
        .map_err(|e| AgentError::ToolExecution {
            tool_name: call.name.clone(),
            message: format!("Failed to generate embedding: {}", e),
        })?;

    // Search memories
    let results = agent
        .memory
        .search(&embedding, agent_id, limit)
        .await
        .map_err(AgentError::Memory)?;

    let output = format_search_results(&results);
    Ok(ToolResult::success(&call.id, output))
}

/// Execute the delegate_to_session tool (placeholder).
pub(crate) async fn execute_delegate_to_session(
    _agent: &UserAgent,
    call: &ToolCall,
) -> Result<ToolResult> {
    let session_id = call.get_string_arg("session_id").map_err(|e| {
        AgentError::InvalidArguments {
            tool_name: call.name.clone(),
            message: e,
        }
    })?;

    let task = call.get_string_arg("task").map_err(|e| {
        AgentError::InvalidArguments {
            tool_name: call.name.clone(),
            message: e,
        }
    })?;

    let context = call.get_optional_string_arg("context");

    info!("Delegating task to session '{}': {}", session_id, task);

    // Placeholder - will be implemented when session agent integration is complete
    let output = format!(
        "Task delegated to session '{}': {}\nContext: {}\n\nNote: Session agent integration is not yet implemented. This is a placeholder response.",
        session_id,
        task,
        context.unwrap_or("None")
    );

    Ok(ToolResult::success(&call.id, output))
}

/// Execute the get_session_status tool (placeholder).
pub(crate) async fn execute_get_session_status(
    _agent: &UserAgent,
    call: &ToolCall,
) -> Result<ToolResult> {
    let session_id = call.get_string_arg("session_id").map_err(|e| {
        AgentError::InvalidArguments {
            tool_name: call.name.clone(),
            message: e,
        }
    })?;

    debug!("Querying status of session '{}'", session_id);

    // Placeholder - will be implemented when session agent integration is complete
    let output = format!(
        "Session '{}' status:\n- State: Not implemented\n- Note: Session agent integration is not yet implemented. This is a placeholder response.",
        session_id
    );

    Ok(ToolResult::success(&call.id, output))
}

/// Format search results as a human-readable string.
pub(crate) fn format_search_results(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No relevant memories found.".to_string();
    }

    let mut output = format!("Found {} relevant memories:\n\n", results.len());

    for (i, result) in results.iter().enumerate() {
        output.push_str(&format!(
            "{}. [Score: {:.2}] {}\n   Agent: {}, Created: {}\n\n",
            i + 1,
            result.score,
            result.memory.content,
            result.memory.agent_id,
            result.memory.created_at.format("%Y-%m-%d %H:%M:%S")
        ));
    }

    output
}
