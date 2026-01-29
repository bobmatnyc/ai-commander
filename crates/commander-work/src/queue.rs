//! WorkQueue - priority-based work queue with dependency tracking.
//!
//! Demonstrates key Rust concurrency patterns:
//! - `Arc<Mutex<T>>` for exclusive access to queue state
//! - `BinaryHeap` with custom `Ord` for priority ordering
//! - Dependency tracking using `HashSet` for O(1) lookup

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use commander_models::{ProjectId, WorkId, WorkItem, WorkPriority, WorkState};
use commander_persistence::WorkStore;

use crate::error::{Result, WorkError};
use crate::filter::WorkFilter;

/// Wrapper for WorkItem that implements custom ordering for BinaryHeap.
///
/// # Ordering Rules
///
/// 1. Higher priority comes first (Critical > High > Medium > Low)
/// 2. For same priority, older items come first (FIFO within priority)
///
/// This is inverted because BinaryHeap is a max-heap, but we want
/// the "highest" priority items (and oldest within same priority) first.
#[derive(Debug, Clone)]
struct PrioritizedWork {
    priority: WorkPriority,
    created_at: DateTime<Utc>,
    item: WorkItem,
}

impl PrioritizedWork {
    fn new(item: WorkItem) -> Self {
        Self {
            priority: item.priority,
            created_at: item.created_at,
            item,
        }
    }
}

impl PartialEq for PrioritizedWork {
    fn eq(&self, other: &Self) -> bool {
        self.item.id == other.item.id
    }
}

impl Eq for PrioritizedWork {}

impl PartialOrd for PrioritizedWork {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedWork {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => {
                // For same priority, older (smaller created_at) comes first
                // Reverse because BinaryHeap is max-heap
                other.created_at.cmp(&self.created_at)
            }
            ord => ord,
        }
    }
}

/// Internal state of the work queue.
struct QueueState {
    /// Priority queue of pending items.
    heap: BinaryHeap<PrioritizedWork>,
    /// All items by ID (for lookup and state tracking).
    items: HashMap<WorkId, WorkItem>,
    /// IDs of completed items (for dependency checking).
    completed: HashSet<WorkId>,
}

impl QueueState {
    fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            items: HashMap::new(),
            completed: HashSet::new(),
        }
    }
}

/// Thread-safe priority work queue with dependency tracking.
///
/// # Concurrency Pattern: `Arc<Mutex<T>>`
///
/// Uses `Mutex` for exclusive access because queue operations
/// (enqueue, dequeue) must be atomic - we can't have two threads
/// simultaneously modifying the heap.
///
/// # Priority Ordering
///
/// Uses `BinaryHeap` with custom `Ord` implementation:
/// - Higher priority items dequeue first
/// - Within same priority, older items dequeue first (FIFO)
///
/// # Dependency Tracking
///
/// Work items can depend on other items. An item is only "ready"
/// when all its dependencies are completed.
///
/// # Example
///
/// ```no_run
/// use commander_work::WorkQueue;
/// use commander_persistence::WorkStore;
/// use commander_models::{WorkItem, WorkPriority};
/// use std::sync::Arc;
/// use std::thread;
///
/// let store = WorkStore::new("/tmp/test");
/// let queue = Arc::new(WorkQueue::new(store));
///
/// // Multiple threads can enqueue
/// let q1 = queue.clone();
/// thread::spawn(move || {
///     let item = WorkItem::with_priority("p1", "Task A", WorkPriority::High);
///     q1.enqueue(item).unwrap();
/// });
///
/// // Single consumer dequeues
/// while let Some(work) = queue.dequeue() {
///     println!("Processing: {}", work.content);
///     queue.complete(&work.id).unwrap();
/// }
/// ```
pub struct WorkQueue {
    /// Persistence store.
    store: WorkStore,
    /// Internal queue state, protected by mutex.
    state: Arc<Mutex<QueueState>>,
}

