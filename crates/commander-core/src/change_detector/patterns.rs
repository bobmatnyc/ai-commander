//! Pattern definitions and classification for change detection.

use regex::Regex;

use super::{ChangeType, Significance};

/// Build default patterns that indicate significant changes.
///
/// Patterns are checked in order, and the HIGHEST significance match wins.
/// More specific patterns (like test results) should come before general ones.
pub fn default_significant_patterns() -> Vec<(Regex, ChangeType, Significance)> {
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
            Regex::new(r"(?i)(waiting for|awaiting|requires?) (input|response|confirmation)")
                .unwrap(),
            ChangeType::WaitingForInput,
            Significance::High,
        ),
        (
            Regex::new(r"(?i)\b(confirm|proceed|continue)\s*\?\s*(\[y/n\])?").unwrap(),
            ChangeType::WaitingForInput,
            Significance::High,
        ),
        (
            Regex::new(
                r"(?i)(enter|type|input|provide)\s+(your|a|the)?\s*(password|passphrase|token|key)",
            )
            .unwrap(),
            ChangeType::WaitingForInput,
            Significance::High,
        ),
        // File changes (Low significance)
        (
            Regex::new(r"(?i)(creat(ed?|ing)|modif(y|ied|ying)|delet(ed?|ing)|writ(e|ing|ten))\s+\S+")
                .unwrap(),
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
pub fn default_ignore_patterns() -> Vec<Regex> {
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

/// Classify the change based on new lines.
///
/// Patterns are evaluated in order. For each line, the FIRST matching pattern
/// determines the type. The significance is then taken as the maximum across
/// all matched types. This allows specific patterns (like test results) to
/// be listed first and take precedence over general patterns (like "failed").
pub fn classify_change(
    new_lines: &[String],
    significant_patterns: &[(Regex, ChangeType, Significance)],
) -> (ChangeType, Significance) {
    if new_lines.is_empty() {
        return (ChangeType::None, Significance::Ignore);
    }

    let mut best_type = ChangeType::Addition;
    let mut best_significance = Significance::Low;

    for line in new_lines {
        // Find the FIRST pattern that matches this line (order matters)
        for (pattern, change_type, significance) in significant_patterns {
            if pattern.is_match(line) {
                // First match for this line wins
                // Update best if this significance is higher
                if *significance > best_significance {
                    best_significance = *significance;
                    best_type = change_type.clone();
                }
                // Stop checking patterns for this line (first match wins)
                break;
            }
        }
    }

    (best_type, best_significance)
}

/// Generate a human-readable summary of the change.
pub fn summarize_change(
    lines: &[String],
    change_type: &ChangeType,
    significant_patterns: &[(Regex, ChangeType, Significance)],
) -> String {
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
            significant_patterns
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

    format!("{}{} (+{} lines)", type_prefix, truncated, lines.len())
}
