//! Type definitions for change detection.

/// Significance level of a detected change.
///
/// Used to determine polling rate and whether to invoke LLM analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Significance {
    /// Ignore - UI noise, spinners, no actual content change
    Ignore,
    /// Low - minor progress, file operations, routine output
    Low,
    /// Medium - task progress, test results, build output
    Medium,
    /// High - completion, errors, needs input
    High,
    /// Critical - immediate attention needed (failures, security issues)
    Critical,
}

impl Default for Significance {
    fn default() -> Self {
        Self::Ignore
    }
}

/// Type of change detected in session output.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChangeType {
    /// No meaningful change detected
    None,
    /// New content added (output appended)
    Addition,
    /// Content changed significantly (replacement)
    Modification,
    /// Task or operation completed
    Completion,
    /// Error or failure detected
    Error,
    /// Session waiting for user input
    WaitingForInput,
    /// Progress update (build, test, install)
    Progress,
}

impl Default for ChangeType {
    fn default() -> Self {
        Self::None
    }
}

/// Event describing a detected change in session output.
#[derive(Debug, Clone, Default)]
pub struct ChangeEvent {
    /// Type of change detected
    pub change_type: ChangeType,
    /// Human-readable summary of the change
    pub summary: String,
    /// New lines that triggered this event
    pub diff_lines: Vec<String>,
    /// Significance level for polling/notification decisions
    pub significance: Significance,
}

impl ChangeEvent {
    /// Create a "no change" event.
    pub fn none() -> Self {
        Self {
            change_type: ChangeType::None,
            summary: String::new(),
            diff_lines: Vec::new(),
            significance: Significance::Ignore,
        }
    }

    /// Check if this event represents a meaningful change.
    pub fn is_meaningful(&self) -> bool {
        self.significance >= Significance::Medium
    }

    /// Check if this event requires user notification.
    pub fn requires_notification(&self) -> bool {
        self.significance >= Significance::High
    }
}

/// Notification to send when significant changes are detected.
#[derive(Debug, Clone)]
pub struct ChangeNotification {
    /// Session that generated this notification
    pub session_id: String,
    /// Summary of what changed
    pub summary: String,
    /// Whether user action is required
    pub requires_action: bool,
    /// Type of change
    pub change_type: ChangeType,
    /// Significance level
    pub significance: Significance,
}