impl WorkQueue {
    /// Creates a new WorkQueue with the given persistence store.
    pub fn new(store: WorkStore) -> Self {
        Self {
            store,
            state: Arc::new(Mutex::new(QueueState::new())),
        }
    }

    /// Loads work items from a project into the queue.
    ///
    /// Items are added based on their current state:
    /// - Pending/Queued: Added to the priority heap
    /// - Completed: Marked as completed (for dependency tracking)
    /// - InProgress/Failed/Cancelled/Blocked: Added to items map only
    pub fn load_project(&self, project_id: &ProjectId) -> Result<()> {
        let items = self.store.list_work(project_id)?;
        let mut state = self
            .state
            .lock()
            .map_err(|e| WorkError::LockPoisoned(e.to_string()))?;

        for item in items {
            let id = item.id.clone();

            match item.state {
                WorkState::Pending | WorkState::Queued => {
                    state.heap.push(PrioritizedWork::new(item.clone()));
                }
                WorkState::Completed => {
                    state.completed.insert(id.clone());
                }
                _ => {}
            }

            state.items.insert(id, item);
        }

        Ok(())
    }

    /// Adds a work item to the queue.
    ///
    /// # Returns
    ///
    /// The WorkId of the enqueued item.
    ///
    /// # Persistence
    ///
    /// Item is persisted before being added to the queue.
    pub fn enqueue(&self, mut item: WorkItem) -> Result<WorkId> {
        let work_id = item.id.clone();

        // Mark as queued
        item.state = WorkState::Queued;

        // Persist first (crash safety)
        self.store.save_work(&item)?;

        // Add to queue
        let mut state = self
            .state
            .lock()
            .map_err(|e| WorkError::LockPoisoned(e.to_string()))?;

        state.items.insert(work_id.clone(), item.clone());
        state.heap.push(PrioritizedWork::new(item));

        Ok(work_id)
    }

    /// Removes and returns the highest-priority ready item.
    ///
    /// An item is "ready" when:
    /// - It's in the queue (Pending or Queued state)
    /// - All its dependencies are completed
    ///
    /// The returned item is marked as InProgress.
    ///
    /// # Returns
    ///
    /// `None` if no ready items are available.
    pub fn dequeue(&self) -> Option<WorkItem> {
        let mut state = self
            .state
            .lock()
            .ok()?;

        // Collect items to re-queue (blocked items)
        let mut blocked = Vec::new();

        while let Some(pw) = state.heap.pop() {
            // Check if item can start (dependencies met)
            if pw.item.can_start(&state.completed) {
                // Mark as in progress
                let mut item = pw.item;
                item.start();

                // Update in items map
                state.items.insert(item.id.clone(), item.clone());

                // Persist state change
                let _ = self.store.save_work(&item);

                // Re-queue blocked items
                for blocked_pw in blocked {
                    state.heap.push(blocked_pw);
                }

                return Some(item);
            }

            // Item is blocked, save for later
            blocked.push(pw);
        }

        // Re-queue all blocked items (no ready items found)
        for blocked_pw in blocked {
            state.heap.push(blocked_pw);
        }

        None
    }

    /// Returns a reference to the highest-priority ready item without removing it.
    ///
    /// Note: Returns a clone since we can't hold a reference across lock.
    pub fn peek(&self) -> Option<WorkItem> {
        let state = self.state.lock().ok()?;

        // Find first ready item
        for pw in state.heap.iter() {
            if pw.item.can_start(&state.completed) {
                return Some(pw.item.clone());
            }
        }

        None
    }

    /// Gets a work item by ID.
    pub fn get(&self, id: &WorkId) -> Option<WorkItem> {
        let state = self.state.lock().ok()?;
        state.items.get(id).cloned()
    }

