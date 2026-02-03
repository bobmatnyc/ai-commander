//! Smart change detection for session output monitoring.
//!
//! This module provides deterministic change detection to reduce inference costs
//! by only invoking agents when meaningful changes occur in session output.
//!
//! # Architecture
//!
//! The change detection system uses a multi-stage approach:
//! 1. **Hash comparison** - Quick check if output changed at all
//! 2. **Noise filtering** - Remove UI artifacts (spinners, box drawing, ANSI codes)
//! 3. **Diff generation** - Find new lines compared to previous output
//! 4. **Pattern classification** - Match against significant/ignore patterns
//! 5. **Significance scoring** - Determine if LLM analysis is needed

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use regex::Regex;

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

/// Pattern-based change detector for session output.
///
/// Uses hash comparison for quick rejection, then pattern matching
/// to classify changes and determine significance.
pub struct ChangeDetector {
    /// Previous output hash for quick comparison
    prev_hash: Option<u64>,
    /// Previous output for diff generation
    prev_output: Option<String>,
    /// Compiled patterns that indicate significant changes
    significant_patterns: Vec<(Regex, ChangeType, Significance)>,
    /// Compiled patterns for UI noise to ignore
    ignore_patterns: Vec<Regex>,
}

impl Default for ChangeDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeDetector {
    /// Create a new change detector with default patterns.
    pub fn new() -> Self {
        Self {
            prev_hash: None,
            prev_output: None,
            significant_patterns: Self::default_significant_patterns(),
            ignore_patterns: Self::default_ignore_patterns(),
        }
    }

    /// Build default patterns that indicate significant changes.
    ///
    /// Patterns are checked in order, and the HIGHEST significance match wins.
    /// More specific patterns (like test results) should come before general ones.
    fn default_significant_patterns() -> Vec<(Regex, ChangeType, Significance)> {
        vec![
            // Test results (Medium significance) - MUST come before completion/error
            // Patterns are specific to avoid matching general error messages
            (
                // Numeric test results: "42 tests passed", "3 failed", "10 passed, 2 failed"
                Regex::new(r"\d+\s+(tests?\s+)?(passed|failed|skipped|ignored)").unwrap(),
                ChangeType::Progress,
                Significance::Medium,
            ),
            (
                // Test suite summaries at line start: "Tests passed", "All tests passed"
                Regex::new(r"(?i)^(all\s+)?tests?\s+(passed|failed|ok|fail)").unwrap(),
                ChangeType::Progress,
                Significance::Medium,
            ),
            (
                // Spec/check results: "specs passed", "checks failed"
                Regex::new(r"(?i)(specs?|checks?)\s+(passed|failed|ok|fail)").unwrap(),
                ChangeType::Progress,
                Significance::Medium,
            ),
            // Completion indicators (High significance)
            // Note: "passed" without number prefix indicates general completion
            (
                Regex::new(r"(?i)\b(completed?|finished|done|success(ful)?)\b").unwrap(),
                ChangeType::Completion,
                Significance::High,
            ),
            (
                Regex::new(r"(?i)^passed\b").unwrap(), // "passed" at start of line only
                ChangeType::Completion,
                Significance::High,
            ),
            // Error indicators (High/Critical significance)
            (
                Regex::new(r"(?i)\b(error|failed|failure|exception|panic|fatal)\b").unwrap(),
                ChangeType::Error,
                Significance::High,
            ),
            (
                Regex::new(r"(?i)\b(segfault|segmentation fault|core dumped|killed|oom)\b").unwrap(),
                ChangeType::Error,
                Significance::Critical,
            ),
            // Input needed (High significance)
            (
                Regex::new(r"(?i)(waiting for|awaiting|requires?) (input|response|confirmation)").unwrap(),
                ChangeType::WaitingForInput,
                Significance::High,
            ),
            (
                Regex::new(r"(?i)\b(confirm|proceed|continue)\s*\?\s*(\[y/n\])?").unwrap(),
                ChangeType::WaitingForInput,
                Significance::High,
            ),
            (
                Regex::new(r"(?i)(enter|type|input|provide)\s+(your|a|the)?\s*(password|passphrase|token|key)").unwrap(),
                ChangeType::WaitingForInput,
                Significance::High,
            ),
            // File changes (Low significance)
            (
                Regex::new(r"(?i)(creat(ed?|ing)|modif(y|ied|ying)|delet(ed?|ing)|writ(e|ing|ten))\s+\S+").unwrap(),
                ChangeType::Progress,
                Significance::Low,
            ),
            // Build progress (Low-Medium significance)
            (
                Regex::new(r"(?i)(compil(e|ing)|build(ing)?|link(ing)?)\s+").unwrap(),
                ChangeType::Progress,
                Significance::Low,
            ),
            (
                Regex::new(r"(?i)(install(ed|ing)?|download(ed|ing)?)\s+").unwrap(),
                ChangeType::Progress,
                Significance::Low,
            ),
            // Git operations (Medium significance)
            (
                Regex::new(r"(?i)(commit(ted)?|push(ed)?|pull(ed)?|merg(e|ed|ing))\b").unwrap(),
                ChangeType::Progress,
                Significance::Medium,
            ),
        ]
    }

