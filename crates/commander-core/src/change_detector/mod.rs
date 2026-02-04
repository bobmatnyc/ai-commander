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

mod patterns;
#[cfg(test)]
mod tests;
mod types;

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use regex::Regex;

pub use self::patterns::{classify_change, default_ignore_patterns, default_significant_patterns, summarize_change};
pub use self::types::{ChangeEvent, ChangeNotification, ChangeType, Significance};

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
            significant_patterns: default_significant_patterns(),
            ignore_patterns: default_ignore_patterns(),
        }
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
        let (change_type, significance) = classify_change(&new_lines, &self.significant_patterns);

        // Stage 5: Generate summary
        let summary = summarize_change(&new_lines, &change_type, &self.significant_patterns);

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
        Self::new(Duration::from_millis(500), Duration::from_secs(5))
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
