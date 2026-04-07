//! Option detection and parsing for interactive selection.
//!
//! Detects when Claude presents options in various formats:
//! - Letter-based: A), B), C) or A., B., C.
//! - Number-based: 1), 2), 3) or 1., 2., 3.
//! - Yes/no questions: (y/n), (yes/no)

use regex::Regex;
use std::sync::OnceLock;

/// Format of detected options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionFormat {
    /// Lettered options: A, B, C
    Letters,
    /// Numbered options: 1, 2, 3
    Numbers,
    /// Yes/no question
    YesNo,
}

/// A single parsed option.
#[derive(Debug, Clone)]
pub struct ParsedOption {
    /// Option key (e.g., "A", "1", "yes")
    pub key: String,
    /// Full option text
    pub label: String,
    /// Optional description (if option has additional context)
    pub description: Option<String>,
}

/// Detected options from Claude's output.
#[derive(Debug, Clone)]
pub struct DetectedOptions {
    /// Format of the options
    pub format: OptionFormat,
    /// List of parsed options
    pub options: Vec<ParsedOption>,
    /// Optional question text that precedes the options
    pub question: Option<String>,
}

/// Option pattern detector.
pub struct OptionDetector;

impl OptionDetector {
    /// Detect options in text output.
    ///
    /// Returns Some(DetectedOptions) if options are found, None otherwise.
    /// Only returns options when the text appears to be asking for a choice
    /// (contains a question or prompt), not when it's informational output
    /// that happens to contain a numbered/lettered list.
    pub fn detect_options(text: &str) -> Option<DetectedOptions> {
        // Try to detect different option formats in priority order
        if let Some(opts) = Self::detect_yes_no(text) {
            return Some(opts);
        }
        if let Some(opts) = Self::detect_letter_options(text) {
            // Only return if the preceding text looks like a question/prompt
            if Self::has_question_context(opts.question.as_deref()) {
                return Some(opts);
            }
        }
        if let Some(opts) = Self::detect_number_options(text) {
            // Only return if the preceding text looks like a question/prompt
            if Self::has_question_context(opts.question.as_deref()) {
                return Some(opts);
            }
        }
        None
    }

    /// Check if the question/context text preceding options looks like an
    /// actual prompt for selection, not an informational summary.
    fn has_question_context(question: Option<&str>) -> bool {
        let q = match question {
            Some(q) if !q.is_empty() => q,
            _ => return false, // No context → no buttons
        };
        let lower = q.to_lowercase();
        // Must contain a question mark or selection-related keywords
        lower.contains('?')
            || lower.contains("choose")
            || lower.contains("select")
            || lower.contains("pick")
            || lower.contains("prefer")
            || lower.contains("which")
            || lower.contains("would you")
            || lower.contains("do you want")
            || lower.contains("option")
            || lower.contains("approach")
    }

    /// Detect yes/no questions.
    fn detect_yes_no(text: &str) -> Option<DetectedOptions> {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        let pattern = PATTERN.get_or_init(|| {
            Regex::new(r"(?i)\((?:y/n|yes/no)\)\s*\??").unwrap()
        });

        if pattern.is_match(text) {
            // Extract the question text (everything before the (y/n))
            let question = pattern.split(text).next()
                .map(|q| q.trim().to_string())
                .filter(|q| !q.is_empty());

            return Some(DetectedOptions {
                format: OptionFormat::YesNo,
                options: vec![
                    ParsedOption {
                        key: "y".to_string(),
                        label: "Yes".to_string(),
                        description: None,
                    },
                    ParsedOption {
                        key: "n".to_string(),
                        label: "No".to_string(),
                        description: None,
                    },
                ],
                question,
            });
        }

        None
    }