    /// Build default patterns for UI noise to ignore.
    fn default_ignore_patterns() -> Vec<Regex> {
        vec![
            // Spinner characters (various Unicode spinners)
            Regex::new(r"^[\u{2800}-\u{28FF}]").unwrap(), // Braille patterns
            Regex::new(r"^[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]").unwrap(),
            Regex::new(r"^[◐◑◒◓◴◵◶◷]").unwrap(),
            Regex::new(r"^[⣾⣽⣻⢿⡿⣟⣯⣷]").unwrap(),
            // Box drawing characters (UI frames)
            Regex::new(r"^[─│┌┐└┘├┤┬┴┼╭╮╯╰╱╲╳]").unwrap(),
            Regex::new(r"^[═║╔╗╚╝╠╣╦╩╬]").unwrap(),
            // ANSI escape sequences
            Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap(),
            // Progress bars
            Regex::new(r"[\[=\->\s\]]{10,}").unwrap(),
            Regex::new(r"\d+%\s*[\[█▓▒░\s\]]*").unwrap(),
            // Timestamps only
            Regex::new(r"^\d{2}:\d{2}(:\d{2})?\s*$").unwrap(),
            // Claude Code specific UI noise
            Regex::new(r"[▐▛▜▌▝▘]").unwrap(), // Logo chars
            Regex::new(r"(?i)(thinking|spelunking|processing)\.{0,3}$").unwrap(),
            Regex::new(r"(?i)ctrl\+[a-z]").unwrap(),
            // MCP tool invocation noise
            Regex::new(r"\(MCP\)\(").unwrap(),
        ]
    }

    /// Add a custom significant pattern.
    pub fn add_significant_pattern(
        &mut self,
        pattern: &str,
        change_type: ChangeType,
        significance: Significance,
    ) -> Result<(), regex::Error> {
        let regex = Regex::new(pattern)?;
        self.significant_patterns.push((regex, change_type, significance));
        Ok(())
    }

    /// Add a custom ignore pattern.
    pub fn add_ignore_pattern(&mut self, pattern: &str) -> Result<(), regex::Error> {
        let regex = Regex::new(pattern)?;
        self.ignore_patterns.push(regex);
        Ok(())
    }

    /// Reset the detector state (for new sessions).
    pub fn reset(&mut self) {
        self.prev_hash = None;
        self.prev_output = None;
    }

    /// Detect changes between previous and current output.
    ///
    /// This is the main entry point for change detection. It:
    /// 1. Performs quick hash comparison
    /// 2. Cleans output to remove noise
    /// 3. Finds new lines via diff
    /// 4. Classifies the change type and significance
    pub fn detect(&mut self, current_output: &str) -> ChangeEvent {
        // Stage 1: Quick hash comparison
        let current_hash = self.hash_output(current_output);
        if self.prev_hash == Some(current_hash) {
            return ChangeEvent::none();
        }

        // Stage 2: Clean output (remove UI noise)
        let cleaned = self.clean_output(current_output);
        let prev_cleaned = self
            .prev_output
            .as_ref()
            .map(|s| self.clean_output(s))
            .unwrap_or_default();

        // Stage 3: Find new lines
        let new_lines = self.find_new_lines(&prev_cleaned, &cleaned);

        // Stage 4: Classify change type and significance
        let (change_type, significance) = self.classify_change(&new_lines);

        // Stage 5: Generate summary
        let summary = self.summarize_change(&new_lines, &change_type);

        // Update state for next detection
        self.prev_hash = Some(current_hash);
        self.prev_output = Some(current_output.to_string());

        ChangeEvent {
            change_type,
            summary,
            diff_lines: new_lines,
            significance,
        }
    }

