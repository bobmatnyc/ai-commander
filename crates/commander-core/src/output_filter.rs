//! Output filtering utilities for Claude Code and other adapters.
//!
//! Provides functions to filter UI noise from terminal output, detect when
//! Claude Code is ready for input, and find new lines in output.

use std::collections::HashSet;

/// Spinner characters that indicate processing activity.
const SPINNER_CHARS: [char; 13] = [
    '\u{2733}', // ✳
    '\u{2736}', // ✶
    '\u{273B}', // ✻
    '\u{273D}', // ✽
    '\u{2722}', // ✢
    '\u{23FA}', // ⏺
    '\u{00B7}', // ·
    '\u{25CF}', // ●
    '\u{25CB}', // ○
    '\u{25D0}', // ◐
    '\u{25D1}', // ◑
    '\u{25D2}', // ◒
    '\u{25D3}', // ◓
];

/// Check if a line is Claude Code UI noise that should be filtered out.
///
/// This detects:
/// - Prompt lines (echoed user input)
/// - Spinner/progress indicators
/// - Box drawing characters (status bars)
/// - Claude Code branding
/// - Thinking indicators
/// - Status messages
pub fn is_ui_noise(line: &str) -> bool {
    // Prompt lines - echoed user input from Claude Code
    // Matches: [project] ❯ text, [project] > text
    if line.contains("] \u{276F} ") || line.contains("] > ") {
        return true;
    }

    // Also matches bare prompt at start: project>
    if line.chars().take(30).collect::<String>().contains("> ")
        && !line.contains(':')
        && !line.contains("http")
    {
        // Looks like a prompt echo, not content
        if let Some(pos) = line.find("> ") {
            let before = &line[..pos];
            // If it's just a word before >, it's likely a prompt
            if !before.contains(' ') || before.starts_with('[') {
                return true;
            }
        }
    }

    // Spinner characters and thinking indicators
    if line
        .chars()
        .next()
        .map(|c| SPINNER_CHARS.contains(&c))
        .unwrap_or(false)
    {
        return true;
    }

    // Status bar box drawing characters
    if line.starts_with('\u{256E}')  // ╮
        || line.starts_with('\u{256D}')  // ╭
        || line.starts_with('\u{2502}')  // │
        || line.starts_with('\u{251C}')  // ├
        || line.starts_with('\u{2514}')  // └
        || line.starts_with('\u{250C}')  // ┌
        || line.starts_with('\u{2510}')  // ┐
        || line.starts_with('\u{2518}')  // ┘
        || line.starts_with('\u{2524}')  // ┤
        || line.starts_with('\u{252C}')  // ┬
        || line.starts_with('\u{2534}')  // ┴
        || line.starts_with('\u{253C}')  // ┼
        || line.starts_with('\u{2570}')  // ╰
    {
        return true;
    }

    // Claude Code branding and UI
    if line.contains("\u{2590}\u{259B}")  // ▐▛
        || line.contains("\u{259C}\u{258C}")  // ▜▌
        || line.contains("\u{259D}\u{259C}")  // ▝▜
        || line.contains("\u{259B}\u{2598}")  // ▛▘
    {
        return true;
    }

    // Thinking/processing indicators
    let lower = line.to_lowercase();
    if lower.contains("spelunking")
        || lower.contains("(thinking)")
        || lower.contains("thinking\u{2026}")  // thinking…
        || lower.contains("thinking...")
    {
        return true;
    }

    // Status messages that are UI noise
    if lower.contains("ctrl+b") || lower.contains("to run in background") {
        return true;
    }

    // Claude Code version/branding line
    if lower.contains("claude code v")
        || lower.contains("claude max")
        || lower.contains("opus 4")
        || lower.contains("sonnet")
    {
        return true;
    }

    // MCP tool invocation noise (keep the result, not the invocation)
    if line.contains("(MCP)(") && (line.contains("owner:") || line.contains("repo:")) {
        return true;
    }

    // Agent/task headers that are noise
    if line.ends_with("(MCP)") && !line.contains(':') {
        return true;
    }

    false
}

/// Check if Claude Code is ready for input (idle at prompt).
///
/// Detects several patterns indicating Claude Code has finished processing:
/// - Prompt character ❯ alone or at end of line
/// - Input box separator lines (───, ╭─, ╰─)
/// - "bypass permissions" hint
/// - Ready indicators like "│ ❯" or ">"
pub fn is_claude_ready(output: &str) -> bool {
    // Get the last few non-empty lines
    let lines: Vec<&str> = output
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(10)
        .collect();

    if lines.is_empty() {
        return false;
    }

    // Pattern 1: Line contains just the prompt character ❯
    // Claude Code shows "❯ " when ready for input
    for line in &lines[..lines.len().min(3)] {
        let trimmed = line.trim();
        if trimmed == "\u{276F}" || trimmed == "\u{276F} " {
            return true;
        }
        // Also check for prompt at end of line (after path)
        if trimmed.ends_with(" \u{276F}") || trimmed.ends_with(" \u{276F} ") {
            return true;
        }
    }

    // Pattern 2: The input box separator lines
    // Claude Code shows ──────────── above and below input
    let has_separator = lines.iter().take(5).any(|l| {
        let trimmed = l.trim();
        trimmed.starts_with("\u{2500}\u{2500}\u{2500}")  // ───
            || trimmed.starts_with("\u{256D}\u{2500}")   // ╭─
            || trimmed.starts_with("\u{2570}\u{2500}")   // ╰─
    });

    // Pattern 3: "bypass permissions" hint shown at prompt
    let has_bypass_hint = lines.iter().take(5).any(|l| l.contains("bypass permissions"));

    // Pattern 4: Empty prompt box (two separators with nothing between)
    if has_separator {
        // Check if we see the prompt structure
        for (i, line) in lines.iter().enumerate() {
            if line.contains("\u{276F}") && i < 5 {
                return true;
            }
        }
    }

    // Pattern 5: Check for common ready indicators
    let has_ready_indicator = lines.iter().take(3).any(|l| {
        let trimmed = l.trim();
        // Empty input prompt
        trimmed == "\u{2502} \u{276F}"  // │ ❯
            || trimmed.starts_with("\u{2502} \u{276F}")  // │ ❯
            // Just the chevron
            || trimmed == ">"
            || trimmed.ends_with("> ")
            // Explicit ready state
            || trimmed.contains("[ready]")
    });

    has_ready_indicator || has_bypass_hint
}

