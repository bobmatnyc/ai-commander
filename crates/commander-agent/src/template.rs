//! Agent templates for different adapter types.
//!
//! This module provides specialized templates that configure agents based on
//! the type of adapter they are managing. Each template includes a tailored
//! system prompt, tools, memory categories, and optional model override.
//!
//! # Built-in Templates
//!
//! - [`AdapterType::ClaudeCode`]: For managing Claude Code coding sessions
//! - [`AdapterType::Mpm`]: For managing MPM orchestration sessions
//! - [`AdapterType::Generic`]: For generic terminal/shell sessions
//!
//! # Example
//!
//! ```
//! use commander_agent::template::{AdapterType, TemplateRegistry};
//!
//! let registry = TemplateRegistry::new();
//! let template = registry.get(&AdapterType::ClaudeCode).unwrap();
//!
//! assert_eq!(template.adapter_type, AdapterType::ClaudeCode);
//! assert!(!template.memory_categories.is_empty());
//! ```

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::context_manager::ContextStrategy;
use crate::error::{AgentError, Result};
use crate::tool::ToolDefinition;

/// Type of adapter that the agent is managing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AdapterType {
    /// Claude Code AI coding assistant.
    ClaudeCode,
    /// MPM multi-agent orchestration.
    Mpm,
    /// Generic terminal/shell session.
    Generic,
}

impl std::fmt::Display for AdapterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClaudeCode => write!(f, "claude_code"),
            Self::Mpm => write!(f, "mpm"),
            Self::Generic => write!(f, "generic"),
        }
    }
}

impl std::str::FromStr for AdapterType {
    type Err = AgentError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "claude_code" | "claudecode" | "claude-code" => Ok(Self::ClaudeCode),
            "mpm" => Ok(Self::Mpm),
            "generic" | "shell" => Ok(Self::Generic),
            _ => Err(AgentError::Configuration(format!(
                "unknown adapter type: {}",
                s
            ))),
        }
    }
}

/// Template configuration for an agent managing a specific adapter type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    /// Type of adapter this template is for.
    pub adapter_type: AdapterType,

    /// System prompt customized for this adapter type.
    pub system_prompt: String,

    /// Tools available to the agent.
    pub tools: Vec<ToolDefinition>,

    /// Memory categories for organizing agent memories.
    pub memory_categories: Vec<String>,

    /// Optional model override (if different from default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_override: Option<String>,

    /// Context management strategy for this adapter type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_strategy: Option<ContextStrategy>,
}

impl AgentTemplate {
    /// Create a new agent template.
    pub fn new(adapter_type: AdapterType) -> Self {
        Self {
            adapter_type,
            system_prompt: String::new(),
            tools: Vec::new(),
            memory_categories: Vec::new(),
            model_override: None,
            context_strategy: None,
        }
    }

    /// Set the system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    /// Add a tool definition.
    pub fn with_tool(mut self, tool: ToolDefinition) -> Self {
        self.tools.push(tool);
        self
    }

    /// Set tools.
    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    /// Add a memory category.
    pub fn with_memory_category(mut self, category: impl Into<String>) -> Self {
        self.memory_categories.push(category.into());
        self
    }

    /// Set memory categories.
    pub fn with_memory_categories(mut self, categories: Vec<String>) -> Self {
        self.memory_categories = categories;
        self
    }

    /// Set the model override.
    pub fn with_model_override(mut self, model: impl Into<String>) -> Self {
        self.model_override = Some(model.into());
        self
    }

    /// Set the context strategy.
    pub fn with_context_strategy(mut self, strategy: ContextStrategy) -> Self {
        self.context_strategy = Some(strategy);
        self
    }

    /// Create the Claude Code template with built-in configuration.
    pub fn claude_code() -> Self {
        Self {
            adapter_type: AdapterType::ClaudeCode,
            system_prompt: CLAUDE_CODE_SYSTEM_PROMPT.to_string(),
            tools: claude_code_tools(),
            memory_categories: vec![
                "code_patterns".to_string(),
                "project_structure".to_string(),
                "user_preferences".to_string(),
            ],
            model_override: None,
            context_strategy: Some(ContextStrategy::Compaction),
        }
    }

    /// Create the MPM template with built-in configuration.
    pub fn mpm() -> Self {
        Self {
            adapter_type: AdapterType::Mpm,
            system_prompt: MPM_SYSTEM_PROMPT.to_string(),
            tools: mpm_tools(),
            memory_categories: vec![
                "delegation_patterns".to_string(),
                "agent_capabilities".to_string(),
                "workflow_history".to_string(),
            ],
            model_override: None,
            context_strategy: Some(ContextStrategy::PauseResume {
                pause_command: "/mpm-session-pause".to_string(),
                resume_command: "/mpm-session-resume".to_string(),
            }),
        }
    }

