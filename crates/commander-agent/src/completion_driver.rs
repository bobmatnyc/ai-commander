//! Completion Driver for autonomous push-to-completion behavior.
//!
//! This module implements "Ralph" style autonomous execution where the agent
//! drives work forward, only stopping when genuinely blocked or complete.

use serde::{Deserialize, Serialize};

/// Default maximum iterations before forcing a user check-in.
const DEFAULT_MAX_ITERATIONS: usize = 50;

/// Drives autonomous completion of goals, tracking progress and blockers.
#[derive(Debug, Clone)]
pub struct CompletionDriver {
    /// Maximum autonomous iterations before forcing user check-in.
    max_iterations: usize,
    /// Current iteration count.
    iteration_count: usize,
    /// Goals to achieve.
    goals: Vec<Goal>,
    /// Blockers requiring user input.
    blockers: Vec<Blocker>,
}

impl Default for CompletionDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl CompletionDriver {
    /// Create a new completion driver with default settings.
    pub fn new() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            iteration_count: 0,
            goals: Vec::new(),
            blockers: Vec::new(),
        }
    }

    /// Create a completion driver with custom max iterations.
    pub fn with_max_iterations(max_iterations: usize) -> Self {
        Self {
            max_iterations,
            ..Self::new()
        }
    }

    /// Determine if we should continue autonomously or stop for user.
    pub fn should_continue(&self) -> ContinueDecision {
        // Stop if blockers exist
        if !self.blockers.is_empty() {
            return ContinueDecision::StopForUser {
                reason: self.format_blockers(),
                blockers: self.blockers.clone(),
            };
        }

        // Stop if max iterations reached
        if self.iteration_count >= self.max_iterations {
            return ContinueDecision::CheckIn {
                reason: "Completed many iterations, checking if on track".to_string(),
                progress: self.format_progress(),
            };
        }

        // Stop if all goals complete
        if self.all_goals_complete() {
            return ContinueDecision::Complete {
                summary: self.format_completion_summary(),
            };
        }

        // Continue working
        ContinueDecision::Continue
    }

    /// Add a goal to track.
    pub fn add_goal(&mut self, goal: Goal) {
        self.goals.push(goal);
    }

    /// Set all goals at once.
    pub fn set_goals(&mut self, goals: Vec<Goal>) {
        self.goals = goals;
    }

    /// Get current goals.
    pub fn goals(&self) -> &[Goal] {
        &self.goals
    }

    /// Get mutable access to goals.
    pub fn goals_mut(&mut self) -> &mut Vec<Goal> {
        &mut self.goals
    }

    /// Update a goal's status by description.
    pub fn update_goal_status(&mut self, description: &str, status: GoalStatus) {
        if let Some(goal) = self.goals.iter_mut().find(|g| g.description == description) {
            goal.status = status;
        }
    }

    /// Mark a goal as completed.
    pub fn complete_goal(&mut self, description: &str) {
        self.update_goal_status(description, GoalStatus::Completed);
    }

    /// Mark a goal as blocked.
    pub fn block_goal(&mut self, description: &str, reason: &str) {
        self.update_goal_status(description, GoalStatus::Blocked(reason.to_string()));
    }

    /// Add a blocker that requires user input.
    pub fn add_blocker(&mut self, blocker: Blocker) {
        self.blockers.push(blocker);
    }

    /// Clear blockers after user provides input.
    pub fn clear_blockers(&mut self) {
        self.blockers.clear();
    }

    /// Get current blockers.
    pub fn blockers(&self) -> &[Blocker] {
        &self.blockers
    }

    /// Check if there are any blockers.
    pub fn has_blockers(&self) -> bool {
        !self.blockers.is_empty()
    }

    /// Increment the iteration count.
    pub fn increment_iteration(&mut self) {
        self.iteration_count += 1;
    }

    /// Get current iteration count.
    pub fn iteration_count(&self) -> usize {
        self.iteration_count
    }

    /// Reset iteration count (e.g., after user check-in).
    pub fn reset_iterations(&mut self) {
        self.iteration_count = 0;
    }

    /// Check if all goals are complete.
    pub fn all_goals_complete(&self) -> bool {
        !self.goals.is_empty() && self.goals.iter().all(|g| g.is_complete())
    }

    /// Get the next pending goal to work on.
    pub fn next_pending_goal(&self) -> Option<&Goal> {
        self.goals.iter().find(|g| g.status == GoalStatus::Pending)
    }

    /// Get the currently in-progress goal.
    pub fn current_goal(&self) -> Option<&Goal> {
        self.goals.iter().find(|g| g.status == GoalStatus::InProgress)
    }

    /// Format blockers for display.
    fn format_blockers(&self) -> String {
        self.blockers
            .iter()
            .map(|b| format!("- {}: {}", b.blocker_type, b.reason))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format current progress for display.
    pub fn format_progress(&self) -> String {
        let completed = self.goals.iter().filter(|g| g.is_complete()).count();
        let total = self.goals.len();
        let pending = self.goals.iter().filter(|g| g.status == GoalStatus::Pending).count();
        let blocked = self.goals.iter().filter(|g| matches!(g.status, GoalStatus::Blocked(_))).count();

        let mut progress = format!(
            "Progress: {}/{} goals complete ({} pending, {} blocked)\n",
            completed, total, pending, blocked
        );

        for goal in &self.goals {
            let status_icon = match &goal.status {
                GoalStatus::Pending => "[ ]",
                GoalStatus::InProgress => "[~]",
                GoalStatus::Completed => "[x]",
                GoalStatus::Blocked(_) => "[!]",
            };
            progress.push_str(&format!("{} {}\n", status_icon, goal.description));
        }

        progress
    }

    /// Format completion summary.
    fn format_completion_summary(&self) -> String {
        let mut summary = "All goals achieved:\n".to_string();
        for goal in &self.goals {
            summary.push_str(&format!("- {}\n", goal.description));
        }
        summary
    }
}