    /// Detect letter-based options (A), B), C) or A., B., C.).
    fn detect_letter_options(text: &str) -> Option<DetectedOptions> {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        let pattern = PATTERN.get_or_init(|| {
            // Match patterns like "A) Option text" or "A. Option text"
            // Requires at least 2 different letters
            Regex::new(r"(?m)^([A-Za-z])[\)\.]\s*(.+?)$").unwrap()
        });

        let matches: Vec<_> = pattern.captures_iter(text).collect();

        // Need at least 2 options
        if matches.len() < 2 {
            return None;
        }

        // Check if letters are sequential (A, B, C or a, b, c)
        let letters: Vec<char> = matches.iter()
            .filter_map(|cap| cap.get(1))
            .map(|m| m.as_str().chars().next().unwrap().to_ascii_uppercase())
            .collect();

        if !Self::is_sequential_letters(&letters) {
            return None;
        }

        let options = matches
            .iter()
            .map(|cap| {
                let key = cap.get(1).unwrap().as_str().to_uppercase();
                let label = cap.get(2).unwrap().as_str().trim().to_string();
                ParsedOption {
                    key,
                    label,
                    description: None,
                }
            })
            .collect();

        // Try to extract question text (text before first option)
        let first_match_start = matches[0].get(0).unwrap().start();
        let question = if first_match_start > 0 {
            let q = text[..first_match_start].trim();
            if !q.is_empty() {
                Some(q.to_string())
            } else {
                None
            }
        } else {
            None
        };

        Some(DetectedOptions {
            format: OptionFormat::Letters,
            options,
            question,
        })
    }

    /// Detect number-based options (1), 2), 3) or 1., 2., 3.).
    fn detect_number_options(text: &str) -> Option<DetectedOptions> {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        let pattern = PATTERN.get_or_init(|| {
            // Match patterns like "1) Option text" or "1. Option text"
            Regex::new(r"(?m)^(\d+)[\)\.]\s*(.+?)$").unwrap()
        });

        let matches: Vec<_> = pattern.captures_iter(text).collect();

        // Need at least 2 options
        if matches.len() < 2 {
            return None;
        }

        // Check if numbers are sequential (1, 2, 3)
        let numbers: Vec<usize> = matches.iter()
            .filter_map(|cap| cap.get(1))
            .filter_map(|m| m.as_str().parse().ok())
            .collect();

        if !Self::is_sequential_numbers(&numbers) {
            return None;
        }

        let options = matches
            .iter()
            .map(|cap| {
                let key = cap.get(1).unwrap().as_str().to_string();
                let label = cap.get(2).unwrap().as_str().trim().to_string();
                ParsedOption {
                    key,
                    label,
                    description: None,
                }
            })
            .collect();

        // Try to extract question text (text before first option)
        let first_match_start = matches[0].get(0).unwrap().start();
        let question = if first_match_start > 0 {
            let q = text[..first_match_start].trim();
            if !q.is_empty() {
                Some(q.to_string())
            } else {
                None
            }
        } else {
            None
        };

        Some(DetectedOptions {
            format: OptionFormat::Numbers,
            options,
            question,
        })
    }

    /// Check if letters are sequential (A, B, C).
    fn is_sequential_letters(letters: &[char]) -> bool {
        if letters.len() < 2 {
            return false;
        }
        for i in 1..letters.len() {
            if letters[i] as u32 != letters[i - 1] as u32 + 1 {
                return false;
            }
        }
        true
    }

