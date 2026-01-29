//! State store for project persistence.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use commander_models::{Project, ProjectId};

use crate::atomic::{atomic_write_json, read_json, read_json_optional};
use crate::error::{PersistenceError, Result};

/// Manages persistence of project state.
///
/// Projects are stored as individual JSON files in a directory structure:
/// ```text
/// base_path/
/// └── projects/
///     ├── proj-abc123.json
///     └── proj-def456.json
/// ```
pub struct StateStore {
    base_path: PathBuf,
}

impl StateStore {
    /// Creates a new StateStore with the given base path.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Returns the path to the projects directory.
    fn projects_dir(&self) -> PathBuf {
        self.base_path.join("projects")
    }

    /// Returns the path to a specific project file.
    fn project_path(&self, id: &ProjectId) -> PathBuf {
        self.projects_dir().join(format!("{}.json", id))
    }

    /// Ensures the projects directory exists.
    fn ensure_dirs(&self) -> Result<()> {
        let dir = self.projects_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir).map_err(|source| PersistenceError::DirectoryError {
                path: dir,
                source,
            })?;
        }
        Ok(())
    }

    /// Saves a project to disk.
    pub fn save_project(&self, project: &Project) -> Result<()> {
        self.ensure_dirs()?;
        let path = self.project_path(&project.id);
        atomic_write_json(&path, project)
    }

    /// Loads a project by ID.
    pub fn load_project(&self, id: &ProjectId) -> Result<Project> {
        let path = self.project_path(id);
        if !path.exists() {
            return Err(PersistenceError::NotFound {
                kind: "project".to_string(),
                id: id.to_string(),
            });
        }
        read_json(&path)
    }

    /// Loads a project by ID, returning None if it doesn't exist.
    pub fn load_project_optional(&self, id: &ProjectId) -> Result<Option<Project>> {
        let path = self.project_path(id);
        read_json_optional(&path)
    }

    /// Lists all project IDs.
    pub fn list_project_ids(&self) -> Result<Vec<ProjectId>> {
        let dir = self.projects_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::new();
        let entries = fs::read_dir(&dir).map_err(|source| PersistenceError::ReadError {
            path: dir.clone(),
            source,
        })?;

        for entry in entries {
            let entry = entry.map_err(|source| PersistenceError::ReadError {
                path: dir.clone(),
                source,
            })?;

            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.push(ProjectId::from(stem));
                }
            }
        }

        Ok(ids)
    }

    /// Loads all projects.
    pub fn load_all_projects(&self) -> Result<HashMap<ProjectId, Project>> {
        let ids = self.list_project_ids()?;
        let mut projects = HashMap::new();

        for id in ids {
            match self.load_project(&id) {
                Ok(project) => {
                    projects.insert(id, project);
                }
                Err(e) => {
                    // Log warning but continue loading other projects
                    eprintln!("Warning: failed to load project {}: {}", id, e);
                }
            }
        }

        Ok(projects)
    }

    /// Deletes a project.
    pub fn delete_project(&self, id: &ProjectId) -> Result<()> {
        let path = self.project_path(id);
        if path.exists() {
            fs::remove_file(&path).map_err(|source| PersistenceError::WriteError { path, source })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_models::ProjectState;
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load_project() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path());

        let project = Project::new("/path/to/project".to_string(), "my-project".to_string());
        let id = project.id.clone();

        store.save_project(&project).unwrap();
        let loaded = store.load_project(&id).unwrap();

        assert_eq!(project.id, loaded.id);
        assert_eq!(project.name, loaded.name);
        assert_eq!(project.path, loaded.path);
    }

    #[test]
    fn test_load_project_not_found() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path());

        let id = ProjectId::from("nonexistent");
        let result = store.load_project(&id);

        assert!(matches!(result, Err(PersistenceError::NotFound { .. })));
    }

    #[test]
    fn test_load_project_optional() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path());

        let id = ProjectId::from("nonexistent");
        let result = store.load_project_optional(&id).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_list_project_ids() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path());

        // Save two projects
        let p1 = Project::new("/path/a".to_string(), "project-a".to_string());
        let p2 = Project::new("/path/b".to_string(), "project-b".to_string());

        store.save_project(&p1).unwrap();
        store.save_project(&p2).unwrap();

        let ids = store.list_project_ids().unwrap();

        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&p1.id));
        assert!(ids.contains(&p2.id));
    }

    #[test]
    fn test_load_all_projects() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path());

        let p1 = Project::new("/path/a".to_string(), "project-a".to_string());
        let p2 = Project::new("/path/b".to_string(), "project-b".to_string());

        store.save_project(&p1).unwrap();
        store.save_project(&p2).unwrap();

        let projects = store.load_all_projects().unwrap();

        assert_eq!(projects.len(), 2);
        assert!(projects.contains_key(&p1.id));
        assert!(projects.contains_key(&p2.id));
    }

    #[test]
    fn test_delete_project() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path());

        let project = Project::new("/path".to_string(), "test".to_string());
        let id = project.id.clone();

        store.save_project(&project).unwrap();
        assert!(store.load_project(&id).is_ok());

        store.delete_project(&id).unwrap();
        assert!(store.load_project(&id).is_err());
    }

    #[test]
    fn test_project_state_preserved() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path());

        let mut project = Project::new("/path".to_string(), "test".to_string());
        project.set_state(ProjectState::Working, Some("Processing".to_string()));
        let id = project.id.clone();

        store.save_project(&project).unwrap();
        let loaded = store.load_project(&id).unwrap();

        assert_eq!(loaded.state, ProjectState::Working);
        assert_eq!(loaded.state_reason, Some("Processing".to_string()));
    }
}
