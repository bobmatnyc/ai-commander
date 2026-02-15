//! Integration tests for rebuild detection and session persistence.

use commander_telegram::{check_rebuild, load_version, BotVersion};

#[test]
fn test_version_tracking() {
    // Create a new version
    let version = BotVersion::new();
    assert_eq!(version.start_count, 1);
    assert!(version.is_first_start());

    // Simulate restart
    let mut version2 = version.clone();
    let is_rebuild = version2.update();
    assert!(!is_rebuild, "Should not be a rebuild on restart");
    assert_eq!(version2.start_count, 2);
    assert!(!version2.is_first_start());
}

#[test]
fn test_version_persistence() {
    // This test uses the actual runtime state directory
    // In production, this would be ~/.ai-commander/state/

    // Check rebuild detection
    let (_is_rebuild, _is_first_start, start_count) = check_rebuild();

    // Verify structure (actual values depend on previous runs)
    assert!(start_count >= 1, "Start count should be at least 1");

    // Load and verify version was saved
    let loaded_version = load_version();
    assert!(loaded_version.start_count >= 1);
    assert!(loaded_version.binary_hash > 0);
}

#[test]
fn test_persisted_session_validation() {
    use commander_telegram::session::PersistedSession;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Create a recent session (< 24h old)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let recent_session = PersistedSession {
        chat_id: 12345,
        project_path: "/test/path".to_string(),
        project_name: "test-project".to_string(),
        tmux_session: "commander-test".to_string(),
        thread_id: None,
        worktree_info: None,
        created_at: now - 3600, // 1 hour ago
        last_activity: now - 60, // 1 minute ago
    };

    assert!(recent_session.is_valid(), "Recent session should be valid");
    assert!(recent_session.age_seconds() < 24 * 60 * 60);

    // Create an old session (> 24h old)
    let old_session = PersistedSession {
        chat_id: 67890,
        project_path: "/old/path".to_string(),
        project_name: "old-project".to_string(),
        tmux_session: "commander-old".to_string(),
        thread_id: None,
        worktree_info: None,
        created_at: now - (25 * 60 * 60), // 25 hours ago
        last_activity: now - (24 * 60 * 60), // 24 hours ago
    };

    assert!(!old_session.is_valid(), "Old session should be invalid");
    assert!(old_session.age_seconds() >= 24 * 60 * 60);
}

#[test]
fn test_session_restoration() {
    use commander_telegram::session::PersistedSession;
    use teloxide::types::{ChatId, MessageId, ThreadId};

    // Test restoration without thread_id
    let persisted = PersistedSession {
        chat_id: 12345,
        project_path: "/test/path".to_string(),
        project_name: "test-project".to_string(),
        tmux_session: "commander-test".to_string(),
        thread_id: None,
        worktree_info: None,
        created_at: 1000,
        last_activity: 2000,
    };

    let restored = persisted.restore_to_user_session();
    assert_eq!(restored.chat_id, ChatId(12345));
    assert_eq!(restored.project_path, "/test/path");
    assert_eq!(restored.project_name, "test-project");
    assert_eq!(restored.tmux_session, "commander-test");
    assert_eq!(restored.thread_id, None);

    // Test restoration with thread_id
    let persisted_with_thread = PersistedSession {
        chat_id: 67890,
        project_path: "/thread/path".to_string(),
        project_name: "thread-project".to_string(),
        tmux_session: "commander-thread".to_string(),
        thread_id: Some(999),
        worktree_info: None,
        created_at: 1000,
        last_activity: 2000,
    };

    let restored_with_thread = persisted_with_thread.restore_to_user_session();
    assert_eq!(restored_with_thread.chat_id, ChatId(67890));
    assert_eq!(restored_with_thread.thread_id, Some(ThreadId(MessageId(999))));
}

#[test]
fn test_session_serialization() {
    use commander_telegram::session::PersistedSession;

    let session = PersistedSession {
        chat_id: 12345,
        project_path: "/test/path".to_string(),
        project_name: "test-project".to_string(),
        tmux_session: "commander-test".to_string(),
        thread_id: Some(999),
        worktree_info: None,
        created_at: 1000,
        last_activity: 2000,
    };

    // Serialize
    let json = serde_json::to_string(&session).expect("Failed to serialize");

    // Deserialize
    let deserialized: PersistedSession = serde_json::from_str(&json)
        .expect("Failed to deserialize");

    assert_eq!(deserialized.chat_id, session.chat_id);
    assert_eq!(deserialized.project_path, session.project_path);
    assert_eq!(deserialized.project_name, session.project_name);
    assert_eq!(deserialized.tmux_session, session.tmux_session);
    assert_eq!(deserialized.thread_id, session.thread_id);
    assert_eq!(deserialized.created_at, session.created_at);
    assert_eq!(deserialized.last_activity, session.last_activity);
}
