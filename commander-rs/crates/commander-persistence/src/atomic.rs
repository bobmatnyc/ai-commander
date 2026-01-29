//! Atomic file operations for crash-safe persistence.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::{PersistenceError, Result};

/// Writes data to a file atomically.
///
/// This function writes to a temporary file first, then renames it to the
/// target path. This ensures that the file is never in a partially written
/// state, even if the process crashes.
///
/// # Arguments
/// * `path` - The target file path
/// * `data` - The data to write
///
/// # Errors
/// Returns an error if the write or rename fails.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|source| PersistenceError::DirectoryError {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }

    // Create temp file in same directory (for same-filesystem rename)
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut temp_file =
        tempfile::NamedTempFile::new_in(dir).map_err(|source| PersistenceError::WriteError {
            path: path.to_path_buf(),
            source,
        })?;

    // Write data to temp file
    temp_file
        .write_all(data)
        .map_err(|source| PersistenceError::WriteError {
            path: path.to_path_buf(),
            source,
        })?;

    // Sync to disk
    temp_file
        .flush()
        .map_err(|source| PersistenceError::WriteError {
            path: path.to_path_buf(),
            source,
        })?;

    // Atomic rename
    temp_file
        .persist(path)
        .map_err(|e| PersistenceError::WriteError {
            path: path.to_path_buf(),
            source: e.error,
        })?;

    Ok(())
}

/// Writes JSON data to a file atomically.
pub fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    atomic_write(path, json.as_bytes())
}

/// Reads and deserializes JSON from a file.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let data = fs::read_to_string(path).map_err(|source| PersistenceError::ReadError {
        path: path.to_path_buf(),
        source,
    })?;
    let value = serde_json::from_str(&data)?;
    Ok(value)
}

/// Reads JSON from a file, returning None if the file doesn't exist.
pub fn read_json_optional<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    read_json(path).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[test]
    fn test_atomic_write_creates_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write(&path, b"hello world").unwrap();

        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn test_atomic_write_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested/dir/test.txt");

        atomic_write(&path, b"nested content").unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_atomic_write_json_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data.json");

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        atomic_write_json(&path, &data).unwrap();
        let loaded: TestData = read_json(&path).unwrap();

        assert_eq!(data, loaded);
    }

    #[test]
    fn test_read_json_optional_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.json");

        let result: Option<TestData> = read_json_optional(&path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_json_optional_exists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("exists.json");

        let data = TestData {
            name: "exists".to_string(),
            value: 99,
        };
        atomic_write_json(&path, &data).unwrap();

        let result: Option<TestData> = read_json_optional(&path).unwrap();
        assert_eq!(result, Some(data));
    }
}
