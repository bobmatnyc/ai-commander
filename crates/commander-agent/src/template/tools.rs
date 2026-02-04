//! Tool definitions for agent templates.

use serde_json::json;

use crate::tool::ToolDefinition;

/// Tools for Claude Code sessions.
pub fn claude_code_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "parse_output",
            "Parse Claude Code output to extract progress information",
            json!({
                "type": "object",
                "properties": {
                    "output": {
                        "type": "string",
                        "description": "Raw output from Claude Code session"
                    }
                },
                "required": ["output"]
            }),
        ),
        ToolDefinition::new(
            "track_files",
            "Track files that have been modified in the session",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["add", "remove", "list"],
                        "description": "Action to perform on file tracking"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file (for add/remove actions)"
                    }
                },
                "required": ["action"]
            }),
        ),
        ToolDefinition::new(
            "detect_completion",
            "Detect if the current task or subtask has completed",
            json!({
                "type": "object",
                "properties": {
                    "context": {
                        "type": "string",
                        "description": "Recent output context to analyze"
                    }
                },
                "required": ["context"]
            }),
        ),
        ToolDefinition::new(
            "report_status",
            "Generate a status report for the current session",
            json!({
                "type": "object",
                "properties": {
                    "include_files": {
                        "type": "boolean",
                        "description": "Include list of modified files"
                    },
                    "include_errors": {
                        "type": "boolean",
                        "description": "Include any detected errors"
                    }
                },
                "required": []
            }),
        ),
    ]
}

/// Tools for MPM orchestration sessions.
pub fn mpm_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "track_delegation",
            "Track an agent delegation event",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "ID of the delegated agent"
                    },
                    "task": {
                        "type": "string",
                        "description": "Task delegated to the agent"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["started", "completed", "failed"],
                        "description": "Status of the delegation"
                    }
                },
                "required": ["agent_id", "task", "status"]
            }),
        ),
        ToolDefinition::new(
            "aggregate_status",
            "Aggregate status from all sub-agents",
            json!({
                "type": "object",
                "properties": {
                    "include_pending": {
                        "type": "boolean",
                        "description": "Include pending delegations"
                    }
                },
                "required": []
            }),
        ),
        ToolDefinition::new(
            "list_agents",
            "List all active agents in the orchestration",
            json!({
                "type": "object",
                "properties": {
                    "status_filter": {
                        "type": "string",
                        "enum": ["all", "active", "completed", "failed"],
                        "description": "Filter agents by status"
                    }
                },
                "required": []
            }),
        ),
    ]
}

/// Tools for generic terminal sessions.
pub fn generic_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "detect_ready",
            "Detect if the terminal is ready for input",
            json!({
                "type": "object",
                "properties": {
                    "output": {
                        "type": "string",
                        "description": "Recent terminal output"
                    }
                },
                "required": ["output"]
            }),
        ),
        ToolDefinition::new(
            "report_output",
            "Report and summarize terminal output",
            json!({
                "type": "object",
                "properties": {
                    "output": {
                        "type": "string",
                        "description": "Terminal output to report"
                    },
                    "max_lines": {
                        "type": "integer",
                        "description": "Maximum lines to include in summary"
                    }
                },
                "required": ["output"]
            }),
        ),
    ]
}
