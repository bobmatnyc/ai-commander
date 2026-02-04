//! Feedback detection from user messages.

use regex::Regex;
use std::collections::HashSet;
use tracing::debug;

use super::types::FeedbackType;

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
        let current_words: HashSet<&str> = current_normalized.split_whitespace().collect();
        let previous_words: HashSet<&str> = previous_normalized.split_whitespace().collect();

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