    /// Compute hash of output for quick comparison.
    fn hash_output(&self, output: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        output.hash(&mut hasher);
        hasher.finish()
    }

    /// Clean output by removing UI noise patterns.
    fn clean_output(&self, output: &str) -> String {
        output
            .lines()
            .filter(|line| !self.is_noise(line))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Check if a line is UI noise that should be ignored.
    fn is_noise(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return true;
        }

        // Check against ignore patterns
        for pattern in &self.ignore_patterns {
            if pattern.is_match(trimmed) {
                return true;
            }
        }

        // Also use the existing output_filter checks
        crate::output_filter::is_ui_noise(trimmed)
    }

    /// Find lines in current that are not in previous.
    fn find_new_lines(&self, prev: &str, current: &str) -> Vec<String> {
        let prev_lines: HashSet<&str> = prev.lines().map(|l| l.trim()).collect();
        let mut new_lines = Vec::new();

        for line in current.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !prev_lines.contains(trimmed) {
                new_lines.push(trimmed.to_string());
            }
        }

        new_lines
    }

    /// Classify the change based on new lines.
    ///
    /// Patterns are evaluated in order. For each line, the FIRST matching pattern
    /// determines the type. The significance is then taken as the maximum across
    /// all matched types. This allows specific patterns (like test results) to
    /// be listed first and take precedence over general patterns (like "failed").
    fn classify_change(&self, new_lines: &[String]) -> (ChangeType, Significance) {
        if new_lines.is_empty() {
            return (ChangeType::None, Significance::Ignore);
        }

        let mut best_type = ChangeType::Addition;
        let mut best_significance = Significance::Low;

        for line in new_lines {
            // Find the FIRST pattern that matches this line (order matters)
            for (pattern, change_type, significance) in &self.significant_patterns {
                if pattern.is_match(line) {
                    // First match for this line wins
                    // Update best if this significance is higher
                    if *significance > best_significance {
                        best_significance = *significance;
                        best_type = change_type.clone();
                    } else if *significance == best_significance
                        && !matches!(best_type, ChangeType::Addition)
                    {
                        // Same significance, keep existing type (don't downgrade)
                    }
                    // Stop checking patterns for this line (first match wins)
                    break;
                }
            }
        }

        (best_type, best_significance)
    }

    /// Generate a human-readable summary of the change.
    fn summarize_change(&self, lines: &[String], change_type: &ChangeType) -> String {
        if lines.is_empty() {
            return String::new();
        }

        let type_prefix = match change_type {
            ChangeType::None => "",
            ChangeType::Addition => "New output: ",
            ChangeType::Modification => "Changed: ",
            ChangeType::Completion => "Completed: ",
            ChangeType::Error => "Error: ",
            ChangeType::WaitingForInput => "Waiting for input: ",
            ChangeType::Progress => "Progress: ",
        };

        // Take the most relevant line(s) for the summary
        let relevant_line = lines
            .iter()
            .find(|l| {
                // Prefer lines that match significant patterns
                self.significant_patterns
                    .iter()
                    .any(|(p, _, _)| p.is_match(l))
            })
            .or_else(|| lines.first())
            .map(|s| s.as_str())
            .unwrap_or("");

        // Truncate long lines
        let truncated = if relevant_line.len() > 100 {
            format!("{}...", &relevant_line[..97])
        } else {
            relevant_line.to_string()
        };

        format!(
            "{}{} (+{} lines)",
            type_prefix,
            truncated,
            lines.len()
        )
    }
}

/// Smart poller with adaptive intervals based on change detection.
///
/// Speeds up polling when activity is detected, slows down when idle.
pub struct SmartPoller {
    /// Base polling interval (fastest rate)
    base_interval: Duration,
    /// Current polling interval (adaptive)
    current_interval: Duration,
    /// Maximum interval when idle (slowest rate)
    max_interval: Duration,
    /// Number of consecutive "no change" detections
    idle_count: u32,
    /// Threshold before starting to slow down
    idle_threshold: u32,
}

impl Default for SmartPoller {
    fn default() -> Self {
        Self::new(
            Duration::from_millis(500),
            Duration::from_secs(5),
        )
    }
}

impl SmartPoller {
    /// Create a new smart poller with specified intervals.
    ///
    /// # Arguments
    /// * `base_interval` - Fastest polling rate (when active)
    /// * `max_interval` - Slowest polling rate (when idle)
    pub fn new(base_interval: Duration, max_interval: Duration) -> Self {
        Self {
            base_interval,
            current_interval: base_interval,
            max_interval,
            idle_count: 0,
            idle_threshold: 3,
        }
    }