/// A goal to achieve during autonomous execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    /// Description of what needs to be accomplished.
    pub description: String,
    /// Current status of the goal.
    pub status: GoalStatus,
    /// Sub-goals that contribute to this goal.
    pub sub_goals: Vec<Goal>,
}

impl Goal {
    /// Create a new pending goal.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            status: GoalStatus::Pending,
            sub_goals: Vec::new(),
        }
    }

    /// Create a goal with sub-goals.
    pub fn with_sub_goals(description: impl Into<String>, sub_goals: Vec<Goal>) -> Self {
        Self {
            description: description.into(),
            status: GoalStatus::Pending,
            sub_goals,
        }
    }

    /// Check if this goal is complete (including all sub-goals).
    pub fn is_complete(&self) -> bool {
        self.status == GoalStatus::Completed
            && self.sub_goals.iter().all(|sg| sg.is_complete())
    }

    /// Mark as in progress.
    pub fn start(&mut self) {
        self.status = GoalStatus::InProgress;
    }

    /// Mark as completed.
    pub fn complete(&mut self) {
        self.status = GoalStatus::Completed;
    }

    /// Mark as blocked.
    pub fn block(&mut self, reason: impl Into<String>) {
        self.status = GoalStatus::Blocked(reason.into());
    }
}

/// Status of a goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalStatus {
    /// Goal has not been started.
    Pending,
    /// Goal is currently being worked on.
    InProgress,
    /// Goal has been achieved.
    Completed,
    /// Goal is blocked and needs user input.
    Blocked(String),
}

impl std::fmt::Display for GoalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in progress"),
            Self::Completed => write!(f, "completed"),
            Self::Blocked(reason) => write!(f, "blocked: {}", reason),
        }
    }
}

/// A blocker requiring user input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blocker {
    /// Description of why we're blocked.
    pub reason: String,
    /// Type of blocker.
    pub blocker_type: BlockerType,
    /// Possible options for the user to choose from.
    pub options: Vec<String>,
}

impl Blocker {
    /// Create a new blocker.
    pub fn new(reason: impl Into<String>, blocker_type: BlockerType) -> Self {
        Self {
            reason: reason.into(),
            blocker_type,
            options: Vec::new(),
        }
    }

    /// Create a blocker with options.
    pub fn with_options(
        reason: impl Into<String>,
        blocker_type: BlockerType,
        options: Vec<String>,
    ) -> Self {
        Self {
            reason: reason.into(),
            blocker_type,
            options,
        }
    }