    /// Check if numbers are sequential (1, 2, 3).
    fn is_sequential_numbers(numbers: &[usize]) -> bool {
        if numbers.len() < 2 {
            return false;
        }
        for i in 1..numbers.len() {
            if numbers[i] != numbers[i - 1] + 1 {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_letter_options() {
        let text = "Which would you prefer?\nA) Option one\nB) Option two\nC) Option three";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_some());

        let opts = detected.unwrap();
        assert_eq!(opts.format, OptionFormat::Letters);
        assert_eq!(opts.options.len(), 3);
        assert_eq!(opts.options[0].key, "A");
        assert_eq!(opts.options[0].label, "Option one");
        assert_eq!(opts.options[1].key, "B");
        assert_eq!(opts.options[2].key, "C");
        assert!(opts.question.is_some());
        assert!(opts.question.unwrap().contains("Which would you prefer"));
    }

    #[test]
    fn test_detect_letter_options_with_period() {
        // Has question context via "which approach?"
        let text = "Which approach?\nA. First option\nB. Second option";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_some());

        let opts = detected.unwrap();
        assert_eq!(opts.format, OptionFormat::Letters);
        assert_eq!(opts.options.len(), 2);
    }

    #[test]
    fn test_no_buttons_on_informational_list() {
        // Informational output — no question, no buttons
        let text = "Done! Here's what I completed:\n1. Fixed the auth bug\n2. Added unit tests\n3. Updated the docs";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_none(), "Informational lists should not produce buttons");
    }

    #[test]
    fn test_no_buttons_without_question_context() {
        // Sequential numbers but no question/prompt context
        let text = "A. Refactored the module\nB. Created new tests";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_none(), "Lists without question context should not produce buttons");
    }

    #[test]
    fn test_detect_number_options() {
        let text = "Select an option:\n1) First option\n2) Second option\n3) Third option";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_some());

        let opts = detected.unwrap();
        assert_eq!(opts.format, OptionFormat::Numbers);
        assert_eq!(opts.options.len(), 3);
        assert_eq!(opts.options[0].key, "1");
        assert_eq!(opts.options[0].label, "First option");
        assert_eq!(opts.options[1].key, "2");
        assert_eq!(opts.options[2].key, "3");
    }

    #[test]
    fn test_detect_number_options_with_period() {
        // Has question context via "Select"
        let text = "Select an option:\n1. First option\n2. Second option";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_some());

        let opts = detected.unwrap();
        assert_eq!(opts.format, OptionFormat::Numbers);
        assert_eq!(opts.options.len(), 2);
    }

    #[test]
    fn test_detect_yes_no() {
        let text = "Would you like to continue? (y/n)";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_some());

        let opts = detected.unwrap();
        assert_eq!(opts.format, OptionFormat::YesNo);
        assert_eq!(opts.options.len(), 2);
        assert_eq!(opts.options[0].key, "y");
        assert_eq!(opts.options[1].key, "n");
        assert!(opts.question.is_some());
    }

    #[test]
    fn test_detect_yes_no_uppercase() {
        let text = "Proceed with installation? (Y/N)";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_some());

        let opts = detected.unwrap();
        assert_eq!(opts.format, OptionFormat::YesNo);
    }

    #[test]
    fn test_detect_yes_no_full() {
        let text = "Do you want to save changes? (yes/no)";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_some());

        let opts = detected.unwrap();
        assert_eq!(opts.format, OptionFormat::YesNo);
    }

    #[test]
    fn test_no_options() {
        let text = "This is just regular text without any options.";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_none());
    }

    #[test]
    fn test_non_sequential_letters() {
        let text = "A) First\nC) Third\nE) Fifth";
        let detected = OptionDetector::detect_options(text);
        // Should not detect non-sequential letters
        assert!(detected.is_none());
    }

    #[test]
    fn test_non_sequential_numbers() {
        let text = "1) First\n3) Third\n5) Fifth";
        let detected = OptionDetector::detect_options(text);
        // Should not detect non-sequential numbers
        assert!(detected.is_none());
    }

    #[test]
    fn test_single_option_not_detected() {
        let text = "A) Only one option";
        let detected = OptionDetector::detect_options(text);
        // Need at least 2 options
        assert!(detected.is_none());
    }

    #[test]
    fn test_mixed_content() {
        let text = "Here are your options:\nA) Refactor the module\nB) Create new module\n\nBoth are valid approaches.";
        let detected = OptionDetector::detect_options(text);
        assert!(detected.is_some());

        let opts = detected.unwrap();
        assert_eq!(opts.format, OptionFormat::Letters);
        assert_eq!(opts.options.len(), 2);
    }
}