    /// Get the current polling interval.
    pub fn interval(&self) -> Duration {
        self.current_interval
    }

    /// Update interval based on a change event.
    ///
    /// Returns the interval to use for the next poll.
    pub fn next_interval(&mut self, change: &ChangeEvent) -> Duration {
        match change.significance {
            Significance::Ignore => {
                // No change - slow down polling
                self.idle_count += 1;
                if self.idle_count > self.idle_threshold {
                    // Exponential backoff (2x each time, capped at max)
                    self.current_interval = (self.current_interval * 2).min(self.max_interval);
                }
            }
            Significance::Low => {
                // Minor activity - maintain or slightly increase
                self.idle_count = 0;
                self.current_interval =
                    (self.current_interval + self.base_interval).min(self.max_interval);
            }
            Significance::Medium => {
                // Moderate activity - reset to base or slightly above
                self.idle_count = 0;
                self.current_interval = self.base_interval * 2;
            }
            Significance::High | Significance::Critical => {
                // High activity - maximum polling rate
                self.idle_count = 0;
                self.current_interval = self.base_interval;
            }
        }

        self.current_interval
    }

    /// Reset to base interval (e.g., after user interaction).
    pub fn reset(&mut self) {
        self.current_interval = self.base_interval;
        self.idle_count = 0;
    }

