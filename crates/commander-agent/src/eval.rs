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

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::{AgentError, Result};

// =============================================================================
// Feedback Types
// =============================================================================

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

// =============================================================================
// Feedback Detection
// =============================================================================

/// Detects feedback signals from user messages.
pub struct FeedbackDetector {
    /// Patterns that indicate negative feedback.
    negative_patterns: Vec<Regex>,
    /// Patterns that indicate positive feedback.
    positive_patterns: Vec<Regex>,
    /// Patterns that indicate a correction.
    correction_patterns: Vec<Regex>,
}

impl Default for FeedbackDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedbackDetector {
    /// Create a new feedback detector with default patterns.
    pub fn new() -> Self {
        Self {
            negative_patterns: vec![
                // Direct rejection
                Regex::new(r"(?i)\b(no|wrong|incorrect|that's not right|not what I)\b").unwrap(),
                // Cancellation
                Regex::new(r"(?i)\b(stop|cancel|nevermind|forget it|abort)\b").unwrap(),
                // Frustration
                Regex::new(r"(?i)\b(doesn't work|broken|bug|failed|error)\b").unwrap(),
                // Explicit negative
                Regex::new(r"(?i)\b(bad|terrible|awful|useless|stupid)\b").unwrap(),
            ],
            positive_patterns: vec![
                // Thanks
                Regex::new(r"(?i)\b(thanks|thank you|thx|ty)\b").unwrap(),
                // Approval
                Regex::new(r"(?i)\b(great|perfect|exactly|excellent|awesome|nice)\b").unwrap(),
                // Confirmation
                Regex::new(r"(?i)\b(works|working|correct|right|good job)\b").unwrap(),
            ],
            correction_patterns: vec![
                // "I meant..."
                Regex::new(r"(?i)\b(I meant|should be|should have)\b").unwrap(),
                // "Instead..." - use word boundaries to avoid matching "not that" in "that's not that"
                Regex::new(r"(?i)\b(instead of|rather than|use .+ instead)\b").unwrap(),
                // "Actually, do X"
                Regex::new(r"(?i)^actually[,\s]").unwrap(),
            ],
        }
    }

    /// Detect feedback type from user message.
    ///
    /// Returns the detected feedback type, or None if no clear signal.
    pub fn detect(&self, message: &str, previous_agent_output: &str) -> Option<FeedbackType> {
        // Check for correction patterns first (most specific)
        for pattern in &self.correction_patterns {
            if pattern.is_match(message) {
                return Some(FeedbackType::Correction);
            }
        }

        // Check for explicit negative
        for pattern in &self.negative_patterns {
            if pattern.is_match(message) {
                // Make sure it's not a false positive (e.g., "no problem" is positive)
                if !self.is_false_positive_negative(message) {
                    return Some(FeedbackType::ExplicitNegative);
                }
            }
        }

        // Check for positive
        for pattern in &self.positive_patterns {
            if pattern.is_match(message) {
                return Some(FeedbackType::Positive);
            }
        }

        // If previous output was short/truncated and user repeats, might be implicit retry
        if !previous_agent_output.is_empty() && previous_agent_output.len() < 50 {
            // Could indicate truncation/failure
            debug!("Short previous output, might be truncation");
        }

        None
    }

    /// Check if this looks like a retry of the previous request.
    ///
    /// Returns true if the current message is similar enough to the previous
    /// to be considered a retry.
    pub fn is_retry(&self, current: &str, previous: &str) -> bool {
        if current.is_empty() || previous.is_empty() {
            return false;
        }

        // Normalize both strings
        let current_normalized = self.normalize(current);
        let previous_normalized = self.normalize(previous);

        // Exact match after normalization
        if current_normalized == previous_normalized {
            return true;
        }

        // Check similarity using simple Jaccard index on words
        let current_words: std::collections::HashSet<&str> =
            current_normalized.split_whitespace().collect();
        let previous_words: std::collections::HashSet<&str> =
            previous_normalized.split_whitespace().collect();

        if current_words.is_empty() || previous_words.is_empty() {
            return false;
        }

        let intersection = current_words.intersection(&previous_words).count();
        let union = current_words.union(&previous_words).count();

        let similarity = intersection as f64 / union as f64;

        // Threshold for considering it a retry
        similarity > 0.7
    }

