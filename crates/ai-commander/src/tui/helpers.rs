//! Helper functions for TUI operations.

use commander_core::notification_parser::strip_ansi;
use regex::Regex;
use std::sync::LazyLock;

/// Regex to match and remove model/framework/context stat patterns like [model|Claude MPM|69%].
static STAT_LINE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\[[^\]]*\|[^\]]*\|[0-9]+%\]").expect("Invalid stat line regex"));

/// Regex to match partial stat fragments that may leak through.
static PARTIAL_STAT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:MPM|Opus|Sonnet|Claude)[|\s][^)]*%\]?").expect("Invalid partial stat regex"));

/// Remove stat line patterns from text.
///
/// Cleans patterns like `[model|Claude MPM|69%]` and partial fragments.
fn strip_stat_patterns(text: &str) -> String {
    let cleaned = STAT_LINE_REGEX.replace_all(text, "");
    let cleaned = PARTIAL_STAT_REGEX.replace_all(&cleaned, "");
    cleaned.trim().to_string()
}

/// Extract a preview of the last meaningful line when session is ready.
///
/// Returns a clean, ANSI-stripped preview suitable for display.
/// Removes ANSI codes, stat line patterns, and UI noise.
pub fn extract_ready_preview(output: &str) -> String {
    // Strip ANSI codes first for clean analysis
    let clean_output = strip_ansi(output);

    // Look for the last non-UI-noise line before the prompt
    let lines: Vec<&str> = clean_output.lines().rev()
        .filter(|l| {
            let trimmed = l.trim();
            let lower = trimmed.to_lowercase();
            !trimmed.is_empty()
                && !commander_core::output_filter::is_ui_noise(trimmed)
                && !trimmed.contains('\u{276f}')  // Skip prompt lines
                && !trimmed.starts_with("\u{2500}\u{2500}\u{2500}")  // Skip separator
                && !trimmed.starts_with("\u{256d}")  // Skip box drawing
                && !trimmed.starts_with("\u{2570}")  // Skip box drawing
                && !trimmed.starts_with("\u{2502}")  // Skip box drawing
                && !lower.contains("bypass permissions")  // Skip Claude Code hint
                && !lower.contains("shift+tab")  // Skip Claude Code hint
                && !lower.contains("shift-tab")  // Skip Claude Code hint
                && !trimmed.contains("\u{23f5}")  // Skip play button indicators
                && !trimmed.contains("\u{23fa}")  // Skip record indicators
        })
        .take(1)
        .collect();

    lines.first()
        .map(|s| {
            let trimmed = s.trim();
            // Additional check - skip if it looks like UI noise we missed
            if trimmed.len() < 5 || trimmed.chars().all(|c| !c.is_alphanumeric()) {
                return String::new();
            }
            // Strip any remaining stat patterns and return clean preview
            strip_stat_patterns(trimmed)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_stat_patterns_full() {
        let input = "Waiting for input [model|Claude MPM|69%]";
        assert_eq!(strip_stat_patterns(input), "Waiting for input");
    }

    #[test]
    fn test_strip_stat_patterns_with_ansi_remnants() {
        // After ANSI stripping, we might see fragments like this
        let input = "duetto-people - Waiting for input (MPM|69%]";
        let result = strip_stat_patterns(input);
        assert!(!result.contains("MPM|"));
        assert!(!result.contains("%]"));
    }

    #[test]
    fn test_strip_stat_patterns_partial_fragment() {
        let input = "Working on task MPM|70%]";
        let result = strip_stat_patterns(input);
        assert!(!result.contains("MPM|"));
    }

    #[test]
    fn test_strip_stat_patterns_opus_variant() {
        let input = "Ready [Opus|Claude Opus|95%]";
        let result = strip_stat_patterns(input);
        assert_eq!(result, "Ready");
    }

    #[test]
    fn test_strip_stat_patterns_sonnet_variant() {
        let input = "Processing Sonnet|50%]";
        let result = strip_stat_patterns(input);
        assert!(!result.contains("Sonnet|"));
    }

    #[test]
    fn test_strip_stat_patterns_no_change_normal_text() {
        let input = "Just a normal preview text";
        assert_eq!(strip_stat_patterns(input), "Just a normal preview text");
    }

    #[test]
    fn test_extract_ready_preview_removes_stat_line() {
        let output = "Some activity\nWaiting for input [model|Claude MPM|69%]\n❯";
        let preview = extract_ready_preview(output);
        assert!(!preview.contains("MPM"));
        assert!(!preview.contains("%]"));
        // Should contain the meaningful part
        assert!(preview.contains("Waiting for input") || preview.contains("activity"));
    }

    #[test]
    fn test_extract_ready_preview_with_ansi_codes() {
        // Simulate output with ANSI codes
        let output = "Activity line\x1B[0m\nWaiting \x1B[90m[model|MPM|70%]\x1B[0m\n❯";
        let preview = extract_ready_preview(output);
        assert!(!preview.contains("\x1B"));
        assert!(!preview.contains("MPM|"));
    }
}