    /// Check if currently in idle state.
    pub fn is_idle(&self) -> bool {
        self.idle_count > self.idle_threshold
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_event_none() {
        let event = ChangeEvent::none();
        assert_eq!(event.change_type, ChangeType::None);
        assert_eq!(event.significance, Significance::Ignore);
        assert!(!event.is_meaningful());
        assert!(!event.requires_notification());
    }

    #[test]
    fn test_significance_ordering() {
        assert!(Significance::Ignore < Significance::Low);
        assert!(Significance::Low < Significance::Medium);
        assert!(Significance::Medium < Significance::High);
        assert!(Significance::High < Significance::Critical);
    }

    #[test]
    fn test_detector_no_change() {
        let mut detector = ChangeDetector::new();
        let output = "Some output text\nMore text";

        // First detection - establishes baseline
        let event1 = detector.detect(output);
        assert!(matches!(event1.change_type, ChangeType::Addition));

        // Same output - no change
        let event2 = detector.detect(output);
        assert_eq!(event2.change_type, ChangeType::None);
        assert_eq!(event2.significance, Significance::Ignore);
    }

    #[test]
    fn test_detector_detects_addition() {
        let mut detector = ChangeDetector::new();

        detector.detect("Line 1");
        let event = detector.detect("Line 1\nLine 2\nLine 3");

        assert_eq!(event.change_type, ChangeType::Addition);
        assert!(event.diff_lines.len() >= 2);
    }

    #[test]
    fn test_detector_detects_completion() {
        let mut detector = ChangeDetector::new();

        detector.detect("Starting task...");
        let event = detector.detect("Starting task...\nTask completed successfully!");

        assert_eq!(event.change_type, ChangeType::Completion);
        assert_eq!(event.significance, Significance::High);
    }

    #[test]
    fn test_detector_detects_error() {
        let mut detector = ChangeDetector::new();

        detector.detect("Running tests...");
        let event = detector.detect("Running tests...\nError: test failed!");

        assert_eq!(event.change_type, ChangeType::Error);
        assert!(event.significance >= Significance::High);
    }

    #[test]
    fn test_detector_detects_waiting_for_input() {
        let mut detector = ChangeDetector::new();

        detector.detect("Installing package...");
        let event = detector.detect("Installing package...\nProceed? [y/n]");

        assert_eq!(event.change_type, ChangeType::WaitingForInput);
        assert_eq!(event.significance, Significance::High);
    }

    #[test]
    fn test_detector_filters_noise() {
        let mut detector = ChangeDetector::new();

        // First establish baseline with noise
        detector.detect("Content line\n⠋ Loading...");

        // Add new content with different spinner
        let event = detector.detect("Content line\n⠙ Loading...\nNew actual content");

        // Should detect the new content, not spinner changes
        assert!(event.diff_lines.iter().any(|l| l.contains("actual content")));
    }

    #[test]
    fn test_detector_detects_test_results() {
        let mut detector = ChangeDetector::new();

        detector.detect("Running tests");
        let event = detector.detect("Running tests\n42 tests passed, 3 failed");

        assert_eq!(event.change_type, ChangeType::Progress);
        assert_eq!(event.significance, Significance::Medium);
    }

    #[test]
    fn test_detector_reset() {
        let mut detector = ChangeDetector::new();

        detector.detect("Some output");
        detector.reset();

        // After reset, same output should be detected as new
        let event = detector.detect("Some output");
        assert!(!matches!(event.change_type, ChangeType::None));
    }

    #[test]
    fn test_smart_poller_default() {
        let poller = SmartPoller::default();
        assert_eq!(poller.interval(), Duration::from_millis(500));
    }

    #[test]
    fn test_smart_poller_speeds_up_on_activity() {
        let mut poller = SmartPoller::new(
            Duration::from_millis(100),
            Duration::from_secs(10),
        );

        // Slow down first
        for _ in 0..10 {
            poller.next_interval(&ChangeEvent::none());
        }
        let slow_interval = poller.interval();

        // Activity should speed up
        let event = ChangeEvent {
            change_type: ChangeType::Error,
            significance: Significance::High,
            ..Default::default()
        };
        poller.next_interval(&event);

        assert!(poller.interval() < slow_interval);
    }

    #[test]
    fn test_smart_poller_slows_down_when_idle() {
        let mut poller = SmartPoller::new(
            Duration::from_millis(100),
            Duration::from_secs(10),
        );

        let initial = poller.interval();

        // Simulate idle period
        for _ in 0..10 {
            poller.next_interval(&ChangeEvent::none());
        }

        assert!(poller.interval() > initial);
        assert!(poller.is_idle());
    }

    #[test]
    fn test_smart_poller_respects_max_interval() {
        let mut poller = SmartPoller::new(
            Duration::from_millis(100),
            Duration::from_secs(1),
        );

        // Try to slow down a lot
        for _ in 0..100 {
            poller.next_interval(&ChangeEvent::none());
        }

        assert!(poller.interval() <= Duration::from_secs(1));
    }

    #[test]
    fn test_smart_poller_reset() {
        let mut poller = SmartPoller::new(
            Duration::from_millis(100),
            Duration::from_secs(10),
        );

        // Slow down
        for _ in 0..10 {
            poller.next_interval(&ChangeEvent::none());
        }

        // Reset
        poller.reset();

        assert_eq!(poller.interval(), Duration::from_millis(100));
        assert!(!poller.is_idle());
    }

    #[test]
    fn test_change_notification_fields() {
        let notification = ChangeNotification {
            session_id: "test-session".to_string(),
            summary: "Task completed".to_string(),
            requires_action: false,
            change_type: ChangeType::Completion,
            significance: Significance::High,
        };

        assert_eq!(notification.session_id, "test-session");
        assert!(!notification.requires_action);
    }

    #[test]
    fn test_custom_patterns() {
        let mut detector = ChangeDetector::new();

        // Add custom pattern for deployment
        detector
            .add_significant_pattern(
                r"(?i)deployed to \w+",
                ChangeType::Completion,
                Significance::Critical,
            )
            .unwrap();

        detector.detect("Starting deployment");
        let event = detector.detect("Starting deployment\nDeployed to production!");

        assert_eq!(event.change_type, ChangeType::Completion);
        assert_eq!(event.significance, Significance::Critical);
    }

    #[test]
    fn test_summary_truncation() {
        let mut detector = ChangeDetector::new();
        detector.detect("");

        let long_line = "x".repeat(200);
        let event = detector.detect(&long_line);

        // Summary should be truncated
        assert!(event.summary.len() < 200);
        assert!(event.summary.contains("..."));
    }

    #[test]
    fn test_significance_meaningful_threshold() {
        // Low significance is not meaningful
        let low_event = ChangeEvent {
            significance: Significance::Low,
            ..Default::default()
        };
        assert!(!low_event.is_meaningful());

        // Medium significance is meaningful
        let medium_event = ChangeEvent {
            significance: Significance::Medium,
            ..Default::default()
        };
        assert!(medium_event.is_meaningful());
    }

    #[test]
    fn test_notification_threshold() {
        // Medium significance does not require notification
        let medium_event = ChangeEvent {
            significance: Significance::Medium,
            ..Default::default()
        };
        assert!(!medium_event.requires_notification());

        // High significance requires notification
        let high_event = ChangeEvent {
            significance: Significance::High,
            ..Default::default()
        };
        assert!(high_event.requires_notification());
    }
}