    /// Normalize a string for comparison.
    fn normalize(&self, s: &str) -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Check if a negative match is a false positive.
    fn is_false_positive_negative(&self, message: &str) -> bool {
        let lower = message.to_lowercase();
        // Common false positives
        let false_positives = [
            "no problem",
            "no worries",
            "no rush",
            "not bad",
            "not wrong",
            "stop there", // Could be intentional stopping point
        ];

        false_positives.iter().any(|fp| lower.contains(fp))
    }
}

// =============================================================================
// Feedback Store
// =============================================================================

/// Persistent storage for feedback entries.
pub struct FeedbackStore {
    /// Directory for storing feedback data.
    path: PathBuf,
    /// In-memory cache of feedback entries.
    entries: Vec<Feedback>,
}

impl FeedbackStore {
    /// Create a new feedback store at the specified path.
    pub fn new(path: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&path).map_err(|e| {
            AgentError::Configuration(format!(
                "Failed to create feedback directory {}: {}",
                path.display(),
                e
            ))
        })?;

        let mut store = Self {
            path,
            entries: Vec::new(),
        };

        store.load()?;
        Ok(store)
    }

    /// Add a feedback entry.
    pub async fn add(&mut self, feedback: Feedback) -> Result<()> {
        info!(
            id = %feedback.id,
            agent_id = %feedback.agent_id,
            feedback_type = %feedback.feedback_type,
            "Recording feedback"
        );

        self.entries.push(feedback);
        self.save()
    }

    /// Get recent feedback for a specific agent.
    pub async fn get_recent(&self, agent_id: &str, limit: usize) -> Vec<&Feedback> {
        self.entries
            .iter()
            .filter(|f| f.agent_id == agent_id)
            .rev() // Most recent first
            .take(limit)
            .collect()
    }

    /// Get feedback by type.
    pub async fn get_by_type(&self, feedback_type: FeedbackType, limit: usize) -> Vec<&Feedback> {
        self.entries
            .iter()
            .filter(|f| f.feedback_type == feedback_type)
            .rev()
            .take(limit)
            .collect()
    }

    /// Get all feedback for an agent.
    pub fn get_all(&self, agent_id: &str) -> Vec<&Feedback> {
        self.entries
            .iter()
            .filter(|f| f.agent_id == agent_id)
            .collect()
    }

    /// Count feedback by type for an agent.
    pub fn count_by_type(&self, agent_id: &str) -> HashMap<FeedbackType, usize> {
        let mut counts = HashMap::new();

        for feedback in self.entries.iter().filter(|f| f.agent_id == agent_id) {
            *counts.entry(feedback.feedback_type.clone()).or_insert(0) += 1;
        }

        counts
    }

    /// Save feedback to disk.
    pub fn save(&self) -> Result<()> {
        let file = self.data_file();
        let json = serde_json::to_string_pretty(&self.entries)?;

        // Atomic write via temp file
        let temp_file = file.with_extension("json.tmp");
        std::fs::write(&temp_file, &json).map_err(|e| {
            AgentError::Configuration(format!("Failed to write feedback: {}", e))
        })?;
        std::fs::rename(&temp_file, &file).map_err(|e| {
            AgentError::Configuration(format!("Failed to save feedback: {}", e))
        })?;

        debug!(count = self.entries.len(), "Saved feedback to disk");
        Ok(())
    }

    /// Load feedback from disk.
    pub fn load(&mut self) -> Result<()> {
        let file = self.data_file();
        if !file.exists() {
            debug!(path = %file.display(), "No existing feedback file");
            return Ok(());
        }

        let data = std::fs::read_to_string(&file).map_err(|e| {
            AgentError::Configuration(format!("Failed to read feedback: {}", e))
        })?;

        self.entries = serde_json::from_str(&data)?;
        info!(count = self.entries.len(), "Loaded feedback from disk");
        Ok(())
    }

    fn data_file(&self) -> PathBuf {
        self.path.join("feedback.json")
    }
}

// =============================================================================
// Improvement Suggestions (Future)
// =============================================================================

/// A suggested improvement based on feedback analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Improvement {
    /// Category of the improvement (e.g., "response_format", "error_handling").
    pub category: String,
    /// Description of current behavior.
    pub current_behavior: String,
    /// Suggested change.
    pub suggested_change: String,
    /// IDs of feedback that support this suggestion.
    pub supporting_feedback: Vec<String>,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f32,
}

