//! Commander CLI library.
//!
//! This crate provides the command-line interface and interactive REPL
//! for Commander.

pub mod chat;
pub mod cli;
pub mod commands;
pub mod filesystem;
pub mod repl;
pub mod tui;

use std::path::Path;

/// Validate that a project path exists, is a directory, and is accessible.
///
/// Returns `Ok(())` if the path is valid, or `Err(message)` describing the issue.
pub fn validate_project_path(path: &str) -> Result<(), String> {
    let path = Path::new(path);

    if !path.exists() {
        return Err(format!("Project path does not exist: {}", path.display()));
    }

    if !path.is_dir() {
        return Err(format!(
            "Project path is not a directory: {}",
            path.display()
        ));
    }

    // Check if readable by attempting to read dir
    if path.read_dir().is_err() {
        return Err(format!(
            "Cannot access project path: {} (permission denied)",
            path.display()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_validate_project_path_valid_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let result = validate_project_path(temp_dir.path().to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_project_path_nonexistent() {
        let result = validate_project_path("/nonexistent/path/that/does/not/exist");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn test_validate_project_path_is_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_file.txt");
        fs::write(&file_path, "test").unwrap();

        let result = validate_project_path(file_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a directory"));
    }
}
