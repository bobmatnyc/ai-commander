//! Session state and output analysis structures.

use serde::{Deserialize, Serialize};

/// State of the session being monitored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    /// Current goals for this session.
    pub goals: Vec<String>,

    /// Current task being worked on, if any.
    pub current_task: Option<String>,

    /// Progress indicator (0.0 to 1.0).
    pub progress: f32,

    /// Current blockers preventing progress.
    pub blockers: Vec<String>,

    /// Files that have been modified in this session.
    pub files_modified: Vec<String>,

    /// Last output received from the session.
    pub last_output: Option<String>,
}

impl SessionState {
    /// Create a new empty session state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a goal to the session.
    pub fn add_goal(&mut self, goal: impl Into<String>) {
        self.goals.push(goal.into());
    }

    /// Set the current task.
    pub fn set_current_task(&mut self, task: impl Into<String>) {
        self.current_task = Some(task.into());
    }

    /// Clear the current task.
    pub fn clear_current_task(&mut self) {
        self.current_task = None;
    }

    /// Update progress (clamped to 0.0 - 1.0).
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
    }

    /// Add a blocker.
    pub fn add_blocker(&mut self, blocker: impl Into<String>) {
        self.blockers.push(blocker.into());
    }

    /// Clear all blockers.
    pub fn clear_blockers(&mut self) {
        self.blockers.clear();
    }

    /// Add a modified file.
    pub fn add_modified_file(&mut self, file: impl Into<String>) {
        let file = file.into();
        if !self.files_modified.contains(&file) {
            self.files_modified.push(file);
        }
    }

    /// Set the last output.
    pub fn set_last_output(&mut self, output: impl Into<String>) {
        self.last_output = Some(output.into());
    }
}

/// Analysis of session output.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputAnalysis {
    /// Whether a task completion was detected.
    pub detected_completion: bool,

    /// Whether the session is waiting for user input.
    pub waiting_for_input: bool,

    /// Error message if an error was detected.
    pub error_detected: Option<String>,

    /// Files that were changed in this output.
    pub files_changed: Vec<String>,

    /// Summary of the output.
    pub summary: String,
}

impl OutputAnalysis {
    /// Create a new empty analysis.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an analysis with a summary.
    pub fn with_summary(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            ..Default::default()
        }
    }
}