    /// Create a decision blocker.
    pub fn decision(reason: impl Into<String>, options: Vec<String>) -> Self {
        Self::with_options(reason, BlockerType::DecisionNeeded, options)
    }

    /// Create an information blocker.
    pub fn information(reason: impl Into<String>) -> Self {
        Self::new(reason, BlockerType::InformationNeeded)
    }

    /// Create an error judgment blocker.
    pub fn error_judgment(reason: impl Into<String>, options: Vec<String>) -> Self {
        Self::with_options(reason, BlockerType::ErrorRequiresJudgment, options)
    }

    /// Create an ambiguous requirements blocker.
    pub fn ambiguous(reason: impl Into<String>, options: Vec<String>) -> Self {
        Self::with_options(reason, BlockerType::AmbiguousRequirements, options)
    }

    /// Create an external dependency blocker.
    pub fn external(reason: impl Into<String>) -> Self {
        Self::new(reason, BlockerType::ExternalDependency)
    }
}

/// Type of blocker requiring user input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockerType {
    /// Need user to make a decision between options.
    DecisionNeeded,
    /// Need information only the user has.
    InformationNeeded,
    /// Error that requires user judgment on how to proceed.
    ErrorRequiresJudgment,
    /// Ambiguous requirements that could go multiple ways.
    AmbiguousRequirements,
    /// External dependency (API key, access, etc.).
    ExternalDependency,
}

impl std::fmt::Display for BlockerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DecisionNeeded => write!(f, "Decision Needed"),
            Self::InformationNeeded => write!(f, "Information Needed"),
            Self::ErrorRequiresJudgment => write!(f, "Error Requires Judgment"),
            Self::AmbiguousRequirements => write!(f, "Ambiguous Requirements"),
            Self::ExternalDependency => write!(f, "External Dependency"),
        }
    }
}

/// Decision on whether to continue autonomous execution.
#[derive(Debug, Clone)]
pub enum ContinueDecision {
    /// Keep working autonomously.
    Continue,
    /// Stop and wait for user input.
    StopForUser {
        /// Reason for stopping.
        reason: String,
        /// Blockers that need resolution.
        blockers: Vec<Blocker>,
    },
    /// Periodic check-in (not blocked, just confirming direction).
    CheckIn {
        /// Reason for check-in.
        reason: String,
        /// Current progress summary.
        progress: String,
    },
    /// All goals achieved.
    Complete {
        /// Summary of what was accomplished.
        summary: String,
    },
}

impl ContinueDecision {
    /// Check if this decision allows continued autonomous work.
    pub fn should_continue(&self) -> bool {
        matches!(self, Self::Continue)
    }

    /// Check if all goals are complete.
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete { .. })
    }

    /// Check if user input is needed.
    pub fn needs_user(&self) -> bool {
        matches!(self, Self::StopForUser { .. } | Self::CheckIn { .. })
    }
}

/// Result of autonomous processing.
#[derive(Debug, Clone)]
pub enum AutonomousResult {
    /// Successfully completed all goals.
    Complete {
        /// Summary of what was accomplished.
        summary: String,
        /// Goals that were achieved.
        goals_achieved: Vec<Goal>,
    },
    /// Need user input to continue.
    NeedsInput {
        /// Reason for needing input.
        reason: String,
        /// Blockers that need resolution.
        blockers: Vec<Blocker>,
        /// Current progress.
        progress: String,
    },
    /// Periodic check-in.
    CheckIn {
        /// Reason for check-in.
        reason: String,
        /// Current progress.
        progress: String,
    },
}

