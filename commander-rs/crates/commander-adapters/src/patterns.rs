//! Pattern matching utilities for output analysis.

use regex::Regex;
use std::sync::OnceLock;

/// A compiled pattern for matching output.
#[derive(Debug)]
pub struct Pattern {
    /// Human-readable name for this pattern.
    pub name: &'static str,
    /// The regex pattern.
    regex: Regex,
    /// Confidence level when this pattern matches (0.0 - 1.0).
    pub confidence: f32,
}

impl Pattern {
    /// Creates a new pattern.
    pub fn new(name: &'static str, pattern: &str, confidence: f32) -> Self {
        Self {
            name,
            regex: Regex::new(pattern).expect("Invalid regex pattern"),
            confidence,
        }
    }

    /// Checks if the pattern matches the given text.
    pub fn matches(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }

    /// Finds all matches in the text.
    pub fn find_all<'a>(&self, text: &'a str) -> Vec<&'a str> {
        self.regex.find_iter(text).map(|m| m.as_str()).collect()
    }

    /// Extracts captured groups from the first match.
    pub fn captures(&self, text: &str) -> Option<Vec<String>> {
        self.regex.captures(text).map(|caps| {
            caps.iter()
                .skip(1) // Skip the full match
                .filter_map(|m| m.map(|m| m.as_str().to_string()))
                .collect()
        })
    }
}

/// Common patterns for Claude Code output.
pub mod claude_code {
    use super::*;

    /// Returns idle detection patterns for Claude Code.
    pub fn idle_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| {
            vec![
                Pattern::new("prompt", r"(?m)^>\s*$", 0.9),
                Pattern::new("waiting", r"(?i)waiting for input", 0.95),
                Pattern::new("ready", r"(?i)ready\s*$", 0.8),
                Pattern::new("idle_marker", r"\[IDLE\]", 1.0),
            ]
        })
    }

    /// Returns error detection patterns for Claude Code.
    pub fn error_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| {
            vec![
                Pattern::new("error", r"(?im)^error:", 0.95),
                Pattern::new("exception", r"(?i)exception|traceback", 0.9),
                Pattern::new("failed", r"(?i)failed|failure", 0.85),
                Pattern::new("permission_denied", r"(?i)permission denied", 0.95),
                Pattern::new("not_found", r"(?i)not found|no such file", 0.9),
            ]
        })
    }

    /// Returns patterns indicating work is in progress.
    pub fn working_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| {
            vec![
                Pattern::new("thinking", r"(?i)thinking|processing", 0.9),
                Pattern::new("writing", r"(?i)writing|creating|updating", 0.85),
                Pattern::new("reading", r"(?i)reading|analyzing", 0.8),
                Pattern::new("running", r"(?i)running|executing", 0.85),
            ]
        })
    }
}

/// Common patterns for MPM output.
pub mod mpm {
    use super::*;

    /// Returns idle detection patterns for MPM.
    pub fn idle_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| {
            vec![
                Pattern::new("pm_ready", r"(?i)PM ready", 0.95),
                Pattern::new("awaiting", r"(?i)awaiting instructions", 0.95),
                Pattern::new("prompt", r"(?m)^>\s*$", 0.9),
                Pattern::new("idle_marker", r"\[IDLE\]", 1.0),
            ]
        })
    }

    /// Returns error detection patterns for MPM.
    pub fn error_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| {
            vec![
                Pattern::new("error", r"(?im)^error:", 0.95),
                Pattern::new("exception", r"(?i)exception|traceback", 0.9),
                Pattern::new("failed", r"(?i)failed|failure", 0.85),
                Pattern::new("agent_error", r"(?i)agent.*error", 0.9),
            ]
        })
    }

    /// Returns patterns indicating work is in progress.
    pub fn working_patterns() -> &'static [Pattern] {
        static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| {
            vec![
                Pattern::new("delegating", r"(?i)delegating|assigning", 0.9),
                Pattern::new("coordinating", r"(?i)coordinating|orchestrating", 0.85),
                Pattern::new("processing", r"(?i)processing|working", 0.8),
            ]
        })
    }
}

/// Analyzes text against a set of patterns, returning the best match.
pub fn best_match<'a>(text: &str, patterns: &'a [Pattern]) -> Option<&'a Pattern> {
    patterns
        .iter()
        .filter(|p| p.matches(text))
        .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
}

/// Checks if any pattern in the set matches.
pub fn any_match(text: &str, patterns: &[Pattern]) -> bool {
    patterns.iter().any(|p| p.matches(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_matches() {
        let pattern = Pattern::new("test", r"hello \w+", 0.9);
        assert!(pattern.matches("hello world"));
        assert!(!pattern.matches("goodbye world"));
    }

    #[test]
    fn test_pattern_captures() {
        let pattern = Pattern::new("test", r"hello (\w+)", 0.9);
        let caps = pattern.captures("hello world").unwrap();
        assert_eq!(caps, vec!["world"]);
    }

    #[test]
    fn test_claude_code_idle_patterns() {
        let patterns = claude_code::idle_patterns();
        assert!(any_match("> ", patterns));
        assert!(any_match("[IDLE]", patterns));
        assert!(any_match("Waiting for input", patterns));
        assert!(!any_match("Processing your request...", patterns));
    }

    #[test]
    fn test_claude_code_error_patterns() {
        let patterns = claude_code::error_patterns();
        assert!(any_match("Error: something went wrong", patterns));
        assert!(any_match("Permission denied", patterns));
        assert!(!any_match("All good!", patterns));
    }

    #[test]
    fn test_best_match() {
        let patterns = claude_code::idle_patterns();
        let best = best_match("[IDLE]", patterns);
        assert!(best.is_some());
        assert_eq!(best.unwrap().name, "idle_marker");
        assert_eq!(best.unwrap().confidence, 1.0);
    }

    #[test]
    fn test_mpm_idle_patterns() {
        let patterns = mpm::idle_patterns();
        assert!(any_match("PM ready", patterns));
        assert!(any_match("Awaiting instructions", patterns));
        assert!(!any_match("Processing task...", patterns));
    }

    #[test]
    fn test_mpm_error_patterns() {
        let patterns = mpm::error_patterns();
        assert!(any_match("Error: agent failed", patterns));
        assert!(any_match("Agent error occurred", patterns));
    }
}
