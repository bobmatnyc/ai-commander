//! Work item types for Commander.
//!
//! Work items represent units of work that can be queued, processed,
//! and tracked through their lifecycle.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::ids::{ProjectId, WorkId};

/// State of a work item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    /// Work is waiting to be processed.
    #[default]
    Pending,
    /// Work has been queued for processing.
    Queued,
    /// Work is currently being processed.
    InProgress,
    /// Work is blocked by dependencies or other issues.
    Blocked,
    /// Work has been completed successfully.
    Completed,
    /// Work has failed.
    Failed,
    /// Work has been cancelled.
    Cancelled,
}

/// Priority levels for work items.
///
/// Higher numeric value = higher priority.
/// Critical (4) > High (3) > Medium (2) > Low (1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkPriority {
    /// Low priority (1).
    Low,
    /// Medium priority (2).
    #[default]
    Medium,
    /// High priority (3).
    High,
    /// Critical priority (4).
    Critical,
}

impl WorkPriority {
    /// Returns the numeric value of this priority.
    /// Higher value = higher priority.
    pub fn as_value(&self) -> u8 {
        match self {
            WorkPriority::Low => 1,
            WorkPriority::Medium => 2,
            WorkPriority::High => 3,
            WorkPriority::Critical => 4,
        }
    }
}

impl PartialOrd for WorkPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WorkPriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_value().cmp(&other.as_value())
    }
}