/// Generator for improvement suggestions.
///
/// Currently a placeholder for future LLM-based improvement generation.
pub struct ImprovementGenerator {
    /// Minimum feedback count before generating suggestions.
    min_feedback_count: usize,
}

impl Default for ImprovementGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl ImprovementGenerator {
    /// Create a new improvement generator.
    pub fn new() -> Self {
        Self {
            min_feedback_count: 5,
        }
    }

    /// Analyze feedback and suggest improvements.
    ///
    /// Currently returns an empty list - future implementation will use
    /// LLM-based analysis to cluster similar issues and generate suggestions.
    pub async fn analyze(&self, feedback: &[Feedback]) -> Result<Vec<Improvement>> {
        if feedback.len() < self.min_feedback_count {
            debug!(
                count = feedback.len(),
                min = self.min_feedback_count,
                "Not enough feedback for improvement analysis"
            );
            return Ok(Vec::new());
        }

        // Placeholder: cluster similar feedback and identify patterns
        // Future implementation will:
        // 1. Group feedback by similarity (using embeddings)
        // 2. Identify common themes in negative feedback
        // 3. Generate improvement suggestions using LLM
        // 4. Rank suggestions by confidence/impact

        warn!("LLM-based improvement generation not yet implemented");

        // For now, generate basic suggestions from feedback counts
        let mut suggestions = Vec::new();

        // Count by type
        let mut type_counts: HashMap<&FeedbackType, usize> = HashMap::new();
        for f in feedback {
            *type_counts.entry(&f.feedback_type).or_insert(0) += 1;
        }

        // If lots of retries, suggest clarity improvements
        if let Some(&retry_count) = type_counts.get(&FeedbackType::ImplicitRetry) {
            if retry_count > 3 {
                suggestions.push(Improvement {
                    category: "clarity".to_string(),
                    current_behavior: format!(
                        "Users retry requests frequently ({} times)",
                        retry_count
                    ),
                    suggested_change:
                        "Improve response clarity and ask clarifying questions when uncertain"
                            .to_string(),
                    supporting_feedback: feedback
                        .iter()
                        .filter(|f| f.feedback_type == FeedbackType::ImplicitRetry)
                        .map(|f| f.id.clone())
                        .collect(),
                    confidence: 0.6,
                });
            }
        }

        // If lots of errors, suggest robustness improvements
        if let Some(&error_count) = type_counts.get(&FeedbackType::Error) {
            if error_count > 3 {
                suggestions.push(Improvement {
                    category: "robustness".to_string(),
                    current_behavior: format!("Frequent errors ({} times)", error_count),
                    suggested_change: "Add more error handling and validation".to_string(),
                    supporting_feedback: feedback
                        .iter()
                        .filter(|f| f.feedback_type == FeedbackType::Error)
                        .map(|f| f.id.clone())
                        .collect(),
                    confidence: 0.7,
                });
            }
        }

        Ok(suggestions)
    }
}