    /// Create the Generic template with built-in configuration.
    pub fn generic() -> Self {
        Self {
            adapter_type: AdapterType::Generic,
            system_prompt: GENERIC_SYSTEM_PROMPT.to_string(),
            tools: generic_tools(),
            memory_categories: vec!["session_history".to_string()],
            model_override: None,
            context_strategy: Some(ContextStrategy::WarnAndContinue),
        }
    }
}

/// Registry for managing agent templates.
#[derive(Debug, Clone)]
pub struct TemplateRegistry {
    templates: HashMap<AdapterType, AgentTemplate>,
    custom_dir: Option<std::path::PathBuf>,
}

impl Default for TemplateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateRegistry {
    /// Create a new template registry with built-in templates.
    pub fn new() -> Self {
        let mut templates = HashMap::new();

        // Register built-in templates
        templates.insert(AdapterType::ClaudeCode, AgentTemplate::claude_code());
        templates.insert(AdapterType::Mpm, AgentTemplate::mpm());
        templates.insert(AdapterType::Generic, AgentTemplate::generic());

        Self {
            templates,
            custom_dir: None,
        }
    }

    /// Create a registry with a custom template directory.
    pub fn with_custom_dir(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.custom_dir = Some(path.into());
        self
    }

    /// Get a template by adapter type.
    pub fn get(&self, adapter_type: &AdapterType) -> Option<&AgentTemplate> {
        self.templates.get(adapter_type)
    }

    /// Register a custom template (replaces existing if same adapter type).
    pub fn register(&mut self, template: AgentTemplate) {
        self.templates.insert(template.adapter_type.clone(), template);
    }

    /// Load custom templates from a directory.
    ///
    /// Supports both YAML (.yaml, .yml) and JSON (.json) files.
    /// Files should be named after the adapter type (e.g., `claude_code.yaml`).
    pub fn load_custom(&mut self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Err(AgentError::Configuration(format!(
                "template directory not found: {}",
                path.display()
            )));
        }

        if !path.is_dir() {
            return Err(AgentError::Configuration(format!(
                "path is not a directory: {}",
                path.display()
            )));
        }

        for entry in fs::read_dir(path).map_err(|e| {
            AgentError::Configuration(format!("failed to read directory {}: {}", path.display(), e))
        })? {
            let entry = entry.map_err(|e| {
                AgentError::Configuration(format!("failed to read directory entry: {}", e))
            })?;
            let file_path = entry.path();

            if let Some(extension) = file_path.extension() {
                let template = match extension.to_str() {
                    Some("yaml") | Some("yml") => self.load_yaml_template(&file_path)?,
                    Some("json") => self.load_json_template(&file_path)?,
                    _ => continue,
                };

                self.register(template);
            }
        }

        self.custom_dir = Some(path.to_path_buf());
        Ok(())
    }

    /// Load a single template from a YAML file.
    fn load_yaml_template(&self, path: &Path) -> Result<AgentTemplate> {
        let content = fs::read_to_string(path).map_err(|e| {
            AgentError::Configuration(format!("failed to read {}: {}", path.display(), e))
        })?;

        serde_yaml::from_str(&content).map_err(|e| {
            AgentError::Configuration(format!("failed to parse YAML {}: {}", path.display(), e))
        })
    }

    /// Load a single template from a JSON file.
    fn load_json_template(&self, path: &Path) -> Result<AgentTemplate> {
        let content = fs::read_to_string(path).map_err(|e| {
            AgentError::Configuration(format!("failed to read {}: {}", path.display(), e))
        })?;

        serde_json::from_str(&content).map_err(AgentError::Serialization)
    }

    /// List all registered adapter types.
    pub fn adapter_types(&self) -> Vec<&AdapterType> {
        self.templates.keys().collect()
    }

    /// Get the custom directory path if set.
    pub fn custom_dir(&self) -> Option<&Path> {
        self.custom_dir.as_deref()
    }
}

// =============================================================================
// System Prompts
// =============================================================================

const CLAUDE_CODE_SYSTEM_PROMPT: &str = r#"You are a session agent managing a Claude Code session.
Your role is to understand the coding task, track progress, and report status.

Key behaviors:
- Parse Claude Code output for progress indicators
- Track files modified and tests run
- Identify when user input is needed
- Summarize completed work
- Detect errors and blockers

## Context Management
Claude Code handles context through compaction:
- Recent messages kept in full detail
- Older messages automatically summarized
- Key facts and decisions preserved

When context is running low:
1. Important context is preserved through summarization
2. Continue working without interruption
3. Recent conversation and current task always available"#;

