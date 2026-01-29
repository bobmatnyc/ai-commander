//! EventManager - thread-safe event management with pub/sub.
//!
//! Demonstrates key Rust concurrency patterns:
//! - `Arc<RwLock<T>>` for shared read-write access to event storage
//! - `mpsc` channels for event notifications (pub/sub pattern)

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, RwLock};

use chrono::Utc;
use commander_models::{Event, EventId, EventStatus, ProjectId};
use commander_persistence::EventStore;

use crate::error::{EventError, Result};
use crate::filter::EventFilter;

/// Thread-safe event manager with pub/sub support.
///
/// # Concurrency Patterns
///
/// - **`Arc<RwLock<HashMap>>`**: Allows multiple readers OR one writer.
///   Events are read frequently (list, get) and written occasionally (emit).
///
/// - **`Arc<RwLock<Vec<Sender>>>`**: Subscriber list uses same pattern.
///   New subscribers added occasionally, broadcast happens on every emit.
///
/// - **`mpsc::channel`**: Multi-producer, single-consumer channels.
///   Each subscriber gets their own receiver; manager holds senders.
///
/// # Example
///
/// ```no_run
/// use commander_events::EventManager;
/// use commander_persistence::EventStore;
/// use commander_models::{Event, EventType};
/// use std::sync::Arc;
/// use std::thread;
///
/// let store = EventStore::new("/tmp/test");
/// let manager = Arc::new(EventManager::new(store));
///
/// // Spawn subscriber thread
/// let m = manager.clone();
/// let handle = thread::spawn(move || {
///     let rx = m.subscribe();
///     while let Ok(event) = rx.recv() {
///         println!("Received: {}", event.title);
///     }
/// });
///
/// // Emit events from main thread
/// let event = Event::new("proj-1", EventType::Status, "Hello");
/// manager.emit(event).unwrap();
/// ```
pub struct EventManager {
    /// Persistence store for events.
    store: EventStore,
    /// In-memory cache of events by ID.
    events: Arc<RwLock<HashMap<EventId, Event>>>,
    /// List of subscriber channels.
    subscribers: Arc<RwLock<Vec<Sender<Event>>>>,
}

