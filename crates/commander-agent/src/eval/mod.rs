//! Auto-Eval Framework for Agent Improvement.
//!
//! This module provides automatic evaluation capabilities that capture errors and
//! negative user feedback to improve agent instructions over time.
//!
//! # Overview
//!
//! The evaluation framework detects:
//! - Explicit negative feedback from users
//! - Implicit retries (same request repeated)
//! - Errors during processing
//! - Timeouts
//! - Corrections provided by users
//! - Positive feedback (for balance)
//!
//! # Example
//!
//! ```ignore
//! use commander_agent::eval::{AutoEval, FeedbackType};
//! use std::path::PathBuf;
//!
//! let mut eval = AutoEval::new(PathBuf::from("~/.ai-commander/db/feedback")).unwrap();
//!
//! // Process a conversation turn
//! let feedback = eval.process_turn(
//!     "agent-1",
//!     "That's wrong, try again",
//!     "Here's the result: ...",
//!     Some("Generate a report"),
//!     None,
//! ).await?;
//!
//! // Get feedback summary
//! let summary = eval.summary("agent-1");
//! println!("Total feedback: {}", summary.total);
//! ```

mod detector;
mod improvement;
mod store;
mod types;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use crate::error::Result;

// Re-export public types
pub use detector::FeedbackDetector;
pub use improvement::{Improvement, ImprovementGenerator};
pub use store::FeedbackStore;
pub use types::{Feedback, FeedbackSummary, FeedbackType};

/// Main auto-eval integration point.
pub struct AutoEval {
    /// Feedback detector.
    detector: FeedbackDetector,
    /// Feedback store.
    store: FeedbackStore,
    /// Previous user input (for retry detection).
    previous_input: Option<String>,
}

impl AutoEval {
    /// Create a new auto-eval instance.
    pub fn new(store_path: PathBuf) -> Result<Self> {
        Ok(Self {
            detector: FeedbackDetector::new(),
            store: FeedbackStore::new(store_path)?,
            previous_input: None,
        })
    }

    /// Process a conversation turn and record any detected feedback.
    ///
    /// Returns the feedback if any was detected.
    pub async fn process_turn(
        &mut self,
        agent_id: &str,
        user_input: &str,
        agent_output: &str,
        previous_input: Option<&str>,
        error: Option<&str>,
    ) -> Result<Option<Feedback>> {
        // Check for error feedback first
        if let Some(error_msg) = error {
            let feedback = Feedback::new(
                agent_id,
                FeedbackType::Error,
                "Error during processing",
                user_input,
                agent_output,
            )
            .with_correction(error_msg.to_string());

            self.store.add(feedback.clone()).await?;
            return Ok(Some(feedback));
        }

        // Check for retry
        if let Some(prev) = previous_input.or(self.previous_input.as_deref()) {
            if self.detector.is_retry(user_input, prev) {
                let feedback = Feedback::new(
                    agent_id,
                    FeedbackType::ImplicitRetry,
                    "User retried previous request",
                    user_input,
                    agent_output,
                );

                self.store.add(feedback.clone()).await?;
                self.previous_input = Some(user_input.to_string());
                return Ok(Some(feedback));
            }
        }

        // Detect feedback type from message
        if let Some(feedback_type) = self.detector.detect(user_input, agent_output) {
            let feedback = Feedback::new(
                agent_id,
                feedback_type,
                "User feedback detected",
                user_input,
                agent_output,
            );

            self.store.add(feedback.clone()).await?;
            self.previous_input = Some(user_input.to_string());
            return Ok(Some(feedback));
        }

        // Update previous input for future retry detection
        self.previous_input = Some(user_input.to_string());

        Ok(None)
    }

    /// Record a timeout event.
    pub async fn record_timeout(
        &mut self,
        agent_id: &str,
        context: &str,
        user_input: &str,
    ) -> Result<()> {
        let feedback = Feedback::new(
            agent_id,
            FeedbackType::Timeout,
            context,
            user_input,
            "(timed out)",
        );

        self.store.add(feedback).await
    }

    /// Get feedback summary for an agent.
    pub fn summary(&self, agent_id: &str) -> FeedbackSummary {
        let counts = self.store.count_by_type(agent_id);
        let all_feedback = self.store.get_all(agent_id);

        FeedbackSummary::from_counts(&counts, &all_feedback)
    }

    /// Get the feedback store for direct access.
    pub fn store(&self) -> &FeedbackStore {
        &self.store
    }

    /// Get mutable access to the feedback store.
    pub fn store_mut(&mut self) -> &mut FeedbackStore {
        &mut self.store
    }
}
