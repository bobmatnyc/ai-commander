//! Tool definitions and execution methods for SessionAgent.

use serde_json::json;
use tracing::debug;

use commander_memory::SearchResult;

use crate::error::{AgentError, Result};
use crate::tool::{ToolCall, ToolDefinition, ToolResult};

use super::SessionAgent;

impl SessionAgent {
    /// Get the built-in tools for session agents.
    pub(super) fn builtin_tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition::new(
                "search_memories",
                "Search your own memories for relevant information (agent-isolated)",
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
                "update_session_state",
                "Update the session state (goals, progress, blockers)",
                json!({
                    "type": "object",
                    "properties": {
                        "add_goal": {
                            "type": "string",
                            "description": "Add a new goal to the session"
                        },
                        "current_task": {
                            "type": "string",
                            "description": "Set the current task being worked on"
                        },
                        "progress": {
                            "type": "number",
                            "description": "Update progress (0.0 to 1.0)",
                            "minimum": 0.0,
                            "maximum": 1.0
                        },
                        "add_blocker": {
                            "type": "string",
                            "description": "Add a blocker"
                        },
                        "clear_blockers": {
                            "type": "boolean",
                            "description": "Clear all blockers"
                        },
                        "add_modified_file": {
                            "type": "string",
                            "description": "Track a modified file"
                        }
                    }
                }),
            ),
            ToolDefinition::new(
                "report_to_user",
                "Send a status report to the User Agent (stored in memory)",
                json!({
                    "type": "object",
                    "properties": {
                        "summary": {
                            "type": "string",
                            "description": "Brief summary of current status"
                        },
                        "progress": {
                            "type": "number",
                            "description": "Progress indicator (0.0 to 1.0)"
                        },
                        "needs_input": {
                            "type": "boolean",
                            "description": "Whether user input is needed"
                        },
                        "has_error": {
                            "type": "boolean",
                            "description": "Whether an error occurred"
                        },
                        "error_message": {
                            "type": "string",
                            "description": "Error message if has_error is true"
                        }
                    },
                    "required": ["summary"]
                }),
            ),
            ToolDefinition::new(
                "analyze_output",
                "Parse session output for progress indicators",
                json!({
                    "type": "object",
                    "properties": {
                        "output": {
                            "type": "string",
                            "description": "Raw output from the session to analyze"
                        }
                    },
                    "required": ["output"]
                }),
            ),
        ]
    }

    /// Execute the search_memories tool (agent-isolated).
    pub(super) async fn execute_search_memories(&self, call: &ToolCall) -> Result<ToolResult> {
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

        debug!(
            "Session agent '{}' searching memories: {} (limit: {})",
            self.id, query, limit
        );

        // Generate embedding for the query
        let embedding = self
            .embedder
            .embed(query)
            .await
            .map_err(|e| AgentError::ToolExecution {
                tool_name: call.name.clone(),
                message: format!("Failed to generate embedding: {}", e),
            })?;

        // Search memories - IMPORTANT: filtered by own agent_id for isolation
        let results = self
            .memory
            .search(&embedding, &self.id, limit)
            .await
            .map_err(AgentError::Memory)?;

        let output = format_search_results(&results);
        Ok(ToolResult::success(&call.id, output))
    }

    /// Execute the update_session_state tool.
    /// Call this method directly when you have mutable access to the SessionAgent.
    pub fn execute_update_session_state(&mut self, call: &ToolCall) -> Result<ToolResult> {
        let mut updates = Vec::new();

        if let Some(goal) = call.get_optional_string_arg("add_goal") {
            self.session_state.add_goal(goal);
            updates.push(format!("Added goal: {}", goal));
        }

        if let Some(task) = call.get_optional_string_arg("current_task") {
            self.session_state.set_current_task(task);
            updates.push(format!("Set current task: {}", task));
        }

        if let Some(progress) = call.get_arg("progress").and_then(|v| v.as_f64()) {
            self.session_state.set_progress(progress as f32);
            updates.push(format!("Updated progress: {:.0}%", progress * 100.0));
        }

        if let Some(blocker) = call.get_optional_string_arg("add_blocker") {
            self.session_state.add_blocker(blocker);
            updates.push(format!("Added blocker: {}", blocker));
        }

        if call.get_arg("clear_blockers").and_then(|v| v.as_bool()) == Some(true) {
            self.session_state.clear_blockers();
            updates.push("Cleared all blockers".to_string());
        }

        if let Some(file) = call.get_optional_string_arg("add_modified_file") {
            self.session_state.add_modified_file(file);
            updates.push(format!("Tracked modified file: {}", file));
        }

        let output = if updates.is_empty() {
            "No state updates performed.".to_string()
        } else {
            format!("Session state updated:\n- {}", updates.join("\n- "))
        };

        Ok(ToolResult::success(&call.id, output))
    }

    /// Execute the report_to_user tool.
    pub(super) async fn execute_report_to_user(&self, call: &ToolCall) -> Result<ToolResult> {
        let summary = call.get_string_arg("summary").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        let progress = call.get_arg("progress").and_then(|v| v.as_f64());
        let needs_input = call.get_arg("needs_input").and_then(|v| v.as_bool()).unwrap_or(false);
        let has_error = call.get_arg("has_error").and_then(|v| v.as_bool()).unwrap_or(false);
        let error_message = call.get_optional_string_arg("error_message");

        // Build report
        let mut report = format!(
            "Session Report [{}]:\nSummary: {}",
            self.session_id, summary
        );

        if let Some(p) = progress {
            report.push_str(&format!("\nProgress: {:.0}%", p * 100.0));
        }

        if needs_input {
            report.push_str("\nStatus: NEEDS INPUT");
        }

        if has_error {
            report.push_str(&format!("\nError: {}", error_message.unwrap_or("Unknown error")));
        }

        // Store report in memory for User Agent to retrieve
        if let Err(e) = self.store_memory(&report).await {
            debug!("Failed to store report memory: {}", e);
        }

        tracing::info!("Session {} report: {}", self.session_id, summary);

        Ok(ToolResult::success(&call.id, format!("Report sent: {}", summary)))
    }

    /// Execute the analyze_output tool.
    /// Call this method directly when you have mutable access to the SessionAgent.
    pub async fn execute_analyze_output(&mut self, call: &ToolCall) -> Result<ToolResult> {
        let output = call.get_string_arg("output").map_err(|e| {
            AgentError::InvalidArguments {
                tool_name: call.name.clone(),
                message: e,
            }
        })?;

        let analysis = self.analyze_output(output).await?;

        let result = json!({
            "detected_completion": analysis.detected_completion,
            "waiting_for_input": analysis.waiting_for_input,
            "error_detected": analysis.error_detected,
            "files_changed": analysis.files_changed,
            "summary": analysis.summary
        });

        Ok(ToolResult::success(&call.id, serde_json::to_string_pretty(&result)?))
    }
}

/// Format search results as a human-readable string.
pub(super) fn format_search_results(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No relevant memories found.".to_string();
    }

    let mut output = format!("Found {} relevant memories:\n\n", results.len());

    for (i, result) in results.iter().enumerate() {
        output.push_str(&format!(
            "{}. [Score: {:.2}] {}\n   Created: {}\n\n",
            i + 1,
            result.score,
            result.memory.content,
            result.memory.created_at.format("%Y-%m-%d %H:%M:%S")
        ));
    }

    output
}
