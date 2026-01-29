//! Work store for work item persistence.

use std::fs;
use std::path::PathBuf;

use commander_models::{ProjectId, WorkId, WorkItem};

use crate::atomic::{atomic_write_json, read_json};
use crate::error::{PersistenceError, Result};

/// Manages persistence of work items.
///
/// Work items are stored as individual JSON files organized by project:
/// ```text
/// base_path/
/// └── work/
///     └── {project_id}/
///         ├── work-abc123.json
///         └── work-def456.json
/// ```
pub struct WorkStore {
    base_path: PathBuf,
}

impl WorkStore {
    /// Creates a new WorkStore with the given base path.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Returns the path to a project's work directory.
    fn work_dir(&self, project_id: &ProjectId) -> PathBuf {
        self.base_path.join("work").join(project_id.as_str())
    }

    /// Returns the path to a specific work item file.
    fn work_path(&self, project_id: &ProjectId, work_id: &WorkId) -> PathBuf {
        self.work_dir(project_id).join(format!("{}.json", work_id))
    }

    /// Ensures the work directory for a project exists.
    fn ensure_dirs(&self, project_id: &ProjectId) -> Result<()> {
        let dir = self.work_dir(project_id);
        if !dir.exists() {
            fs::create_dir_all(&dir).map_err(|source| PersistenceError::DirectoryError {
                path: dir,
                source,
            })?;
        }
        Ok(())
    }

    /// Saves a work item.
    pub fn save_work(&self, work: &WorkItem) -> Result<()> {
        self.ensure_dirs(&work.project_id)?;
        let path = self.work_path(&work.project_id, &work.id);
        atomic_write_json(&path, work)
    }

    /// Loads a work item by ID.
    pub fn load_work(&self, project_id: &ProjectId, work_id: &WorkId) -> Result<WorkItem> {
        let path = self.work_path(project_id, work_id);
        if !path.exists() {
            return Err(PersistenceError::NotFound {
                kind: "work".to_string(),
                id: work_id.to_string(),
            });
        }
        read_json(&path)
    }

    /// Lists all work items for a project.
    ///
    /// Items are sorted by priority (highest first), then by created_at (oldest first).
    pub fn list_work(&self, project_id: &ProjectId) -> Result<Vec<WorkItem>> {
        let dir = self.work_dir(project_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut items = Vec::new();
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
                match read_json::<WorkItem>(&path) {
                    Ok(item) => items.push(item),
                    Err(e) => {
                        eprintln!("Warning: failed to load work item {:?}: {}", path, e);
                    }
                }
            }
        }

        // Sort by priority descending (highest first), then by created_at ascending (oldest first)
        items.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });

        Ok(items)
    }

    /// Deletes a work item.
    pub fn delete_work(&self, project_id: &ProjectId, work_id: &WorkId) -> Result<()> {
        let path = self.work_path(project_id, work_id);
        if path.exists() {
            fs::remove_file(&path).map_err(|source| PersistenceError::WriteError { path, source })?;
        }
        Ok(())
    }

    /// Deletes all work items for a project.
    pub fn delete_project_work(&self, project_id: &ProjectId) -> Result<()> {
        let dir = self.work_dir(project_id);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|source| PersistenceError::WriteError { path: dir, source })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_models::WorkPriority;
    use tempfile::tempdir;

    fn create_test_work(project_id: &ProjectId) -> WorkItem {
        WorkItem::new(project_id.clone(), "Test work item".to_string())
    }

    #[test]
    fn test_save_and_load_work() {
        let dir = tempdir().unwrap();
        let store = WorkStore::new(dir.path());

        let project_id = ProjectId::new();
        let work = create_test_work(&project_id);
        let work_id = work.id.clone();

        store.save_work(&work).unwrap();
        let loaded = store.load_work(&project_id, &work_id).unwrap();

        assert_eq!(work.id, loaded.id);
        assert_eq!(work.content, loaded.content);
    }

    #[test]
    fn test_load_work_not_found() {
        let dir = tempdir().unwrap();
        let store = WorkStore::new(dir.path());

        let project_id = ProjectId::new();
        let work_id = WorkId::new();
        let result = store.load_work(&project_id, &work_id);

        assert!(matches!(result, Err(PersistenceError::NotFound { .. })));
    }

    #[test]
    fn test_list_work() {
        let dir = tempdir().unwrap();
        let store = WorkStore::new(dir.path());

        let project_id = ProjectId::new();
        let w1 = create_test_work(&project_id);
        let w2 = create_test_work(&project_id);

        store.save_work(&w1).unwrap();
        store.save_work(&w2).unwrap();

        let items = store.list_work(&project_id).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn test_list_work_empty_project() {
        let dir = tempdir().unwrap();
        let store = WorkStore::new(dir.path());

        let project_id = ProjectId::new();
        let items = store.list_work(&project_id).unwrap();

        assert!(items.is_empty());
    }

    #[test]
    fn test_list_work_sorted_by_priority() {
        let dir = tempdir().unwrap();
        let store = WorkStore::new(dir.path());

        let project_id = ProjectId::new();

        let low = WorkItem::with_priority(project_id.clone(), "Low", WorkPriority::Low);
        let high = WorkItem::with_priority(project_id.clone(), "High", WorkPriority::High);
        let critical = WorkItem::with_priority(project_id.clone(), "Critical", WorkPriority::Critical);

        // Save in random order
        store.save_work(&low).unwrap();
        store.save_work(&critical).unwrap();
        store.save_work(&high).unwrap();

        let items = store.list_work(&project_id).unwrap();

        assert_eq!(items.len(), 3);
        assert_eq!(items[0].priority, WorkPriority::Critical);
        assert_eq!(items[1].priority, WorkPriority::High);
        assert_eq!(items[2].priority, WorkPriority::Low);
    }

    #[test]
    fn test_delete_work() {
        let dir = tempdir().unwrap();
        let store = WorkStore::new(dir.path());

        let project_id = ProjectId::new();
        let work = create_test_work(&project_id);
        let work_id = work.id.clone();

        store.save_work(&work).unwrap();
        store.delete_work(&project_id, &work_id).unwrap();

        assert!(store.load_work(&project_id, &work_id).is_err());
    }

    #[test]
    fn test_delete_project_work() {
        let dir = tempdir().unwrap();
        let store = WorkStore::new(dir.path());

        let project_id = ProjectId::new();
        let w1 = create_test_work(&project_id);
        let w2 = create_test_work(&project_id);

        store.save_work(&w1).unwrap();
        store.save_work(&w2).unwrap();
        assert_eq!(store.list_work(&project_id).unwrap().len(), 2);

        store.delete_project_work(&project_id).unwrap();
        assert_eq!(store.list_work(&project_id).unwrap().len(), 0);
    }

    #[test]
    fn test_work_state_preserved() {
        let dir = tempdir().unwrap();
        let store = WorkStore::new(dir.path());

        let project_id = ProjectId::new();
        let mut work = create_test_work(&project_id);
        work.start();
        work.complete(Some("Done!".to_string()));
        let work_id = work.id.clone();

        store.save_work(&work).unwrap();
        let loaded = store.load_work(&project_id, &work_id).unwrap();

        assert_eq!(loaded.state, commander_models::WorkState::Completed);
        assert_eq!(loaded.result, Some("Done!".to_string()));
        assert!(loaded.started_at.is_some());
        assert!(loaded.completed_at.is_some());
    }
}