/// Find new lines in tmux output by comparing previous and current captures.
///
/// Returns lines that appear in `current` but not in `prev`, filtering out
/// UI noise. Useful for tracking incremental output from Claude Code.
pub fn find_new_lines(prev: &str, current: &str) -> Vec<String> {
    let prev_lines: HashSet<&str> = prev.lines().collect();
    let mut new_lines = Vec::new();

    for line in current.lines() {
        let trimmed = line.trim();
        if !prev_lines.contains(line) && !prev_lines.contains(trimmed) && !trimmed.is_empty() {
            // Filter out Claude Code UI noise
            if !is_ui_noise(trimmed) {
                new_lines.push(line.to_string());
            }
        }
    }

    new_lines
}

/// Clean raw response by removing UI artifacts.
///
/// Filters out common noise patterns when summarization is not available:
/// - Empty lines
/// - Continuation markers (⎿)
/// - Progress indicators (⏺)
/// - Hook/control messages
/// - MCP tool output
/// - Reading/Searching status
pub fn clean_response(raw: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        // Skip obvious noise
        if trimmed.is_empty()
            || trimmed.starts_with('\u{23BF}')  // ⎿
            || trimmed.starts_with('\u{23FA}')  // ⏺
            || trimmed.contains("hook")
            || trimmed.contains("ctrl+o")
            || trimmed.contains("(MCP)")
            || trimmed.starts_with("Reading")
            || trimmed.starts_with("Searched")
        {
            continue;
        }
        lines.push(trimmed);
    }
    lines.join("\n")
}

/// Clean screen output for preview display.
///
/// Returns the last few meaningful lines from output, suitable for
/// status displays or brief previews.
pub fn clean_screen_preview(output: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = output
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !is_ui_noise(trimmed)
        })
        .collect();

    // Take last N meaningful lines
    let start = if lines.len() > max_lines {
        lines.len() - max_lines
    } else {
        0
    };

    lines[start..]
        .iter()
        .map(|s| s.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ui_noise_prompt_lines() {
        assert!(is_ui_noise("[duetto] \u{276F} some command"));
        assert!(is_ui_noise("[project] > test input"));
        assert!(is_ui_noise("duetto> hello"));
        assert!(!is_ui_noise("This is actual content"));
        assert!(!is_ui_noise("Response: here is the answer"));
    }

    #[test]
    fn test_is_ui_noise_spinners() {
        assert!(is_ui_noise("\u{2733} Loading..."));
        assert!(is_ui_noise("\u{25CF} Working"));
    }

    #[test]
    fn test_is_ui_noise_box_drawing() {
        assert!(is_ui_noise("\u{256D}\u{2500}\u{2500} header"));
        assert!(is_ui_noise("\u{2502} content"));
        assert!(is_ui_noise("\u{2570}\u{2500}\u{2500} footer"));
    }

    #[test]
    fn test_is_ui_noise_branding() {
        assert!(is_ui_noise("Claude Code v1.0.0"));
        assert!(is_ui_noise("Using Opus 4.5"));
    }

    #[test]
    fn test_find_new_lines() {
        let prev = "line1\nline2\n";
        let current = "line1\nline2\nline3\n";
        let new = find_new_lines(prev, current);
        assert_eq!(new, vec!["line3"]);
    }

    #[test]
    fn test_find_new_lines_filters_prompt_echo() {
        let prev = "";
        let current = "[duetto] \u{276F} describe this project\nActual response here\n";
        let new = find_new_lines(prev, current);
        assert_eq!(new, vec!["Actual response here"]);
    }

    #[test]
    fn test_clean_response() {
        let raw = "\u{23FA} Working...\nActual content\nReading file.txt\n";
        let cleaned = clean_response(raw);
        assert_eq!(cleaned, "Actual content");
    }

    #[test]
    fn test_clean_screen_preview() {
        let output = "line1\nline2\nline3\nline4\nline5\nline6";
        let preview = clean_screen_preview(output, 3);
        assert_eq!(preview, "line4\nline5\nline6");
    }

    #[test]
    fn test_is_claude_ready_prompt() {
        assert!(is_claude_ready("\u{276F}"));
        assert!(is_claude_ready("path/to/dir \u{276F}"));
        assert!(!is_claude_ready("Still processing..."));
    }
}