impl EventManager {
    /// Creates a new EventManager with the given persistence store.
    pub fn new(store: EventStore) -> Self {
        Self {
            store,
            events: Arc::new(RwLock::new(HashMap::new())),
            subscribers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Loads events from a project into the in-memory cache.
    ///
    /// This should be called when starting to work with a project's events.
    pub fn load_project(&self, project_id: &ProjectId) -> Result<()> {
        let events = self.store.list_events(project_id)?;
        let mut cache = self
            .events
            .write()
            .map_err(|e| EventError::LockPoisoned(e.to_string()))?;

        for event in events {
            cache.insert(event.id.clone(), event);
        }
        Ok(())
    }

    /// Subscribes to event notifications.
    ///
    /// Returns a receiver that will receive clones of all emitted events.
    /// The receiver is disconnected when the EventManager is dropped.
    ///
    /// # Concurrency Note
    ///
    /// Uses `mpsc::channel` which is multi-producer, single-consumer.
    /// Each subscriber gets their own channel; we keep the sender.
    pub fn subscribe(&self) -> Receiver<Event> {
        let (tx, rx) = mpsc::channel();

        // Add sender to subscriber list
        if let Ok(mut subs) = self.subscribers.write() {
            subs.push(tx);
        }

        rx
    }

    /// Emits an event, persisting it and notifying subscribers.
    ///
    /// # Returns
    ///
    /// The EventId of the emitted event.
    ///
    /// # Concurrency Note
    ///
    /// 1. Acquires write lock on events to insert
    /// 2. Persists to store
    /// 3. Acquires read lock on subscribers to broadcast
    pub fn emit(&self, event: Event) -> Result<EventId> {
        let event_id = event.id.clone();

        // Persist first (crash safety)
        self.store.save_event(&event)?;

        // Add to in-memory cache
        {
            let mut events = self
                .events
                .write()
                .map_err(|e| EventError::LockPoisoned(e.to_string()))?;
            events.insert(event_id.clone(), event.clone());
        }

        // Notify subscribers (best effort - don't fail on send errors)
        self.broadcast(event);

        Ok(event_id)
    }

    /// Broadcasts an event to all subscribers.
    ///
    /// Removes any disconnected subscribers (closed receivers).
    fn broadcast(&self, event: Event) {
        if let Ok(mut subs) = self.subscribers.write() {
            // Send to all subscribers, remove disconnected ones
            subs.retain(|tx| tx.send(event.clone()).is_ok());
        }
    }

    /// Gets an event by ID.
    ///
    /// First checks the in-memory cache, then falls back to the store.
    pub fn get(&self, id: &EventId) -> Option<Event> {
        // Check cache first
        if let Ok(events) = self.events.read() {
            if let Some(event) = events.get(id) {
                return Some(event.clone());
            }
        }
        None
    }

    /// Gets an event by ID, loading from store if not in cache.
    ///
    /// This requires the project_id to load from the store.
    pub fn get_from_store(&self, project_id: &ProjectId, id: &EventId) -> Result<Option<Event>> {
        // Check cache first
        if let Ok(events) = self.events.read() {
            if let Some(event) = events.get(id) {
                return Ok(Some(event.clone()));
            }
        }

        // Try loading from store
        match self.store.load_event(project_id, id) {
            Ok(event) => {
                // Add to cache
                if let Ok(mut events) = self.events.write() {
                    events.insert(event.id.clone(), event.clone());
                }
                Ok(Some(event))
            }
            Err(commander_persistence::PersistenceError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Lists events, optionally filtered.
    ///
    /// # Arguments
    ///
    /// * `filter` - Optional filter criteria
    ///
    /// # Returns
    ///
    /// Events sorted by created_at descending (newest first).
    pub fn list(&self, filter: Option<EventFilter>) -> Vec<Event> {
        let events = match self.events.read() {
            Ok(events) => events,
            Err(_) => return Vec::new(),
        };

        let mut result: Vec<Event> = events
            .values()
            .filter(|e| {
                filter
                    .as_ref()
                    .map(|f| f.matches(e))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        // Sort by created_at descending
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        result
    }

    /// Acknowledges an event (marks it as seen but not resolved).
    ///
    /// # Arguments
    ///
    /// * `id` - Event ID to acknowledge
    ///
    /// # Returns
    ///
    /// Error if event not found or already resolved.
    pub fn acknowledge(&self, id: &EventId) -> Result<()> {
        let mut events = self
            .events
            .write()
            .map_err(|e| EventError::LockPoisoned(e.to_string()))?;

        let event = events
            .get_mut(id)
            .ok_or_else(|| EventError::NotFound(id.to_string()))?;

        if event.status == EventStatus::Resolved {
            return Err(EventError::InvalidState(
                "event already resolved".to_string(),
            ));
        }

        event.status = EventStatus::Acknowledged;

        // Persist
        self.store.save_event(event)?;

        Ok(())
    }

    /// Resolves an event with an optional response.
    ///
    /// # Arguments
    ///
    /// * `id` - Event ID to resolve
    /// * `response` - Optional response text (e.g., user decision)
    ///
    /// # Returns
    ///
    /// Error if event not found.
    pub fn resolve(&self, id: &EventId, response: Option<String>) -> Result<()> {
        let mut events = self
            .events
            .write()
            .map_err(|e| EventError::LockPoisoned(e.to_string()))?;

        let event = events
            .get_mut(id)
            .ok_or_else(|| EventError::NotFound(id.to_string()))?;

        event.status = EventStatus::Resolved;
        event.response = response;
        event.responded_at = Some(Utc::now());

        // Persist
        self.store.save_event(event)?;

        Ok(())
    }

    /// Returns the number of events in the cache.
    pub fn len(&self) -> usize {
        self.events
            .read()
            .map(|e| e.len())
            .unwrap_or(0)
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clears all events from the cache (does not affect persistence).
    pub fn clear_cache(&self) {
        if let Ok(mut events) = self.events.write() {
            events.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_models::EventType;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    fn make_manager() -> EventManager {
        let dir = tempdir().unwrap();
        // Keep the tempdir from being deleted by converting to path
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);  // Leak the tempdir handle so it's not cleaned up during test
        let store = EventStore::new(path);
        EventManager::new(store)
    }

    fn make_event(project: &str, title: &str) -> Event {
        Event::new(project, EventType::Status, title)
    }

    #[test]
    fn test_emit_and_get() {
        let manager = make_manager();
        let event = make_event("proj-1", "Test");
        let event_id = event.id.clone();

        let id = manager.emit(event).unwrap();
        assert_eq!(id, event_id);

        let retrieved = manager.get(&id).unwrap();
        assert_eq!(retrieved.title, "Test");
    }

    #[test]
    fn test_emit_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let event_id;
        let project_id;

        // Emit event
        {
            let store = EventStore::new(&path);
            let manager = EventManager::new(store);

            let event = make_event("proj-1", "Persisted");
            project_id = event.project_id.clone();
            event_id = manager.emit(event).unwrap();
        }

        // Load from fresh manager
        {
            let store = EventStore::new(&path);
            let manager = EventManager::new(store);
            manager.load_project(&project_id).unwrap();

            let retrieved = manager.get(&event_id).unwrap();
            assert_eq!(retrieved.title, "Persisted");
        }
    }

    #[test]
    fn test_subscribe_receives_events() {
        let manager = Arc::new(make_manager());

        let rx = manager.subscribe();

        let event = make_event("proj-1", "Notification");
        manager.emit(event).unwrap();

        let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(received.title, "Notification");
    }

    #[test]
    fn test_multiple_subscribers() {
        let manager = Arc::new(make_manager());

        let rx1 = manager.subscribe();
        let rx2 = manager.subscribe();

        let event = make_event("proj-1", "Broadcast");
        manager.emit(event).unwrap();

        let r1 = rx1.recv_timeout(Duration::from_secs(1)).unwrap();
        let r2 = rx2.recv_timeout(Duration::from_secs(1)).unwrap();

        assert_eq!(r1.title, "Broadcast");
        assert_eq!(r2.title, "Broadcast");
    }

    #[test]
    fn test_list_with_filter() {
        let manager = make_manager();

        manager
            .emit(make_event("proj-1", "E1"))
            .unwrap();
        manager
            .emit(make_event("proj-2", "E2"))
            .unwrap();
        manager
            .emit(make_event("proj-1", "E3"))
            .unwrap();

        let filter = EventFilter::new().with_project_id("proj-1".into());
        let events = manager.list(Some(filter));

        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.project_id.as_str() == "proj-1"));
    }

    #[test]
    fn test_list_sorted_by_created_at() {
        let manager = make_manager();

        manager.emit(make_event("p1", "First")).unwrap();
        // Small delay to ensure different timestamps
        thread::sleep(Duration::from_millis(10));
        manager.emit(make_event("p1", "Second")).unwrap();
        thread::sleep(Duration::from_millis(10));
        manager.emit(make_event("p1", "Third")).unwrap();

        let events = manager.list(None);

        // Should be newest first
        assert_eq!(events[0].title, "Third");
        assert_eq!(events[1].title, "Second");
        assert_eq!(events[2].title, "First");
    }

    #[test]
    fn test_acknowledge() {
        let manager = make_manager();

        let event = make_event("proj-1", "Ack me");
        let id = manager.emit(event).unwrap();

        manager.acknowledge(&id).unwrap();

        let retrieved = manager.get(&id).unwrap();
        assert_eq!(retrieved.status, EventStatus::Acknowledged);
    }

    #[test]
    fn test_acknowledge_resolved_fails() {
        let manager = make_manager();

        let event = make_event("proj-1", "Resolved");
        let id = manager.emit(event).unwrap();

        manager.resolve(&id, None).unwrap();

        let result = manager.acknowledge(&id);
        assert!(matches!(result, Err(EventError::InvalidState(_))));
    }

    #[test]
    fn test_resolve() {
        let manager = make_manager();

        let event = Event::new("proj-1", EventType::DecisionNeeded, "Choose");
        let id = manager.emit(event).unwrap();

        manager
            .resolve(&id, Some("Option A".to_string()))
            .unwrap();

        let retrieved = manager.get(&id).unwrap();
        assert_eq!(retrieved.status, EventStatus::Resolved);
        assert_eq!(retrieved.response, Some("Option A".to_string()));
        assert!(retrieved.responded_at.is_some());
    }

    #[test]
    fn test_get_not_found() {
        let manager = make_manager();
        let id = EventId::new();
        assert!(manager.get(&id).is_none());
    }

    #[test]
    fn test_acknowledge_not_found() {
        let manager = make_manager();
        let id = EventId::new();
        let result = manager.acknowledge(&id);
        assert!(matches!(result, Err(EventError::NotFound(_))));
    }

    #[test]
    fn test_resolve_not_found() {
        let manager = make_manager();
        let id = EventId::new();
        let result = manager.resolve(&id, None);
        assert!(matches!(result, Err(EventError::NotFound(_))));
    }

    #[test]
    fn test_thread_safe_emit() {
        let manager = Arc::new(make_manager());
        let mut handles = vec![];

        for i in 0..10 {
            let m = manager.clone();
            let handle = thread::spawn(move || {
                let event = make_event("proj-1", &format!("Event {}", i));
                m.emit(event).unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(manager.len(), 10);
    }

    #[test]
    fn test_thread_safe_read_write() {
        let manager = Arc::new(make_manager());

        // Pre-populate some events
        for i in 0..5 {
            let event = make_event("proj-1", &format!("Initial {}", i));
            manager.emit(event).unwrap();
        }

        let mut handles = vec![];

        // Readers
        for _ in 0..5 {
            let m = manager.clone();
            let handle = thread::spawn(move || {
                for _ in 0..10 {
                    let _ = m.list(None);
                    thread::sleep(Duration::from_micros(100));
                }
            });
            handles.push(handle);
        }

        // Writers
        for i in 0..5 {
            let m = manager.clone();
            let handle = thread::spawn(move || {
                for j in 0..10 {
                    let event = make_event("proj-1", &format!("New {} {}", i, j));
                    let _ = m.emit(event);
                    thread::sleep(Duration::from_micros(100));
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have initial 5 + 5 writers * 10 each = 55 events
        assert_eq!(manager.len(), 55);
    }

    #[test]
    fn test_subscriber_thread_receives() {
        let manager = Arc::new(make_manager());
        let received = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // Subscriber thread
        let rx = manager.subscribe();
        let recv_count = received.clone();
        let subscriber = thread::spawn(move || {
            while let Ok(_event) = rx.recv_timeout(Duration::from_millis(500)) {
                recv_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        });

        // Emit some events
        for i in 0..5 {
            let event = make_event("proj-1", &format!("Event {}", i));
            manager.emit(event).unwrap();
            thread::sleep(Duration::from_millis(10));
        }

        // Give subscriber time to process
        thread::sleep(Duration::from_millis(100));

        // Drop manager to close channel
        drop(manager);

        subscriber.join().unwrap();

        assert_eq!(
            received.load(std::sync::atomic::Ordering::SeqCst),
            5
        );
    }

    #[test]
    fn test_disconnected_subscriber_removed() {
        let manager = make_manager();

        // Subscribe and immediately drop receiver
        let _rx = manager.subscribe();
        drop(_rx);

        // Emit should not panic (disconnected subscriber is removed)
        let event = make_event("proj-1", "Test");
        manager.emit(event).unwrap();
    }

    #[test]
    fn test_clear_cache() {
        let manager = make_manager();

        manager.emit(make_event("p1", "E1")).unwrap();
        manager.emit(make_event("p1", "E2")).unwrap();

        assert_eq!(manager.len(), 2);

        manager.clear_cache();

        assert_eq!(manager.len(), 0);
        assert!(manager.is_empty());
    }
}