const MPM_SYSTEM_PROMPT: &str = r#"You are a session agent managing an MPM orchestration session.
Track multi-agent delegation, task completion, and coordination.

Key behaviors:
- Monitor agent delegations
- Track task completion across agents
- Aggregate status from sub-agents
- Identify workflow blockers

## Context Management
When context usage reaches critical levels (< 10% remaining):
1. Execute `/mpm-session-pause` to save current state
2. Summarize work completed and remaining tasks
3. Provide resume instructions

When resuming a session:
1. Execute `/mpm-session-resume` to load saved state
2. Review the saved context
3. Continue from where you left off

## Pause State Format
When pausing, create a summary:
```
## Session Pause State
Tasks Completed: [list]
Tasks In Progress: [list]
Tasks Remaining: [list]
Current Focus: [description]
Next Action: [what to do when resumed]
```"#;

const GENERIC_SYSTEM_PROMPT: &str = r#"You are a session agent managing a terminal session.
Track command execution and output.

Key behaviors:
- Monitor command output
- Detect command completion
- Track working directory
- Report session state

## Context Management
When context usage reaches critical levels:
- You will receive warnings about context capacity
- Consider starting a new session if capacity is low
- Important information from early in the session may be summarized"#;

// =============================================================================
// Tool Definitions
// =============================================================================