// =============================================================================
// Auto-Eval Integration
// =============================================================================

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
            // Simple word frequency for now
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

        FeedbackSummary {
            total: all_feedback.len(),
            positive,
            negative,
            errors,
            timeouts,
            corrections,
            most_common_issues,
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_store() -> (FeedbackStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = FeedbackStore::new(temp_dir.path().to_path_buf()).unwrap();
        (store, temp_dir)
    }

    #[test]
    fn test_feedback_type_display() {
        assert_eq!(FeedbackType::ExplicitNegative.to_string(), "explicit_negative");
        assert_eq!(FeedbackType::ImplicitRetry.to_string(), "implicit_retry");
        assert_eq!(FeedbackType::Error.to_string(), "error");
        assert_eq!(FeedbackType::Timeout.to_string(), "timeout");
        assert_eq!(FeedbackType::Correction.to_string(), "correction");
        assert_eq!(FeedbackType::Positive.to_string(), "positive");
    }

    #[test]
    fn test_feedback_creation() {
        let feedback = Feedback::new(
            "agent-1",
            FeedbackType::ExplicitNegative,
            "Testing feature",
            "That's wrong",
            "Here's the result",
        );

        assert_eq!(feedback.agent_id, "agent-1");
        assert_eq!(feedback.feedback_type, FeedbackType::ExplicitNegative);
        assert!(feedback.correction.is_none());

        let feedback = feedback.with_correction("Do it this way");
        assert_eq!(feedback.correction, Some("Do it this way".to_string()));
    }

    #[test]
    fn test_feedback_detector_negative() {
        let detector = FeedbackDetector::new();

        // Should detect negative feedback
        assert_eq!(
            detector.detect("That's wrong", "Some output"),
            Some(FeedbackType::ExplicitNegative)
        );
        assert_eq!(
            detector.detect("No, that's not what I wanted", "Some output"),
            Some(FeedbackType::ExplicitNegative)
        );
        assert_eq!(
            detector.detect("This is broken and doesn't work", "Some output"),
            Some(FeedbackType::ExplicitNegative)
        );
        assert_eq!(
            detector.detect("Abort the operation", "Some output"),
            Some(FeedbackType::ExplicitNegative)
        );
    }

    #[test]
    fn test_feedback_detector_positive() {
        let detector = FeedbackDetector::new();

        // Should detect positive feedback
        assert_eq!(
            detector.detect("Thanks, that's great!", "Some output"),
            Some(FeedbackType::Positive)
        );
        assert_eq!(
            detector.detect("Perfect, exactly what I needed", "Some output"),
            Some(FeedbackType::Positive)
        );
    }

    #[test]
    fn test_feedback_detector_correction() {
        let detector = FeedbackDetector::new();

        // Should detect corrections
        assert_eq!(
            detector.detect("I meant the other file", "Some output"),
            Some(FeedbackType::Correction)
        );
        assert_eq!(
            detector.detect("Actually, use Python", "Some output"),
            Some(FeedbackType::Correction)
        );
        assert_eq!(
            detector.detect("It should be 'hello' not 'world'", "Some output"),
            Some(FeedbackType::Correction)
        );
    }

    #[test]
    fn test_feedback_detector_false_positive() {
        let detector = FeedbackDetector::new();

        // "No problem" should not be detected as negative
        assert_ne!(
            detector.detect("No problem, thanks!", "Some output"),
            Some(FeedbackType::ExplicitNegative)
        );
    }

    #[test]
    fn test_feedback_detector_no_signal() {
        let detector = FeedbackDetector::new();

        // Neutral messages should return None
        assert_eq!(detector.detect("Can you help me with this?", "Some output"), None);
        assert_eq!(detector.detect("Show me the code", "Some output"), None);
    }

    #[test]
    fn test_retry_detection() {
        let detector = FeedbackDetector::new();

        // Exact retry
        assert!(detector.is_retry("Generate a report", "Generate a report"));

        // Similar retry (case insensitive)
        assert!(detector.is_retry("Generate a Report", "generate a report"));

        // Similar retry (with extra punctuation)
        assert!(detector.is_retry("Generate a report!", "Generate a report."));

        // Different requests
        assert!(!detector.is_retry("Generate a report", "Delete the file"));
        assert!(!detector.is_retry("Generate a report", "Show me the logs"));
    }

    #[tokio::test]
    async fn test_feedback_store_add_and_get() {
        let (mut store, _dir) = create_test_store();

        let feedback = Feedback::new(
            "agent-1",
            FeedbackType::ExplicitNegative,
            "Test context",
            "User input",
            "Agent output",
        );

        store.add(feedback).await.unwrap();

        let recent = store.get_recent("agent-1", 10).await;
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].agent_id, "agent-1");
    }

    #[tokio::test]
    async fn test_feedback_store_get_by_type() {
        let (mut store, _dir) = create_test_store();

        store
            .add(Feedback::new(
                "agent-1",
                FeedbackType::ExplicitNegative,
                "Context",
                "Input 1",
                "Output 1",
            ))
            .await
            .unwrap();

        store
            .add(Feedback::new(
                "agent-1",
                FeedbackType::Positive,
                "Context",
                "Input 2",
                "Output 2",
            ))
            .await
            .unwrap();

        store
            .add(Feedback::new(
                "agent-1",
                FeedbackType::ExplicitNegative,
                "Context",
                "Input 3",
                "Output 3",
            ))
            .await
            .unwrap();

        let negative = store.get_by_type(FeedbackType::ExplicitNegative, 10).await;
        assert_eq!(negative.len(), 2);

        let positive = store.get_by_type(FeedbackType::Positive, 10).await;
        assert_eq!(positive.len(), 1);
    }

    #[tokio::test]
    async fn test_feedback_store_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_path_buf();

        // Create store and add feedback
        {
            let mut store = FeedbackStore::new(path.clone()).unwrap();
            store
                .add(Feedback::new(
                    "agent-1",
                    FeedbackType::ExplicitNegative,
                    "Context",
                    "Input",
                    "Output",
                ))
                .await
                .unwrap();
        }

        // Create new store and verify persistence
        {
            let store = FeedbackStore::new(path).unwrap();
            let recent = store.get_recent("agent-1", 10).await;
            assert_eq!(recent.len(), 1);
        }
    }

    #[tokio::test]
    async fn test_auto_eval_process_turn() {
        let temp_dir = TempDir::new().unwrap();
        let mut eval = AutoEval::new(temp_dir.path().to_path_buf()).unwrap();

        // Negative feedback should be detected
        let feedback = eval
            .process_turn(
                "agent-1",
                "That's wrong, try again",
                "Here's the result",
                None,
                None,
            )
            .await
            .unwrap();

        assert!(feedback.is_some());
        assert_eq!(feedback.unwrap().feedback_type, FeedbackType::ExplicitNegative);
    }

    #[tokio::test]
    async fn test_auto_eval_error() {
        let temp_dir = TempDir::new().unwrap();
        let mut eval = AutoEval::new(temp_dir.path().to_path_buf()).unwrap();

        // Error should be recorded
        let feedback = eval
            .process_turn(
                "agent-1",
                "Do something",
                "Failed",
                None,
                Some("Connection timeout"),
            )
            .await
            .unwrap();

        assert!(feedback.is_some());
        let fb = feedback.unwrap();
        assert_eq!(fb.feedback_type, FeedbackType::Error);
        assert_eq!(fb.correction, Some("Connection timeout".to_string()));
    }

    #[tokio::test]
    async fn test_auto_eval_retry_detection() {
        let temp_dir = TempDir::new().unwrap();
        let mut eval = AutoEval::new(temp_dir.path().to_path_buf()).unwrap();

        // First request - no retry
        let feedback = eval
            .process_turn("agent-1", "Generate a report", "Here's the report", None, None)
            .await
            .unwrap();
        assert!(feedback.is_none());

        // Same request again - should be detected as retry
        let feedback = eval
            .process_turn(
                "agent-1",
                "Generate a report",
                "Here's another report",
                Some("Generate a report"),
                None,
            )
            .await
            .unwrap();

        assert!(feedback.is_some());
        assert_eq!(feedback.unwrap().feedback_type, FeedbackType::ImplicitRetry);
    }

    #[tokio::test]
    async fn test_auto_eval_summary() {
        let temp_dir = TempDir::new().unwrap();
        let mut eval = AutoEval::new(temp_dir.path().to_path_buf()).unwrap();

        // Add various feedback
        eval.process_turn("agent-1", "Wrong!", "Output", None, None)
            .await
            .unwrap();
        eval.process_turn("agent-1", "Thanks!", "Output", None, None)
            .await
            .unwrap();
        eval.process_turn("agent-1", "Error", "Output", None, Some("Failed"))
            .await
            .unwrap();

        let summary = eval.summary("agent-1");
        assert_eq!(summary.total, 3);
        assert_eq!(summary.positive, 1);
        assert_eq!(summary.negative, 1);
        assert_eq!(summary.errors, 1);
    }

    #[tokio::test]
    async fn test_improvement_generator() {
        let generator = ImprovementGenerator::new();

        // Not enough feedback
        let feedback: Vec<Feedback> = vec![];
        let improvements = generator.analyze(&feedback).await.unwrap();
        assert!(improvements.is_empty());

        // With enough retry feedback
        let feedback: Vec<Feedback> = (0..6)
            .map(|i| {
                Feedback::new(
                    "agent-1",
                    FeedbackType::ImplicitRetry,
                    "Context",
                    format!("Input {}", i),
                    "Output",
                )
            })
            .collect();

        let improvements = generator.analyze(&feedback).await.unwrap();
        assert!(!improvements.is_empty());
        assert!(improvements.iter().any(|i| i.category == "clarity"));
    }
}
