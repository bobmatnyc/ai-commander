//! Storage migration from legacy paths.
//!
//! Handles migration from the old `~/.commander` directory structure
//! to the new `~/.ai-commander` directory structure.

use std::fs;
use std::path::Path;

use tracing::{debug, info, warn};

use crate::config;

/// Files that should be migrated to the state subdirectory.
const STATE_FILES: &[&str] = &[
    "pairings.json",
    "telegram.pid",
    "projects.json",
    "notifications.json",
];

/// Files that should be migrated to the config subdirectory.
const CONFIG_FILES: &[&str] = &[
    "config.toml",
    ".env.local",
];

/// Check if migration is needed and perform it.
///
/// Migration is needed if:
/// 1. The legacy `~/.commander` directory exists
/// 2. The new `~/.ai-commander` directory does NOT exist (or is empty)
///
/// # Returns
/// - `Ok(true)` if migration was performed
/// - `Ok(false)` if no migration was needed
/// - `Err` if migration failed
pub fn migrate_if_needed() -> std::io::Result<bool> {
    let Some(legacy_dir) = config::legacy_state_dir() else {
        debug!("No home directory found, skipping migration check");
        return Ok(false);
    };

    // If legacy directory doesn't exist, no migration needed
    if !legacy_dir.exists() {
        debug!("Legacy directory does not exist, no migration needed");
        return Ok(false);
    }

    let new_dir = config::state_dir();

    // If new directory already exists and has content, don't migrate
    if new_dir.exists() && has_content(&new_dir) {
        debug!(
            "New directory already exists with content at {:?}, skipping migration",
            new_dir
        );
        return Ok(false);
    }

    info!(
        "Migrating from {:?} to {:?}",
        legacy_dir, new_dir
    );

    perform_migration(&legacy_dir, &new_dir)?;

    info!("Migration completed successfully");
    Ok(true)
}

/// Check if a directory has any content.
fn has_content(dir: &Path) -> bool {
    if let Ok(entries) = fs::read_dir(dir) {
        return entries.count() > 0;
    }
    false
}

/// Perform the actual migration.
fn perform_migration(legacy_dir: &Path, new_dir: &Path) -> std::io::Result<()> {
    // Create all new directories
    config::ensure_all_dirs()?;

    // Migrate state files
    let state_dir = config::runtime_state_dir();
    for file_name in STATE_FILES {
        let src = legacy_dir.join(file_name);
        if src.exists() {
            let dst = state_dir.join(file_name);
            migrate_file(&src, &dst)?;
        }
    }

    // Migrate config files
    let config_subdir = config::config_dir();
    for file_name in CONFIG_FILES {
        let src = legacy_dir.join(file_name);
        if src.exists() {
            let dst = config_subdir.join(file_name);
            migrate_file(&src, &dst)?;
        }
    }

    // Migrate sessions directory
    let legacy_sessions = legacy_dir.join("sessions");
    if legacy_sessions.exists() && legacy_sessions.is_dir() {
        let new_sessions = config::sessions_dir();
        migrate_directory(&legacy_sessions, &new_sessions)?;
    }

    // Leave a marker file in the legacy directory indicating migration happened
    let marker_path = legacy_dir.join(".migrated-to-ai-commander");
    let marker_content = format!(
        "This directory has been migrated to {:?}\n\
         Migration date: {}\n\
         You can safely delete this directory.\n",
        new_dir,
        chrono::Utc::now().to_rfc3339()
    );
    if let Err(e) = fs::write(&marker_path, marker_content) {
        warn!("Could not write migration marker: {}", e);
    }

    Ok(())
}

/// Migrate a single file by copying it.
fn migrate_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    debug!("Migrating {:?} -> {:?}", src, dst);

    // Ensure parent directory exists
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    // Copy the file (don't move, in case something goes wrong)
    fs::copy(src, dst)?;

    info!("Migrated: {:?}", src.file_name().unwrap_or_default());
    Ok(())
}

/// Migrate a directory by copying all its contents.
fn migrate_directory(src: &Path, dst: &Path) -> std::io::Result<()> {
    debug!("Migrating directory {:?} -> {:?}", src, dst);

    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            migrate_directory(&src_path, &dst_path)?;
        } else {
            migrate_file(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_has_content_empty() {
        let dir = tempdir().unwrap();
        assert!(!has_content(dir.path()));
    }

    #[test]
    fn test_has_content_with_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), "content").unwrap();
        assert!(has_content(dir.path()));
    }

    #[test]
    fn test_migrate_file() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let src_file = src_dir.path().join("test.json");
        let dst_file = dst_dir.path().join("subdir").join("test.json");

        fs::write(&src_file, r#"{"key": "value"}"#).unwrap();

        migrate_file(&src_file, &dst_file).unwrap();

        assert!(dst_file.exists());
        assert_eq!(
            fs::read_to_string(&dst_file).unwrap(),
            r#"{"key": "value"}"#
        );
        // Source should still exist (copy, not move)
        assert!(src_file.exists());
    }

    #[test]
    fn test_migrate_directory() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        // Create source structure
        let src_sessions = src_dir.path().join("sessions");
        fs::create_dir(&src_sessions).unwrap();
        fs::write(src_sessions.join("session1.json"), "{}").unwrap();
        fs::write(src_sessions.join("session2.json"), "{}").unwrap();

        let dst_sessions = dst_dir.path().join("sessions");
        migrate_directory(&src_sessions, &dst_sessions).unwrap();

        assert!(dst_sessions.exists());
        assert!(dst_sessions.join("session1.json").exists());
        assert!(dst_sessions.join("session2.json").exists());
    }
}