    /// Marks a work item as completed.
    ///
    /// This:
    /// - Updates the item state to Completed
    /// - Adds the ID to completed set (unblocks dependents)
    /// - Persists the change
    pub fn complete(&self, id: &WorkId) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| WorkError::LockPoisoned(e.to_string()))?;

        // Check state first
        {
            let item = state
                .items
                .get(id)
                .ok_or_else(|| WorkError::NotFound(id.to_string()))?;

            if item.state != WorkState::InProgress {
                return Err(WorkError::InvalidState(format!(
                    "cannot complete item in {:?} state",
                    item.state
                )));
            }
        }

        // Now update - get mutable reference and modify
        let item = state.items.get_mut(id).unwrap();
        item.complete(None);
        let item_clone = item.clone();

        // Update completed set
        state.completed.insert(id.clone());

        // Persist (drop lock by consuming state or clone the item first)
        drop(state);
        self.store.save_work(&item_clone)?;

        Ok(())
    }

    /// Marks a work item as completed with a result.
    pub fn complete_with_result(&self, id: &WorkId, result: String) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| WorkError::LockPoisoned(e.to_string()))?;

        // Check state first
        {
            let item = state
                .items
                .get(id)
                .ok_or_else(|| WorkError::NotFound(id.to_string()))?;

            if item.state != WorkState::InProgress {
                return Err(WorkError::InvalidState(format!(
                    "cannot complete item in {:?} state",
                    item.state
                )));
            }
        }

        // Now update - get mutable reference and modify
        let item = state.items.get_mut(id).unwrap();
        item.complete(Some(result));
        let item_clone = item.clone();

        // Update completed set
        state.completed.insert(id.clone());

        // Persist
        drop(state);
        self.store.save_work(&item_clone)?;

        Ok(())
    }

    /// Marks a work item as failed.
    ///
    /// Failed items do NOT unblock dependents.
    pub fn fail(&self, id: &WorkId, error: String) -> Result<()> {
        let item_clone = {
            let mut state = self
                .state
                .lock()
                .map_err(|e| WorkError::LockPoisoned(e.to_string()))?;

            let item = state
                .items
                .get_mut(id)
                .ok_or_else(|| WorkError::NotFound(id.to_string()))?;

            if item.state != WorkState::InProgress {
                return Err(WorkError::InvalidState(format!(
                    "cannot fail item in {:?} state",
                    item.state
                )));
            }

            item.fail(error);

            item.clone()
        };

        // Persist outside the lock
        self.store.save_work(&item_clone)?;

        Ok(())
    }

    /// Lists work items, optionally filtered.
    ///
    /// # Returns
    ///
    /// Items sorted by priority (highest first), then by created_at (oldest first).
    pub fn list(&self, filter: Option<WorkFilter>) -> Vec<WorkItem> {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let mut items: Vec<WorkItem> = state
            .items
            .values()
            .filter(|item| {
                filter
                    .as_ref()
                    .map(|f| f.matches(item))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        // Sort by priority descending, then created_at ascending
        items.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });

        items
    }

    /// Returns all items that are ready to start (dependencies met).
    ///
    /// # Returns
    ///
    /// Ready items sorted by priority (highest first).
    pub fn ready_items(&self) -> Vec<WorkItem> {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let mut ready: Vec<WorkItem> = state
            .heap
            .iter()
            .filter(|pw| pw.item.can_start(&state.completed))
            .map(|pw| pw.item.clone())
            .collect();

        // Sort by priority descending, then created_at ascending
        ready.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });

        ready
    }

    /// Returns the number of items in the queue (all states).
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .map(|s| s.items.len())
            .unwrap_or(0)
    }

    /// Returns true if the queue has no items.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of pending items in the heap.
    pub fn pending_count(&self) -> usize {
        self.state
            .lock()
            .map(|s| s.heap.len())
            .unwrap_or(0)
    }

    /// Returns the number of completed items.
    pub fn completed_count(&self) -> usize {
        self.state
            .lock()
            .map(|s| s.completed.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    fn make_queue() -> WorkQueue {
        let dir = tempdir().unwrap();
        // Keep the tempdir from being deleted by converting to path
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);  // Leak the tempdir handle so it's not cleaned up during test
        let store = WorkStore::new(path);
        WorkQueue::new(store)
    }

    fn make_item(project: &str, content: &str) -> WorkItem {
        WorkItem::new(project, content)
    }

    #[test]
    fn test_enqueue_and_dequeue() {
        let queue = make_queue();

        let item = make_item("proj-1", "Task");
        let id = queue.enqueue(item).unwrap();

        let dequeued = queue.dequeue().unwrap();
        assert_eq!(dequeued.id, id);
        assert_eq!(dequeued.state, WorkState::InProgress);
    }

    #[test]
    fn test_dequeue_empty() {
        let queue = make_queue();
        assert!(queue.dequeue().is_none());
    }

    #[test]
    fn test_priority_ordering() {
        let queue = make_queue();

        // Enqueue in reverse priority order
        queue
            .enqueue(WorkItem::with_priority("p1", "Low", WorkPriority::Low))
            .unwrap();
        queue
            .enqueue(WorkItem::with_priority("p1", "Critical", WorkPriority::Critical))
            .unwrap();
        queue
            .enqueue(WorkItem::with_priority("p1", "High", WorkPriority::High))
            .unwrap();
        queue
            .enqueue(WorkItem::with_priority("p1", "Medium", WorkPriority::Medium))
            .unwrap();

        // Dequeue should be in priority order
        assert_eq!(queue.dequeue().unwrap().content, "Critical");
        assert_eq!(queue.dequeue().unwrap().content, "High");
        assert_eq!(queue.dequeue().unwrap().content, "Medium");
        assert_eq!(queue.dequeue().unwrap().content, "Low");
    }

    #[test]
    fn test_fifo_within_priority() {
        let queue = make_queue();

        // Enqueue multiple items with same priority
        queue
            .enqueue(make_item("p1", "First"))
            .unwrap();
        thread::sleep(Duration::from_millis(10));
        queue
            .enqueue(make_item("p1", "Second"))
            .unwrap();
        thread::sleep(Duration::from_millis(10));
        queue
            .enqueue(make_item("p1", "Third"))
            .unwrap();

        // Should dequeue in FIFO order
        assert_eq!(queue.dequeue().unwrap().content, "First");
        assert_eq!(queue.dequeue().unwrap().content, "Second");
        assert_eq!(queue.dequeue().unwrap().content, "Third");
    }

    #[test]
    fn test_dependency_blocking() {
        let queue = make_queue();

        // Enqueue dep first
        let dep = make_item("p1", "Dependency");
        let dep_id = queue.enqueue(dep).unwrap();

        // Enqueue item that depends on it
        let mut item = WorkItem::with_priority("p1", "Dependent", WorkPriority::Critical);
        item.depends_on = vec![dep_id.clone()];
        queue.enqueue(item).unwrap();

        // Dequeue should return the dependency first (even though dependent has higher priority)
        let first = queue.dequeue().unwrap();
        assert_eq!(first.content, "Dependency");

        // Complete the dependency
        queue.complete(&first.id).unwrap();

        // Now the dependent should be ready
        let second = queue.dequeue().unwrap();
        assert_eq!(second.content, "Dependent");
    }

    #[test]
    fn test_multiple_dependencies() {
        let queue = make_queue();

        // Enqueue two dependencies
        let dep1 = queue.enqueue(make_item("p1", "Dep1")).unwrap();
        let dep2 = queue.enqueue(make_item("p1", "Dep2")).unwrap();

        // Enqueue item that depends on both
        let mut item = make_item("p1", "Dependent");
        item.depends_on = vec![dep1.clone(), dep2.clone()];
        queue.enqueue(item).unwrap();

        // Dequeue and complete first dep
        let d1 = queue.dequeue().unwrap();
        queue.complete(&d1.id).unwrap();

        // Dependent still blocked (waiting for dep2)
        let d2 = queue.dequeue().unwrap();
        assert!(d2.content.starts_with("Dep")); // Should be Dep2

        // Complete second dep
        queue.complete(&d2.id).unwrap();

        // Now dependent is ready
        let dependent = queue.dequeue().unwrap();
        assert_eq!(dependent.content, "Dependent");
    }

    #[test]
    fn test_get() {
        let queue = make_queue();

        let item = make_item("p1", "Task");
        let id = queue.enqueue(item).unwrap();

        let retrieved = queue.get(&id).unwrap();
        assert_eq!(retrieved.content, "Task");
    }

    #[test]
    fn test_get_not_found() {
        let queue = make_queue();
        let id = WorkId::new();
        assert!(queue.get(&id).is_none());
    }

    #[test]
    fn test_complete() {
        let queue = make_queue();

        let item = make_item("p1", "Task");
        queue.enqueue(item).unwrap();

        let dequeued = queue.dequeue().unwrap();
        queue.complete(&dequeued.id).unwrap();

        let completed = queue.get(&dequeued.id).unwrap();
        assert_eq!(completed.state, WorkState::Completed);
        assert!(completed.completed_at.is_some());
    }

    #[test]
    fn test_complete_wrong_state() {
        let queue = make_queue();

        let item = make_item("p1", "Task");
        let id = queue.enqueue(item).unwrap();

        // Try to complete without dequeuing (item is Queued, not InProgress)
        let result = queue.complete(&id);
        assert!(matches!(result, Err(WorkError::InvalidState(_))));
    }

    #[test]
    fn test_fail() {
        let queue = make_queue();

        let item = make_item("p1", "Task");
        queue.enqueue(item).unwrap();

        let dequeued = queue.dequeue().unwrap();
        queue
            .fail(&dequeued.id, "Something broke".to_string())
            .unwrap();

        let failed = queue.get(&dequeued.id).unwrap();
        assert_eq!(failed.state, WorkState::Failed);
        assert_eq!(failed.error, Some("Something broke".to_string()));
    }

    #[test]
    fn test_fail_does_not_unblock() {
        let queue = make_queue();

        // Enqueue dep and dependent
        let dep = make_item("p1", "Dep");
        let dep_id = queue.enqueue(dep).unwrap();

        let mut item = make_item("p1", "Dependent");
        item.depends_on = vec![dep_id];
        queue.enqueue(item).unwrap();

        // Fail the dependency
        let dequeued = queue.dequeue().unwrap();
        queue.fail(&dequeued.id, "Failed".to_string()).unwrap();

        // Dependent should still be blocked
        assert!(queue.dequeue().is_none());
    }

    #[test]
    fn test_list() {
        let queue = make_queue();

        queue.enqueue(make_item("p1", "A")).unwrap();
        queue.enqueue(make_item("p1", "B")).unwrap();
        queue.enqueue(make_item("p2", "C")).unwrap();

        let all = queue.list(None);
        assert_eq!(all.len(), 3);

        let p1_only = queue.list(Some(WorkFilter::new().with_project_id("p1".into())));
        assert_eq!(p1_only.len(), 2);
    }

    #[test]
    fn test_ready_items() {
        let queue = make_queue();

        // Ready item
        queue.enqueue(make_item("p1", "Ready")).unwrap();

        // Blocked item
        let dep_id = WorkId::from("non-existent");
        let mut blocked = make_item("p1", "Blocked");
        blocked.depends_on = vec![dep_id];
        queue.enqueue(blocked).unwrap();

        let ready = queue.ready_items();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].content, "Ready");
    }

    #[test]
    fn test_peek() {
        let queue = make_queue();

        queue
            .enqueue(WorkItem::with_priority("p1", "Low", WorkPriority::Low))
            .unwrap();
        queue
            .enqueue(WorkItem::with_priority("p1", "High", WorkPriority::High))
            .unwrap();

        // Peek should return highest priority
        let peeked = queue.peek().unwrap();
        assert_eq!(peeked.content, "High");

        // Peek again should return same item (not removed)
        let peeked2 = queue.peek().unwrap();
        assert_eq!(peeked2.id, peeked.id);
    }

    #[test]
    fn test_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let work_id;
        let project_id;

        // Enqueue and complete
        {
            let store = WorkStore::new(&path);
            let queue = WorkQueue::new(store);

            let item = make_item("proj-1", "Persisted");
            project_id = item.project_id.clone();
            work_id = queue.enqueue(item).unwrap();

            let dequeued = queue.dequeue().unwrap();
            queue.complete(&dequeued.id).unwrap();
        }

        // Load from fresh queue
        {
            let store = WorkStore::new(&path);
            let queue = WorkQueue::new(store);
            queue.load_project(&project_id).unwrap();

            let item = queue.get(&work_id).unwrap();
            assert_eq!(item.state, WorkState::Completed);
        }
    }

    #[test]
    fn test_thread_safe_enqueue() {
        let queue = Arc::new(make_queue());
        let mut handles = vec![];

        for i in 0..10 {
            let q = queue.clone();
            let handle = thread::spawn(move || {
                let item = make_item("p1", &format!("Task {}", i));
                q.enqueue(item).unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(queue.len(), 10);
    }

    #[test]
    fn test_thread_safe_enqueue_dequeue() {
        let queue = Arc::new(make_queue());
        let processed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // Enqueue items
        for i in 0..20 {
            let item = make_item("p1", &format!("Task {}", i));
            queue.enqueue(item).unwrap();
        }

        let mut handles = vec![];

        // Multiple consumer threads
        for _ in 0..4 {
            let q = queue.clone();
            let p = processed.clone();
            let handle = thread::spawn(move || {
                loop {
                    if let Some(item) = q.dequeue() {
                        // Simulate work
                        thread::sleep(Duration::from_micros(100));
                        q.complete(&item.id).unwrap();
                        p.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    } else {
                        break;
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(
            processed.load(std::sync::atomic::Ordering::SeqCst),
            20
        );
        assert_eq!(queue.completed_count(), 20);
    }

    #[test]
    fn test_concurrent_enqueue_dequeue() {
        let queue = Arc::new(make_queue());
        let enqueued = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let processed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let mut handles = vec![];

        // Producer threads
        for i in 0..4 {
            let q = queue.clone();
            let e = enqueued.clone();
            let handle = thread::spawn(move || {
                for j in 0..10 {
                    let item = make_item("p1", &format!("T{}-{}", i, j));
                    q.enqueue(item).unwrap();
                    e.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    thread::sleep(Duration::from_micros(50));
                }
            });
            handles.push(handle);
        }

        // Consumer threads
        for _ in 0..2 {
            let q = queue.clone();
            let p = processed.clone();
            let e = enqueued.clone();
            let handle = thread::spawn(move || {
                let mut retries = 0;
                loop {
                    if let Some(item) = q.dequeue() {
                        q.complete(&item.id).unwrap();
                        p.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        retries = 0;
                    } else {
                        retries += 1;
                        // Stop if we've tried many times and all items enqueued
                        if retries > 10 && e.load(std::sync::atomic::Ordering::SeqCst) >= 40 {
                            break;
                        }
                        thread::sleep(Duration::from_micros(100));
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // All items should be processed
        assert_eq!(
            processed.load(std::sync::atomic::Ordering::SeqCst),
            40
        );
    }

    #[test]
    fn test_len_and_is_empty() {
        let queue = make_queue();

        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);

        queue.enqueue(make_item("p1", "A")).unwrap();
        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 1);

        queue.enqueue(make_item("p1", "B")).unwrap();
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_pending_count() {
        let queue = make_queue();

        queue.enqueue(make_item("p1", "A")).unwrap();
        queue.enqueue(make_item("p1", "B")).unwrap();
        assert_eq!(queue.pending_count(), 2);

        queue.dequeue().unwrap();
        assert_eq!(queue.pending_count(), 1);
    }

    #[test]
    fn test_complete_with_result() {
        let queue = make_queue();

        let item = make_item("p1", "Task");
        queue.enqueue(item).unwrap();

        let dequeued = queue.dequeue().unwrap();
        queue
            .complete_with_result(&dequeued.id, "Success!".to_string())
            .unwrap();

        let completed = queue.get(&dequeued.id).unwrap();
        assert_eq!(completed.result, Some("Success!".to_string()));
    }
}
