//! Feedback types and data structures for the auto-eval framework.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A single piece of feedback about an agent's performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    /// Unique identifier for this feedback.
    pub id: String,
    /// ID of the agent that produced the output.
    pub agent_id: String,
    /// Type of feedback detected.
    pub feedback_type: FeedbackType,
    /// What was happening (task context).
    pub context: String,
    /// What the user said/did.
    pub user_input: String,
    /// What the agent produced.
    pub agent_output: String,
    /// What should have happened (if correction provided).
    pub correction: Option<String>,
    /// When this feedback was recorded.
    pub timestamp: DateTime<Utc>,
}

impl Feedback {
    /// Create new feedback.
    pub fn new(
        agent_id: impl Into<String>,
        feedback_type: FeedbackType,
        context: impl Into<String>,
        user_input: impl Into<String>,
        agent_output: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            feedback_type,
            context: context.into(),
            user_input: user_input.into(),
            agent_output: agent_output.into(),
            correction: None,
            timestamp: Utc::now(),
        }
    }

    /// Add a correction to this feedback.
    pub fn with_correction(mut self, correction: impl Into<String>) -> Self {
        self.correction = Some(correction.into());
        self
    }
}

/// Types of feedback that can be detected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackType {
    /// User explicitly said something was wrong.
    ExplicitNegative,
    /// User retried the same request immediately.
    ImplicitRetry,
    /// An error occurred during processing.
    Error,
    /// Request timed out or got stuck.
    Timeout,
    /// User provided a correction.
    Correction,
    /// Positive feedback (for balance).
    Positive,
}

impl std::fmt::Display for FeedbackType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExplicitNegative => write!(f, "explicit_negative"),
            Self::ImplicitRetry => write!(f, "implicit_retry"),
            Self::Error => write!(f, "error"),
            Self::Timeout => write!(f, "timeout"),
            Self::Correction => write!(f, "correction"),
            Self::Positive => write!(f, "positive"),
        }
    }
}

/// Summary of feedback for an agent.
#[derive(Debug, Clone, Default)]
pub struct FeedbackSummary {
    /// Total feedback entries.
    pub total: usize,
    /// Count of positive feedback.
    pub positive: usize,
    /// Count of negative feedback (explicit + implicit).
    pub negative: usize,
    /// Count of error feedback.
    pub errors: usize,
    /// Count of timeout feedback.
    pub timeouts: usize,
    /// Count of corrections.
    pub corrections: usize,
    /// Most common issues (derived from feedback).
    pub most_common_issues: Vec<String>,
}

impl FeedbackSummary {
    /// Build summary from feedback counts and entries.
    pub fn from_counts(counts: &HashMap<FeedbackType, usize>, all_feedback: &[&Feedback]) -> Self {
        let positive = counts.get(&FeedbackType::Positive).copied().unwrap_or(0);
        let negative = counts.get(&FeedbackType::ExplicitNegative).copied().unwrap_or(0)
            + counts.get(&FeedbackType::ImplicitRetry).copied().unwrap_or(0);
        let errors = counts.get(&FeedbackType::Error).copied().unwrap_or(0);
        let timeouts = counts.get(&FeedbackType::Timeout).copied().unwrap_or(0);
        let corrections = counts.get(&FeedbackType::Correction).copied().unwrap_or(0);

        // Extract common issues from negative feedback
        let mut issue_counts: HashMap<String, usize> = HashMap::new();
        for feedback in all_feedback.iter().filter(|f| {
            f.feedback_type == FeedbackType::ExplicitNegative
                || f.feedback_type == FeedbackType::Error
        }) {
            for word in feedback.user_input.split_whitespace() {
                let word = word.to_lowercase();
                if word.len() > 3 {
                    *issue_counts.entry(word).or_insert(0) += 1;
                }
            }
        }

        let mut most_common: Vec<_> = issue_counts.into_iter().collect();
        most_common.sort_by(|a, b| b.1.cmp(&a.1));
        let most_common_issues: Vec<String> = most_common
            .into_iter()
            .take(5)
            .map(|(word, _)| word)
            .collect();

        Self {
            total: all_feedback.len(),
            positive,
            negative,
            errors,
            timeouts,
            corrections,
            most_common_issues,
        }
    }
}
