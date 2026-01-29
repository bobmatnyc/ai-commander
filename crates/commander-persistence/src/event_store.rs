//! Event store for event persistence.

use std::fs;
use std::path::PathBuf;

use commander_models::{Event, EventId, ProjectId};

use crate::atomic::{atomic_write_json, read_json};
use crate::error::{PersistenceError, Result};

/// Manages persistence of events.
///
/// Events are stored as individual JSON files organized by project:
/// ```text
/// base_path/
/// └── events/
///     └── {project_id}/
///         ├── evt-abc123.json
///         └── evt-def456.json
/// ```
pub struct EventStore {
    base_path: PathBuf,
}

impl EventStore {
    /// Creates a new EventStore with the given base path.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Returns the path to a project's events directory.
    fn events_dir(&self, project_id: &ProjectId) -> PathBuf {
        self.base_path.join("events").join(project_id.as_str())
    }

    /// Returns the path to a specific event file.
    fn event_path(&self, project_id: &ProjectId, event_id: &EventId) -> PathBuf {
        self.events_dir(project_id)
            .join(format!("{}.json", event_id))
    }

    /// Ensures the events directory for a project exists.
    fn ensure_dirs(&self, project_id: &ProjectId) -> Result<()> {
        let dir = self.events_dir(project_id);
        if !dir.exists() {
            fs::create_dir_all(&dir).map_err(|source| PersistenceError::DirectoryError {
                path: dir,
                source,
            })?;
        }
        Ok(())
    }

    /// Saves an event.
    pub fn save_event(&self, event: &Event) -> Result<()> {
        self.ensure_dirs(&event.project_id)?;
        let path = self.event_path(&event.project_id, &event.id);
        atomic_write_json(&path, event)
    }

    /// Loads an event by ID.
    pub fn load_event(&self, project_id: &ProjectId, event_id: &EventId) -> Result<Event> {
        let path = self.event_path(project_id, event_id);
        if !path.exists() {
            return Err(PersistenceError::NotFound {
                kind: "event".to_string(),
                id: event_id.to_string(),
            });
        }
        read_json(&path)
    }

    /// Lists all events for a project.
    pub fn list_events(&self, project_id: &ProjectId) -> Result<Vec<Event>> {
        let dir = self.events_dir(project_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut events = Vec::new();
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
                match read_json::<Event>(&path) {
                    Ok(event) => events.push(event),
                    Err(e) => {
                        eprintln!("Warning: failed to load event {:?}: {}", path, e);
                    }
                }
            }
        }

        // Sort by created_at descending (newest first)
        events.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(events)
    }

    /// Deletes an event.
    pub fn delete_event(&self, project_id: &ProjectId, event_id: &EventId) -> Result<()> {
        let path = self.event_path(project_id, event_id);
        if path.exists() {
            fs::remove_file(&path).map_err(|source| PersistenceError::WriteError { path, source })?;
        }
        Ok(())
    }

    /// Deletes all events for a project.
    pub fn delete_project_events(&self, project_id: &ProjectId) -> Result<()> {
        let dir = self.events_dir(project_id);
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
    use commander_models::EventType;
    use tempfile::tempdir;

    fn create_test_event(project_id: &ProjectId) -> Event {
        Event::new(project_id.clone(), EventType::Status, "Test event".to_string())
    }

    #[test]
    fn test_save_and_load_event() {
        let dir = tempdir().unwrap();
        let store = EventStore::new(dir.path());

        let project_id = ProjectId::new();
        let event = create_test_event(&project_id);
        let event_id = event.id.clone();

        store.save_event(&event).unwrap();
        let loaded = store.load_event(&project_id, &event_id).unwrap();

        assert_eq!(event.id, loaded.id);
        assert_eq!(event.title, loaded.title);
    }

    #[test]
    fn test_load_event_not_found() {
        let dir = tempdir().unwrap();
        let store = EventStore::new(dir.path());

        let project_id = ProjectId::new();
        let event_id = EventId::new();
        let result = store.load_event(&project_id, &event_id);

        assert!(matches!(result, Err(PersistenceError::NotFound { .. })));
    }

    #[test]
    fn test_list_events() {
        let dir = tempdir().unwrap();
        let store = EventStore::new(dir.path());

        let project_id = ProjectId::new();
        let e1 = create_test_event(&project_id);
        let e2 = create_test_event(&project_id);

        store.save_event(&e1).unwrap();
        store.save_event(&e2).unwrap();

        let events = store.list_events(&project_id).unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_list_events_empty_project() {
        let dir = tempdir().unwrap();
        let store = EventStore::new(dir.path());

        let project_id = ProjectId::new();
        let events = store.list_events(&project_id).unwrap();

        assert!(events.is_empty());
    }

    #[test]
    fn test_delete_event() {
        let dir = tempdir().unwrap();
        let store = EventStore::new(dir.path());

        let project_id = ProjectId::new();
        let event = create_test_event(&project_id);
        let event_id = event.id.clone();

        store.save_event(&event).unwrap();
        store.delete_event(&project_id, &event_id).unwrap();

        assert!(store.load_event(&project_id, &event_id).is_err());
    }

    #[test]
    fn test_delete_project_events() {
        let dir = tempdir().unwrap();
        let store = EventStore::new(dir.path());

        let project_id = ProjectId::new();
        let e1 = create_test_event(&project_id);
        let e2 = create_test_event(&project_id);

        store.save_event(&e1).unwrap();
        store.save_event(&e2).unwrap();
        assert_eq!(store.list_events(&project_id).unwrap().len(), 2);

        store.delete_project_events(&project_id).unwrap();
        assert_eq!(store.list_events(&project_id).unwrap().len(), 0);
    }
}