impl AutonomousResult {
    /// Check if work is complete.
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete { .. })
    }

    /// Check if user input is needed.
    pub fn needs_input(&self) -> bool {
        matches!(self, Self::NeedsInput { .. })
    }

    /// Get a human-readable summary.
    pub fn summary(&self) -> String {
        match self {
            Self::Complete { summary, .. } => format!("Completed: {}", summary),
            Self::NeedsInput { reason, .. } => format!("Needs input: {}", reason),
            Self::CheckIn { reason, progress } => format!("Check-in: {}\n{}", reason, progress),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_driver_new() {
        let driver = CompletionDriver::new();
        assert_eq!(driver.max_iterations, DEFAULT_MAX_ITERATIONS);
        assert_eq!(driver.iteration_count, 0);
        assert!(driver.goals.is_empty());
        assert!(driver.blockers.is_empty());
    }

    #[test]
    fn test_should_continue_empty_goals() {
        let driver = CompletionDriver::new();
        // No goals means continue (nothing to complete)
        assert!(matches!(driver.should_continue(), ContinueDecision::Continue));
    }

    #[test]
    fn test_should_continue_with_blockers() {
        let mut driver = CompletionDriver::new();
        driver.add_blocker(Blocker::decision("Choose implementation", vec!["A".into(), "B".into()]));

        match driver.should_continue() {
            ContinueDecision::StopForUser { blockers, .. } => {
                assert_eq!(blockers.len(), 1);
            }
            _ => panic!("Expected StopForUser"),
        }
    }

    #[test]
    fn test_should_continue_max_iterations() {
        let mut driver = CompletionDriver::with_max_iterations(5);
        driver.add_goal(Goal::new("Test goal"));
        driver.iteration_count = 5;

        assert!(matches!(driver.should_continue(), ContinueDecision::CheckIn { .. }));
    }

    #[test]
    fn test_should_continue_all_complete() {
        let mut driver = CompletionDriver::new();
        let mut goal = Goal::new("Test goal");
        goal.complete();
        driver.add_goal(goal);

        assert!(matches!(driver.should_continue(), ContinueDecision::Complete { .. }));
    }

    #[test]
    fn test_goal_status() {
        let mut goal = Goal::new("Implement feature");
        assert_eq!(goal.status, GoalStatus::Pending);

        goal.start();
        assert_eq!(goal.status, GoalStatus::InProgress);

        goal.complete();
        assert_eq!(goal.status, GoalStatus::Completed);
        assert!(goal.is_complete());
    }

    #[test]
    fn test_goal_with_sub_goals() {
        let sub1 = Goal::new("Sub goal 1");
        let mut sub2 = Goal::new("Sub goal 2");
        sub2.complete();

        let mut goal = Goal::with_sub_goals("Main goal", vec![sub1, sub2]);
        goal.complete();

        // Not complete because sub-goal 1 is pending
        assert!(!goal.is_complete());
    }

    #[test]
    fn test_blocker_types() {
        let decision = Blocker::decision("Choose", vec!["A".into()]);
        assert_eq!(decision.blocker_type, BlockerType::DecisionNeeded);

        let info = Blocker::information("Need API key");
        assert_eq!(info.blocker_type, BlockerType::InformationNeeded);

        let error = Blocker::error_judgment("Error occurred", vec!["Retry".into()]);
        assert_eq!(error.blocker_type, BlockerType::ErrorRequiresJudgment);
    }

    #[test]
    fn test_format_progress() {
        let mut driver = CompletionDriver::new();
        driver.add_goal(Goal::new("Goal 1"));
        let mut goal2 = Goal::new("Goal 2");
        goal2.complete();
        driver.add_goal(goal2);

        let progress = driver.format_progress();
        assert!(progress.contains("1/2 goals complete"));
        assert!(progress.contains("[ ] Goal 1"));
        assert!(progress.contains("[x] Goal 2"));
    }

    #[test]
    fn test_continue_decision_helpers() {
        let cont = ContinueDecision::Continue;
        assert!(cont.should_continue());
        assert!(!cont.is_complete());
        assert!(!cont.needs_user());

        let complete = ContinueDecision::Complete { summary: "Done".into() };
        assert!(!complete.should_continue());
        assert!(complete.is_complete());
        assert!(!complete.needs_user());

        let stop = ContinueDecision::StopForUser {
            reason: "Need help".into(),
            blockers: vec![],
        };
        assert!(!stop.should_continue());
        assert!(!stop.is_complete());
        assert!(stop.needs_user());
    }

    #[test]
    fn test_autonomous_result_helpers() {
        let complete = AutonomousResult::Complete {
            summary: "All done".into(),
            goals_achieved: vec![],
        };
        assert!(complete.is_complete());
        assert!(!complete.needs_input());

        let needs = AutonomousResult::NeedsInput {
            reason: "Help".into(),
            blockers: vec![],
            progress: "50%".into(),
        };
        assert!(!needs.is_complete());
        assert!(needs.needs_input());
    }
}
