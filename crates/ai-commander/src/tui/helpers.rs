//! Helper functions for TUI operations.

use commander_core::notification_parser::strip_ansi;

/// Extract a preview of the last meaningful line when session is ready.
///
/// Returns a clean, ANSI-stripped preview suitable for display.
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
            // Return clean preview (already ANSI-stripped)
            trimmed.to_string()
        })
        .unwrap_or_default()
}
