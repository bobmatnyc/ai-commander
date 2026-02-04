//! Tests for agent template module.

use super::*;
use std::fs;
use std::io::Write;
use std::path::Path;
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
