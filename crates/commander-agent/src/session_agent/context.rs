//! Context management methods for SessionAgent.

use tracing::{debug, info, trace};

use crate::context_manager::{ContextAction, CriticalAction};
use crate::error::Result;

use super::SessionAgent;

impl SessionAgent {
    /// Check context usage and take appropriate action based on strategy.
    ///
    /// This method estimates current token usage and triggers the appropriate
    /// action based on the configured context strategy:
    /// - MPM: Executes pause command and provides resume instructions
    /// - Claude Code: Triggers context compaction
    /// - Generic: Warns the user
    ///
    /// # Returns
    ///
    /// Returns the action that was triggered, or `Continue` if context is fine.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let action = agent.check_context().await?;
    /// match action {
    ///     ContextAction::Critical { action: CriticalAction::Pause { .. } } => {
    ///         // Session was paused, notify user
    ///     }
    ///     ContextAction::Warn { remaining_percent } => {
    ///         // Context getting low, warn user
    ///     }
    ///     _ => {}
    /// }
    /// ```
    pub async fn check_context(&mut self) -> Result<ContextAction> {
        // Estimate current token usage
        let estimated = self.estimate_context_tokens();

        // Get action from context manager
        let action = self.context_manager.update(estimated);

        match &action {
            ContextAction::Critical {
                action: CriticalAction::Pause { command, state_summary },
            } => {
                // For MPM: Log the pause command (actual execution handled by caller)
                info!(
                    session_id = %self.session_id,
                    command = %command,
                    "Context critical, pause recommended"
                );
                // Store state summary for resumption
                self.context_manager.set_state_summary(self.generate_pause_state());
                debug!("Pause state: {}", state_summary);
            }
            ContextAction::Critical {
                action: CriticalAction::Compact { messages_to_summarize },
            } => {
                // For Claude Code: Trigger compaction
                info!(
                    session_id = %self.session_id,
                    messages_to_summarize = %messages_to_summarize,
                    "Context critical, triggering compaction"
                );
                self.context_window.compact().await?;
            }
            ContextAction::Critical {
                action: CriticalAction::Alert { message },
            } => {
                // For Generic: Log the alert
                info!(
                    session_id = %self.session_id,
                    message = %message,
                    "Context alert"
                );
            }
            ContextAction::Warn { remaining_percent } => {
                debug!(
                    session_id = %self.session_id,
                    remaining_percent = %remaining_percent,
                    "Context warning"
                );
            }
            ContextAction::Continue => {
                trace!(
                    session_id = %self.session_id,
                    "Context OK"
                );
            }
        }

        Ok(action)
    }

    /// Estimate the current context token usage.
    pub(super) fn estimate_context_tokens(&self) -> usize {
        // Rough estimate: 4 chars per token
        let context_chars = self.context.estimated_tokens() * 4;
        let window_tokens = self.context_window.estimated_tokens();

        // Add state information
        let state_chars = format!("{:?}", self.session_state).len();

        (context_chars + state_chars) / 4 + window_tokens
    }

    /// Generate a state summary for pause/resume operations.
    pub(super) fn generate_pause_state(&self) -> String {
        let mut summary = String::from("## Session Pause State\n\n");

        // Tasks completed (progress = 1.0)
        if self.session_state.progress >= 1.0 {
            summary.push_str("Tasks Completed: Current task completed\n");
        }

        // Tasks in progress
        if let Some(ref task) = self.session_state.current_task {
            summary.push_str(&format!("Tasks In Progress: {}\n", task));
        }

        // Goals
        if !self.session_state.goals.is_empty() {
            summary.push_str(&format!(
                "Goals: {}\n",
                self.session_state.goals.join(", ")
            ));
        }

        // Blockers
        if !self.session_state.blockers.is_empty() {
            summary.push_str(&format!(
                "Blockers: {}\n",
                self.session_state.blockers.join(", ")
            ));
        }

        // Files modified
        if !self.session_state.files_modified.is_empty() {
            summary.push_str(&format!(
                "Files Modified: {}\n",
                self.session_state.files_modified.join(", ")
            ));
        }

        // Progress
        summary.push_str(&format!(
            "Progress: {:.0}%\n",
            self.session_state.progress * 100.0
        ));

        // Context usage
        summary.push_str(&format!(
            "Context Usage: {:.1}%\n",
            (1.0 - self.context_manager.remaining_percent()) * 100.0
        ));

        summary.push_str("\nNext Action: Resume session to continue from this state\n");

        summary
    }
}
