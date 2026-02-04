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

mod adapter_type;
mod prompts;
mod tools;

#[cfg(test)]
mod tests;

pub use adapter_type::AdapterType;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::context_manager::ContextStrategy;
use crate::error::{AgentError, Result};
use crate::tool::ToolDefinition;

use prompts::{CLAUDE_CODE_SYSTEM_PROMPT, GENERIC_SYSTEM_PROMPT, MPM_SYSTEM_PROMPT};
use tools::{claude_code_tools, generic_tools, mpm_tools};

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
