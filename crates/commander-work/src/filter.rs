//! Work item filtering for queries.

use commander_models::{ProjectId, WorkItem, WorkPriority, WorkState};

/// Filter criteria for querying work items.
#[derive(Debug, Clone, Default)]
pub struct WorkFilter {
    /// Filter by project ID.
    pub project_id: Option<ProjectId>,
    /// Filter by work state.
    pub state: Option<WorkState>,
    /// Filter by priority.
    pub priority: Option<WorkPriority>,
}

impl WorkFilter {
    /// Creates a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the project ID filter.
    pub fn with_project_id(mut self, project_id: ProjectId) -> Self {
        self.project_id = Some(project_id);
        self
    }

    /// Sets the state filter.
    pub fn with_state(mut self, state: WorkState) -> Self {
        self.state = Some(state);
        self
    }

    /// Sets the priority filter.
    pub fn with_priority(mut self, priority: WorkPriority) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Returns true if the work item matches this filter.
    pub fn matches(&self, item: &WorkItem) -> bool {
        if let Some(ref project_id) = self.project_id {
            if item.project_id != *project_id {
                return false;
            }
        }

        if let Some(state) = self.state {
            if item.state != state {
                return false;
            }
        }

        if let Some(priority) = self.priority {
            if item.priority != priority {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_models::WorkItem;

    fn make_item(project: &str, content: &str) -> WorkItem {
        WorkItem::new(project, content)
    }

    #[test]
    fn test_empty_filter_matches_all() {
        let filter = WorkFilter::new();
        let item = make_item("proj-1", "Task");
        assert!(filter.matches(&item));
    }

    #[test]
    fn test_filter_by_project_id() {
        let filter = WorkFilter::new().with_project_id("proj-1".into());

        let i1 = make_item("proj-1", "Task");
        let i2 = make_item("proj-2", "Task");

        assert!(filter.matches(&i1));
        assert!(!filter.matches(&i2));
    }

    #[test]
    fn test_filter_by_state() {
        let filter = WorkFilter::new().with_state(WorkState::Pending);

        let i1 = make_item("proj-1", "Task");
        let mut i2 = make_item("proj-1", "Task");
        i2.start();

        assert!(filter.matches(&i1));
        assert!(!filter.matches(&i2));
    }

    #[test]
    fn test_filter_by_priority() {
        let filter = WorkFilter::new().with_priority(WorkPriority::High);

        let i1 = WorkItem::with_priority("proj-1", "High", WorkPriority::High);
        let i2 = WorkItem::with_priority("proj-1", "Low", WorkPriority::Low);

        assert!(filter.matches(&i1));
        assert!(!filter.matches(&i2));
    }

    #[test]
    fn test_combined_filters() {
        let filter = WorkFilter::new()
            .with_project_id("proj-1".into())
            .with_state(WorkState::Pending)
            .with_priority(WorkPriority::High);

        let i1 = WorkItem::with_priority("proj-1", "Match", WorkPriority::High);
        let i2 = WorkItem::with_priority("proj-2", "Wrong project", WorkPriority::High);
        let i3 = WorkItem::with_priority("proj-1", "Wrong priority", WorkPriority::Low);

        assert!(filter.matches(&i1));
        assert!(!filter.matches(&i2));
        assert!(!filter.matches(&i3));
    }
}