/// A unit of work in the Commander system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    /// Unique identifier for the work item.
    pub id: WorkId,

    /// ID of the project this work item belongs to.
    pub project_id: ProjectId,

    /// Description of the work to be done.
    pub content: String,

    /// Current state of the work item.
    pub state: WorkState,

    /// Priority level of the work item.
    pub priority: WorkPriority,

    /// When the work item was created.
    pub created_at: DateTime<Utc>,

    /// When the work item was started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,

    /// When the work item was completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,

    /// Result of the work if completed successfully.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,

    /// Error message if the work failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// IDs of work items that this item depends on.
    #[serde(default)]
    pub depends_on: Vec<WorkId>,

    /// Additional metadata for the work item.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl WorkItem {
    /// Creates a new work item with the given parameters.
    pub fn new(project_id: impl Into<ProjectId>, content: impl Into<String>) -> Self {
        Self {
            id: WorkId::new(),
            project_id: project_id.into(),
            content: content.into(),
            state: WorkState::Pending,
            priority: WorkPriority::Medium,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
            depends_on: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Creates a new work item with the specified priority.
    pub fn with_priority(project_id: impl Into<ProjectId>, content: impl Into<String>, priority: WorkPriority) -> Self {
        let mut item = Self::new(project_id, content);
        item.priority = priority;
        item
    }

    /// Checks if this work item can start based on completed dependencies.
    ///
    /// Returns true if:
    /// - The item has no dependencies, or
    /// - All dependencies are in the completed_ids set
    ///
    /// # Arguments
    /// * `completed_ids` - Set of IDs of completed work items
    pub fn can_start(&self, completed_ids: &HashSet<WorkId>) -> bool {
        if self.depends_on.is_empty() {
            return true;
        }

        self.depends_on.iter().all(|dep| completed_ids.contains(dep))
    }

    /// Marks the work item as started.
    pub fn start(&mut self) {
        self.state = WorkState::InProgress;
        self.started_at = Some(Utc::now());
    }

    /// Marks the work item as completed with the given result.
    pub fn complete(&mut self, result: Option<String>) {
        self.state = WorkState::Completed;
        self.completed_at = Some(Utc::now());
        self.result = result;
    }

    /// Marks the work item as failed with the given error.
    pub fn fail(&mut self, error: String) {
        self.state = WorkState::Failed;
        self.completed_at = Some(Utc::now());
        self.error = Some(error);
    }

    /// Marks the work item as cancelled.
    pub fn cancel(&mut self) {
        self.state = WorkState::Cancelled;
        self.completed_at = Some(Utc::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_work_priority_ordering() {
        assert!(WorkPriority::Critical > WorkPriority::High);
        assert!(WorkPriority::High > WorkPriority::Medium);
        assert!(WorkPriority::Medium > WorkPriority::Low);
    }

    #[test]
    fn test_work_priority_values() {
        assert_eq!(WorkPriority::Low.as_value(), 1);
        assert_eq!(WorkPriority::Medium.as_value(), 2);
        assert_eq!(WorkPriority::High.as_value(), 3);
        assert_eq!(WorkPriority::Critical.as_value(), 4);
    }

    #[test]
    fn test_work_priority_equality() {
        assert_eq!(WorkPriority::Critical, WorkPriority::Critical);
        assert_ne!(WorkPriority::Critical, WorkPriority::High);
    }

    #[test]
    fn test_work_item_creation() {
        let item = WorkItem::new("project-1", "Do something");

        assert!(item.id.as_str().starts_with("work-"));
        assert_eq!(item.project_id.as_str(), "project-1");
        assert_eq!(item.content, "Do something");
        assert_eq!(item.state, WorkState::Pending);
        assert_eq!(item.priority, WorkPriority::Medium);
        assert!(item.depends_on.is_empty());
    }

    #[test]
    fn test_work_item_with_priority() {
        let item = WorkItem::with_priority(
            "project-1",
            "Urgent task",
            WorkPriority::Critical,
        );

        assert_eq!(item.priority, WorkPriority::Critical);
    }

    #[test]
    fn test_can_start_no_dependencies() {
        let item = WorkItem::new("p1", "Task");
        let completed: HashSet<WorkId> = HashSet::new();

        assert!(item.can_start(&completed));
    }

    #[test]
    fn test_can_start_dependencies_met() {
        let mut item = WorkItem::new("p1", "Task");
        item.depends_on = vec![WorkId::from("dep1"), WorkId::from("dep2")];

        let mut completed: HashSet<WorkId> = HashSet::new();
        completed.insert(WorkId::from("dep1"));
        completed.insert(WorkId::from("dep2"));

        assert!(item.can_start(&completed));
    }

    #[test]
    fn test_can_start_dependencies_not_met() {
        let mut item = WorkItem::new("p1", "Task");
        item.depends_on = vec![WorkId::from("dep1"), WorkId::from("dep2")];

        let mut completed: HashSet<WorkId> = HashSet::new();
        completed.insert(WorkId::from("dep1"));
        // dep2 not completed

        assert!(!item.can_start(&completed));
    }

    #[test]
    fn test_can_start_partial_dependencies() {
        let mut item = WorkItem::new("p1", "Task");
        item.depends_on = vec![WorkId::from("dep1"), WorkId::from("dep2"), WorkId::from("dep3")];

        let mut completed: HashSet<WorkId> = HashSet::new();
        completed.insert(WorkId::from("dep1"));
        completed.insert(WorkId::from("dep3"));
        // dep2 not completed

        assert!(!item.can_start(&completed));
    }

    #[test]
    fn test_can_start_extra_completed() {
        let mut item = WorkItem::new("p1", "Task");
        item.depends_on = vec![WorkId::from("dep1")];

        let mut completed: HashSet<WorkId> = HashSet::new();
        completed.insert(WorkId::from("dep1"));
        completed.insert(WorkId::from("dep2")); // Extra completed item
        completed.insert(WorkId::from("dep3")); // Extra completed item

        assert!(item.can_start(&completed));
    }

    #[test]
    fn test_work_item_start() {
        let mut item = WorkItem::new("p1", "Task");
        item.start();

        assert_eq!(item.state, WorkState::InProgress);
        assert!(item.started_at.is_some());
    }

    #[test]
    fn test_work_item_complete() {
        let mut item = WorkItem::new("p1", "Task");
        item.start();
        item.complete(Some("Success".to_string()));

        assert_eq!(item.state, WorkState::Completed);
        assert!(item.completed_at.is_some());
        assert_eq!(item.result, Some("Success".to_string()));
    }

    #[test]
    fn test_work_item_fail() {
        let mut item = WorkItem::new("p1", "Task");
        item.start();
        item.fail("Something went wrong".to_string());

        assert_eq!(item.state, WorkState::Failed);
        assert!(item.completed_at.is_some());
        assert_eq!(item.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_work_item_cancel() {
        let mut item = WorkItem::new("p1", "Task");
        item.cancel();

        assert_eq!(item.state, WorkState::Cancelled);
        assert!(item.completed_at.is_some());
    }

    #[test]
    fn test_work_state_serialization() {
        let json = serde_json::to_string(&WorkState::InProgress).unwrap();
        assert_eq!(json, "\"in_progress\"");

        let deserialized: WorkState = serde_json::from_str("\"in_progress\"").unwrap();
        assert_eq!(deserialized, WorkState::InProgress);
    }

    #[test]
    fn test_work_priority_serialization() {
        let json = serde_json::to_string(&WorkPriority::Critical).unwrap();
        assert_eq!(json, "\"critical\"");

        let deserialized: WorkPriority = serde_json::from_str("\"critical\"").unwrap();
        assert_eq!(deserialized, WorkPriority::Critical);
    }

    #[test]
    fn test_work_item_serialization_roundtrip() {
        let mut item = WorkItem::new("project-1", "Do something");
        item.depends_on = vec![WorkId::from("dep1"), WorkId::from("dep2")];
        item.metadata
            .insert("key".to_string(), serde_json::json!("value"));

        let json = serde_json::to_string(&item).unwrap();
        let deserialized: WorkItem = serde_json::from_str(&json).unwrap();

        assert_eq!(item.id, deserialized.id);
        assert_eq!(item.project_id, deserialized.project_id);
        assert_eq!(item.content, deserialized.content);
        assert_eq!(item.state, deserialized.state);
        assert_eq!(item.priority, deserialized.priority);
        assert_eq!(item.depends_on, deserialized.depends_on);
    }

    #[test]
    fn test_work_item_default_state() {
        assert_eq!(WorkState::default(), WorkState::Pending);
    }

    #[test]
    fn test_work_item_default_priority() {
        assert_eq!(WorkPriority::default(), WorkPriority::Medium);
    }
}
