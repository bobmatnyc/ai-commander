//! Integration test for session aliasing functionality.

use commander_models::Project;
use commander_persistence::StateStore;
use tempfile::TempDir;

#[test]
fn test_alias_integration() {
    // Setup
    let temp_dir = TempDir::new().unwrap();
    let store = StateStore::new(temp_dir.path());

    // Create a project
    let mut project = Project::new("/path/to/myapp", "myapp");
    project
        .config
        .insert("tool".to_string(), serde_json::json!("claude-code"));
    store.save_project(&project).unwrap();

    // Add aliases
    project.add_alias("prod".to_string()).unwrap();
    project.add_alias("staging".to_string()).unwrap();
    store.save_project(&project).unwrap();

    // Test: Find by project name
    let found = store.find_project_by_name_or_alias("myapp").unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "myapp");

    // Test: Find by alias
    let found = store.find_project_by_name_or_alias("prod").unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "myapp");

    let found = store.find_project_by_name_or_alias("staging").unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "myapp");

    // Test: Alias collision detection
    assert!(store.alias_exists("prod").unwrap());
    assert!(store.alias_exists("staging").unwrap());
    assert!(!store.alias_exists("nonexistent").unwrap());

    // Test: Remove alias
    let mut project = store.find_project_by_name_or_alias("myapp").unwrap().unwrap();
    assert!(project.remove_alias("prod"));
    store.save_project(&project).unwrap();

    // Verify removal - prod should no longer resolve to myapp
    let found = store.find_project_by_name_or_alias("prod").unwrap();
    assert!(found.is_none());

    // Test: Alias collision detection with another project
    let mut other_project = Project::new("/path/to/other", "other");
    // Add the alias "staging" which is still in use by myapp
    let result = other_project.add_alias("staging".to_string());
    assert!(result.is_ok()); // Add is OK at project level
    store.save_project(&other_project).unwrap();

    // StateStore should detect that "staging" exists
    assert!(store.alias_exists("staging").unwrap());

    // Test: Aliases persist across load/save
    let loaded = store.find_project_by_name_or_alias("myapp").unwrap().unwrap();
    assert_eq!(loaded.aliases, vec!["staging"]);
}

#[test]
fn test_alias_validation() {
    let mut project = Project::new("/path", "test");

    // Valid aliases
    assert!(project.add_alias("prod".to_string()).is_ok());
    assert!(project.add_alias("dev-1".to_string()).is_ok());
    assert!(project.add_alias("my_alias".to_string()).is_ok());

    // Invalid: special characters
    assert!(project.add_alias("prod@staging".to_string()).is_err());

    // Invalid: too long
    let long_alias = "a".repeat(65);
    assert!(project.add_alias(long_alias).is_err());

    // Invalid: empty
    assert!(project.add_alias("".to_string()).is_err());
}

#[test]
fn test_alias_max_limit() {
    let mut project = Project::new("/path", "test");

    // Add 10 aliases (maximum)
    for i in 0..10 {
        assert!(project.add_alias(format!("alias{}", i)).is_ok());
    }

    // Try to add 11th
    assert!(project.add_alias("alias11".to_string()).is_err());
}

#[test]
fn test_backward_compatibility() {
    let temp_dir = TempDir::new().unwrap();
    let store = StateStore::new(temp_dir.path());

    // Simulate old project JSON without aliases field
    let json = r#"{
        "id": "proj-123",
        "path": "/path",
        "name": "test",
        "state": "idle",
        "config_loaded": false,
        "config": {},
        "sessions": {},
        "work_queue": [],
        "completed_work": [],
        "pending_events": [],
        "event_history": [],
        "thread": [],
        "created_at": "2024-01-01T00:00:00Z"
    }"#;

    let project: Project = serde_json::from_str(json).unwrap();
    assert!(project.aliases.is_empty());

    // Should be able to save and load
    store.save_project(&project).unwrap();
    let loaded = store.load_project(&project.id).unwrap();
    assert!(loaded.aliases.is_empty());
}