fn claude_code_tools() -> Vec<ToolDefinition> {
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

fn mpm_tools() -> Vec<ToolDefinition> {
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

fn generic_tools() -> Vec<ToolDefinition> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_adapter_type_display() {
        assert_eq!(AdapterType::ClaudeCode.to_string(), "claude_code");
        assert_eq!(AdapterType::Mpm.to_string(), "mpm");
        assert_eq!(AdapterType::Generic.to_string(), "generic");
    }

    #[test]
    fn test_adapter_type_from_str() {
        assert_eq!(
            "claude_code".parse::<AdapterType>().unwrap(),
            AdapterType::ClaudeCode
        );
        assert_eq!(
            "claude-code".parse::<AdapterType>().unwrap(),
            AdapterType::ClaudeCode
        );
        assert_eq!("mpm".parse::<AdapterType>().unwrap(), AdapterType::Mpm);
        assert_eq!(
            "generic".parse::<AdapterType>().unwrap(),
            AdapterType::Generic
        );
        assert_eq!(
            "shell".parse::<AdapterType>().unwrap(),
            AdapterType::Generic
        );

        assert!("unknown".parse::<AdapterType>().is_err());
    }

    #[test]
    fn test_agent_template_builder() {
        let template = AgentTemplate::new(AdapterType::ClaudeCode)
            .with_system_prompt("Test prompt")
            .with_memory_category("test_category")
            .with_model_override("test-model");

        assert_eq!(template.adapter_type, AdapterType::ClaudeCode);
        assert_eq!(template.system_prompt, "Test prompt");
        assert_eq!(template.memory_categories, vec!["test_category"]);
        assert_eq!(template.model_override, Some("test-model".to_string()));
    }

    #[test]
    fn test_claude_code_template() {
        let template = AgentTemplate::claude_code();

        assert_eq!(template.adapter_type, AdapterType::ClaudeCode);
        assert!(!template.system_prompt.is_empty());
        assert!(!template.tools.is_empty());
        assert!(template.memory_categories.contains(&"code_patterns".to_string()));
        assert!(template
            .memory_categories
            .contains(&"project_structure".to_string()));
        assert!(template
            .memory_categories
            .contains(&"user_preferences".to_string()));
        assert!(template.model_override.is_none());

        // Check context strategy
        assert!(matches!(
            template.context_strategy,
            Some(ContextStrategy::Compaction)
        ));

        // Check tools
        let tool_names: Vec<&str> = template.tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"parse_output"));
        assert!(tool_names.contains(&"track_files"));
        assert!(tool_names.contains(&"detect_completion"));
        assert!(tool_names.contains(&"report_status"));
    }

    #[test]
    fn test_mpm_template() {
        let template = AgentTemplate::mpm();

        assert_eq!(template.adapter_type, AdapterType::Mpm);
        assert!(!template.system_prompt.is_empty());
        assert!(!template.tools.is_empty());
        assert!(template
            .memory_categories
            .contains(&"delegation_patterns".to_string()));
        assert!(template
            .memory_categories
            .contains(&"agent_capabilities".to_string()));
        assert!(template
            .memory_categories
            .contains(&"workflow_history".to_string()));

        // Check context strategy
        assert!(matches!(
            template.context_strategy,
            Some(ContextStrategy::PauseResume { .. })
        ));
        if let Some(ContextStrategy::PauseResume { pause_command, resume_command }) = &template.context_strategy {
            assert_eq!(pause_command, "/mpm-session-pause");
            assert_eq!(resume_command, "/mpm-session-resume");
        }

        // Check tools
        let tool_names: Vec<&str> = template.tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"track_delegation"));
        assert!(tool_names.contains(&"aggregate_status"));
        assert!(tool_names.contains(&"list_agents"));
    }

    #[test]
    fn test_generic_template() {
        let template = AgentTemplate::generic();

        assert_eq!(template.adapter_type, AdapterType::Generic);
        assert!(!template.system_prompt.is_empty());
        assert!(!template.tools.is_empty());
        assert!(template
            .memory_categories
            .contains(&"session_history".to_string()));

        // Check context strategy
        assert!(matches!(
            template.context_strategy,
            Some(ContextStrategy::WarnAndContinue)
        ));

        // Check tools
        let tool_names: Vec<&str> = template.tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"detect_ready"));
        assert!(tool_names.contains(&"report_output"));
    }

    #[test]
    fn test_template_registry_new() {
        let registry = TemplateRegistry::new();

        // All built-in templates should be present
        assert!(registry.get(&AdapterType::ClaudeCode).is_some());
        assert!(registry.get(&AdapterType::Mpm).is_some());
        assert!(registry.get(&AdapterType::Generic).is_some());

        // Check adapter types list
        let types = registry.adapter_types();
        assert_eq!(types.len(), 3);
    }

    #[test]
    fn test_template_registry_register() {
        let mut registry = TemplateRegistry::new();

        let custom_template = AgentTemplate::new(AdapterType::ClaudeCode)
            .with_system_prompt("Custom prompt");

        registry.register(custom_template);

        let template = registry.get(&AdapterType::ClaudeCode).unwrap();
        assert_eq!(template.system_prompt, "Custom prompt");
    }

    #[test]
    fn test_template_serialization_json() {
        let template = AgentTemplate::claude_code();
        let json = serde_json::to_string_pretty(&template).unwrap();
        let parsed: AgentTemplate = serde_json::from_str(&json).unwrap();

        assert_eq!(template.adapter_type, parsed.adapter_type);
        assert_eq!(template.system_prompt, parsed.system_prompt);
        assert_eq!(template.tools.len(), parsed.tools.len());
        assert_eq!(template.memory_categories, parsed.memory_categories);
    }

    #[test]
    fn test_template_serialization_yaml() {
        let template = AgentTemplate::mpm();
        let yaml = serde_yaml::to_string(&template).unwrap();
        let parsed: AgentTemplate = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(template.adapter_type, parsed.adapter_type);
        assert_eq!(template.system_prompt, parsed.system_prompt);
    }

    #[test]
    fn test_load_custom_templates_json() {
        let temp_dir = TempDir::new().unwrap();
        let template_path = temp_dir.path().join("claude_code.json");

        let custom_template = AgentTemplate::new(AdapterType::ClaudeCode)
            .with_system_prompt("Custom JSON prompt")
            .with_memory_category("custom_category");

        let json = serde_json::to_string_pretty(&custom_template).unwrap();
        let mut file = fs::File::create(&template_path).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        let mut registry = TemplateRegistry::new();
        registry.load_custom(temp_dir.path()).unwrap();

        let loaded = registry.get(&AdapterType::ClaudeCode).unwrap();
        assert_eq!(loaded.system_prompt, "Custom JSON prompt");
        assert!(loaded
            .memory_categories
            .contains(&"custom_category".to_string()));
    }

    #[test]
    fn test_load_custom_templates_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let template_path = temp_dir.path().join("mpm.yaml");

        let custom_template = AgentTemplate::new(AdapterType::Mpm)
            .with_system_prompt("Custom YAML prompt");

        let yaml = serde_yaml::to_string(&custom_template).unwrap();
        let mut file = fs::File::create(&template_path).unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let mut registry = TemplateRegistry::new();
        registry.load_custom(temp_dir.path()).unwrap();

        let loaded = registry.get(&AdapterType::Mpm).unwrap();
        assert_eq!(loaded.system_prompt, "Custom YAML prompt");
    }

    #[test]
    fn test_load_custom_nonexistent_dir() {
        let mut registry = TemplateRegistry::new();
        let result = registry.load_custom(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_custom_not_a_directory() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("not_a_dir.txt");
        fs::write(&file_path, "test").unwrap();

        let mut registry = TemplateRegistry::new();
        let result = registry.load_custom(&file_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_custom_dir_tracking() {
        let temp_dir = TempDir::new().unwrap();
        let mut registry = TemplateRegistry::new();

        assert!(registry.custom_dir().is_none());

        registry.load_custom(temp_dir.path()).unwrap();

        assert_eq!(registry.custom_dir(), Some(temp_dir.path()));
    }
}
