//! Structured fact extraction from Claude Code terminal output.
//!
//! Extracts structured facts (file edits, test results, git ops, errors, etc.)
//! from raw terminal output and produces template-based summaries with
//! confidence scoring. Designed to handle ~75% of summaries instantly without
//! LLM calls, with the remaining 25% falling through with pre-digested context.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

// ---------------------------------------------------------------------------
// Compiled regex patterns (LazyLock for zero-cost lazy init)
// ---------------------------------------------------------------------------

static RE_FILE_EDIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(created|modified|wrote|updated|edited)\s+(.+\.\w+)").expect("RE_FILE_EDIT")
});

static RE_FILE_READ: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(reading|read|searched|found in)\s+(.+)").expect("RE_FILE_READ")
});

static RE_TEST_PASSED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+)\s+(?:tests?\s+)?passed").expect("RE_TEST_PASSED")
});

static RE_TEST_FAILED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+)\s+(?:tests?\s+)?failed").expect("RE_TEST_FAILED")
});

static RE_TEST_SKIPPED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+)\s+(?:tests?\s+)?(?:skipped|ignored)").expect("RE_TEST_SKIPPED")
});

static RE_CARGO_TEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"test result:\s*\w+\.\s*(\d+)\s*passed;\s*(\d+)\s*failed").expect("RE_CARGO_TEST")
});

static RE_PYTEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+)\s+passed(?:,\s*(\d+)\s+failed)?").expect("RE_PYTEST")
});

static RE_NPM_TEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Tests:\s*(\d+)\s+passed(?:,\s*(\d+)\s+failed)?").expect("RE_NPM_TEST")
});

static RE_GIT_OP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(committed|pushed|merged|rebased|cherry-picked|stashed)")
        .expect("RE_GIT_OP")
});

static RE_BUILD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(compiling|building|finished|compiled|built|linking)").expect("RE_BUILD")
});

static RE_ERROR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^error(\[E\d+\])?:|panic!|failed:").expect("RE_ERROR")
});

static RE_TOOL_SPINNER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]?\s*\((Bash|Read|Edit|Write|Grep|Glob|Agent)\)")
        .expect("RE_TOOL_SPINNER")
});

static RE_TOOL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"── (Bash|Read|Edit|Write|Grep|Glob)").expect("RE_TOOL_HEADER")
});

static RE_SUCCESS_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"✓|✔").expect("RE_SUCCESS_MARKER"));

static RE_FAILURE_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"✗|✘").expect("RE_FAILURE_MARKER"));

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Structured facts extracted from Claude Code terminal output.
#[derive(Debug, Clone, Default)]
pub struct StructuredSummary {
    /// Files created/modified/written.
    pub files_edited: Vec<String>,
    /// Files read/searched.
    pub files_read: Vec<String>,
    /// Test pass/fail counts (if detected).
    pub tests: Option<TestResult>,
    /// Git operations performed.
    pub git_ops: Vec<String>,
    /// Error messages.
    pub errors: Vec<String>,
    /// Tool names used (Bash, Read, Edit, Write, Grep, Glob, Agent).
    pub tools_used: HashSet<String>,
    /// Build result (if detected).
    pub build_status: Option<String>,
    /// Important non-noise lines that don't fit other categories.
    pub key_lines: Vec<String>,
    /// Total input lines processed.
    pub total_lines: usize,
}

/// Aggregated test results.
#[derive(Debug, Clone, Default)]
pub struct TestResult {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Extract structured facts from terminal output lines.
///
/// Scans each line against compiled regex patterns and populates a
/// [`StructuredSummary`]. Lines that match no pattern are collected as
/// `key_lines` (up to a reasonable cap) for downstream use.
pub fn extract(lines: &[String]) -> StructuredSummary {
    let mut summary = StructuredSummary {
        total_lines: lines.len(),
        ..Default::default()
    };

    let mut seen_files_edited: HashSet<String> = HashSet::new();
    let mut seen_files_read: HashSet<String> = HashSet::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut classified = false;

        // --- File edits ---
        if let Some(caps) = RE_FILE_EDIT.captures(trimmed) {
            if let Some(path) = caps.get(2) {
                let p = path.as_str().trim().to_string();
                if seen_files_edited.insert(p.clone()) {
                    summary.files_edited.push(p);
                }
            }
            classified = true;
        }

        // --- File reads ---
        if let Some(caps) = RE_FILE_READ.captures(trimmed) {
            if let Some(path) = caps.get(2) {
                let p = path.as_str().trim().to_string();
                if seen_files_read.insert(p.clone()) {
                    summary.files_read.push(p);
                }
            }
            classified = true;
        }

        // --- Test results (cargo, pytest, npm, generic) ---
        if let Some(caps) = RE_CARGO_TEST.captures(trimmed) {
            let passed: u32 = caps[1].parse().unwrap_or(0);
            let failed: u32 = caps[2].parse().unwrap_or(0);
            let tr = summary.tests.get_or_insert(TestResult::default());
            tr.passed = tr.passed.max(passed);
            tr.failed = tr.failed.max(failed);
            classified = true;
        } else if let Some(caps) = RE_NPM_TEST.captures(trimmed) {
            let passed: u32 = caps[1].parse().unwrap_or(0);
            let failed: u32 = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let tr = summary.tests.get_or_insert(TestResult::default());
            tr.passed = tr.passed.max(passed);
            tr.failed = tr.failed.max(failed);
            classified = true;
        } else if let Some(caps) = RE_PYTEST.captures(trimmed) {
            let passed: u32 = caps[1].parse().unwrap_or(0);
            let failed: u32 = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let tr = summary.tests.get_or_insert(TestResult::default());
            tr.passed = tr.passed.max(passed);
            tr.failed = tr.failed.max(failed);
            classified = true;
        } else {
            // Generic test patterns
            if let Some(caps) = RE_TEST_PASSED.captures(trimmed) {
                let n: u32 = caps[1].parse().unwrap_or(0);
                let tr = summary.tests.get_or_insert(TestResult::default());
                tr.passed = tr.passed.max(n);
                classified = true;
            }
            if let Some(caps) = RE_TEST_FAILED.captures(trimmed) {
                let n: u32 = caps[1].parse().unwrap_or(0);
                let tr = summary.tests.get_or_insert(TestResult::default());
                tr.failed = tr.failed.max(n);
                classified = true;
            }
            if let Some(caps) = RE_TEST_SKIPPED.captures(trimmed) {
                let n: u32 = caps[1].parse().unwrap_or(0);
                let tr = summary.tests.get_or_insert(TestResult::default());
                tr.skipped = tr.skipped.max(n);
                classified = true;
            }
        }

        // --- Git operations ---
        if let Some(caps) = RE_GIT_OP.captures(trimmed) {
            summary.git_ops.push(caps[1].to_lowercase());
            classified = true;
        }

        // --- Errors ---
        if RE_ERROR.is_match(trimmed) {
            // Keep the first 200 chars of the error line
            let err_text = if trimmed.len() > 200 {
                format!("{}...", &trimmed[..200])
            } else {
                trimmed.to_string()
            };
            summary.errors.push(err_text);
            classified = true;
        }

        // --- Build status ---
        if let Some(caps) = RE_BUILD.captures(trimmed) {
            summary.build_status = Some(caps[1].to_lowercase());
            classified = true;
        }

        // --- Tool usage ---
        if let Some(caps) = RE_TOOL_SPINNER.captures(trimmed) {
            summary.tools_used.insert(caps[1].to_string());
            classified = true;
        }
        if let Some(caps) = RE_TOOL_HEADER.captures(trimmed) {
            summary.tools_used.insert(caps[1].to_string());
            classified = true;
        }

        // --- Success/failure markers ---
        if RE_SUCCESS_MARKER.is_match(trimmed) || RE_FAILURE_MARKER.is_match(trimmed) {
            classified = true;
            // These are informative but captured by other patterns too
        }

        // --- Unclassified but potentially interesting lines ---
        if !classified && summary.key_lines.len() < 20 {
            // Skip very short lines and obvious noise
            if trimmed.len() > 3
                && !crate::output_filter::is_ui_noise(trimmed)
            {
                summary.key_lines.push(trimmed.to_string());
            }
        }
    }

    summary
}

// ---------------------------------------------------------------------------
// Summary generation
// ---------------------------------------------------------------------------

impl StructuredSummary {
    /// Produce a natural language summary from extracted facts.
    ///
    /// Ordered by importance:
    /// 1. Errors (if any)
    /// 2. Test results
    /// 3. Files edited (count, first 3 listed)
    /// 4. Git operations
    /// 5. Build status
    /// 6. Files read/searched (count only)
    /// 7. Tools used (if nothing else to report)
    ///
    /// Keeps output concise: 2-4 sentences max.
    pub fn to_summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        // 1. Errors
        if !self.errors.is_empty() {
            if self.errors.len() == 1 {
                parts.push(format!("Error: {}", self.errors[0]));
            } else {
                parts.push(format!(
                    "{} errors found. First: {}",
                    self.errors.len(),
                    self.errors[0]
                ));
            }
        }

        // 2. Test results
        if let Some(ref tr) = self.tests {
            let mut test_parts = Vec::new();
            if tr.passed > 0 {
                test_parts.push(format!("{} passed", tr.passed));
            }
            if tr.failed > 0 {
                test_parts.push(format!("{} failed", tr.failed));
            }
            if tr.skipped > 0 {
                test_parts.push(format!("{} skipped", tr.skipped));
            }
            if !test_parts.is_empty() {
                parts.push(format!("Tests: {}.", test_parts.join(", ")));
            }
        }

        // 3. Files edited
        if !self.files_edited.is_empty() {
            let count = self.files_edited.len();
            let listed: Vec<&str> = self.files_edited.iter().take(3).map(|s| s.as_str()).collect();
            let list_str = listed.join(", ");
            if count <= 3 {
                parts.push(format!("Edited {}.", list_str));
            } else {
                parts.push(format!(
                    "Edited {} files ({} and {} more).",
                    count,
                    list_str,
                    count - 3
                ));
            }
        }

        // 4. Git operations
        if !self.git_ops.is_empty() {
            let unique: Vec<&str> = {
                let mut seen = HashSet::new();
                self.git_ops
                    .iter()
                    .filter(|op| seen.insert(op.as_str()))
                    .map(|s| s.as_str())
                    .collect()
            };
            parts.push(format!("Git: {}.", unique.join(", ")));
        }

        // 5. Build status
        if let Some(ref status) = self.build_status {
            parts.push(format!("Build: {}.", status));
        }

        // 6. Files read
        if !self.files_read.is_empty() {
            parts.push(format!("Read/searched {} files.", self.files_read.len()));
        }

        // 7. Fallback: tools used
        if parts.is_empty() && !self.tools_used.is_empty() {
            let mut tools: Vec<&str> = self.tools_used.iter().map(|s| s.as_str()).collect();
            tools.sort();
            parts.push(format!("Used tools: {}.", tools.join(", ")));
        }

        // 8. Ultimate fallback: key lines
        if parts.is_empty() && !self.key_lines.is_empty() {
            let joined = self.key_lines.iter().take(3).cloned().collect::<Vec<_>>().join(" ");
            return joined;
        }

        if parts.is_empty() {
            return "No significant output detected.".to_string();
        }

        parts.join(" ")
    }

    /// Confidence that [`to_summary()`] is sufficient without LLM assistance.
    ///
    /// Returns a value in `[0.0, 1.0]`:
    /// - `0.9+` : Clear structured events (file edits, test results, errors)
    /// - `0.7`  : Short output with few key_lines (< 5 lines)
    /// - `0.5`  : Moderate key_lines but no structured events
    /// - `0.3`  : Many unclassified key_lines, complex output
    pub fn confidence(&self) -> f32 {
        let mut score: f32 = 0.0;

        let has_structured = !self.files_edited.is_empty()
            || self.tests.is_some()
            || !self.git_ops.is_empty()
            || !self.errors.is_empty()
            || self.build_status.is_some();

        // Structured fields populated
        if has_structured {
            score += 0.3;
        }

        // Classification ratio: most lines were classified
        if self.total_lines > 0 {
            let key_ratio = self.key_lines.len() as f32 / self.total_lines as f32;
            if key_ratio < 0.3 {
                score += 0.2;
            }
        }

        // Errors are always important and clear to surface
        if !self.errors.is_empty() {
            score += 0.1;
        }

        // Short output is inherently easy to summarize (but not empty)
        if self.total_lines > 0 && self.total_lines < 10 {
            score += 0.2;
        }

        // Test results are very structured
        if self.tests.is_some() {
            score += 0.1;
        }

        // Cap at 1.0
        score.min(1.0)
    }

    /// Produce a compact string of structured facts for LLM context.
    ///
    /// Used when [`confidence()`] is low and we fall through to an LLM call.
    /// Pre-digesting the output reduces the amount of raw text the LLM must
    /// process, lowering cost and latency.
    pub fn to_context(&self) -> String {
        let mut facts: Vec<String> = Vec::new();

        if !self.files_edited.is_empty() {
            let listed: Vec<&str> = self.files_edited.iter().take(5).map(|s| s.as_str()).collect();
            facts.push(format!("Edited {} files ({})", self.files_edited.len(), listed.join(", ")));
        }

        if !self.files_read.is_empty() {
            facts.push(format!("Read {} files", self.files_read.len()));
        }

        if let Some(ref tr) = self.tests {
            facts.push(format!(
                "Tests: {} passed, {} failed, {} skipped",
                tr.passed, tr.failed, tr.skipped
            ));
        }

        if !self.git_ops.is_empty() {
            facts.push(format!("Git ops: {}", self.git_ops.join(", ")));
        }

        if !self.errors.is_empty() {
            facts.push(format!("{} errors", self.errors.len()));
        }

        if let Some(ref status) = self.build_status {
            facts.push(format!("Build: {}", status));
        }

        if !self.tools_used.is_empty() {
            let mut tools: Vec<&str> = self.tools_used.iter().map(|s| s.as_str()).collect();
            tools.sort();
            facts.push(format!("Tools: {}", tools.join(", ")));
        }

        if facts.is_empty() {
            return String::new();
        }

        format!("Structured facts: {}.", facts.join(". "))
    }

    /// Returns `true` if any structured fields are populated.
    pub fn has_structured_data(&self) -> bool {
        !self.files_edited.is_empty()
            || !self.files_read.is_empty()
            || self.tests.is_some()
            || !self.git_ops.is_empty()
            || !self.errors.is_empty()
            || self.build_status.is_some()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(|l| l.to_string()).collect()
    }

    #[test]
    fn test_extract_file_edits() {
        let input = lines(
            "Created src/main.rs\n\
             Modified lib.rs\n\
             Wrote config.toml\n\
             Some irrelevant line",
        );
        let s = extract(&input);

        assert_eq!(s.files_edited.len(), 3);
        assert!(s.files_edited.contains(&"src/main.rs".to_string()));
        assert!(s.files_edited.contains(&"lib.rs".to_string()));
        assert!(s.files_edited.contains(&"config.toml".to_string()));
        assert!(s.confidence() >= 0.3, "confidence should be at least 0.3 with file edits");
    }

    #[test]
    fn test_extract_test_results_cargo() {
        let input = lines(
            "running 42 tests\n\
             test foo::bar ... ok\n\
             test result: ok. 40 passed; 2 failed; 0 ignored",
        );
        let s = extract(&input);

        let tr = s.tests.as_ref().expect("should have test results");
        assert_eq!(tr.passed, 40);
        assert_eq!(tr.failed, 2);
        assert!(s.confidence() >= 0.5);
    }

    #[test]
    fn test_extract_test_results_pytest() {
        let input = lines("========== 15 passed, 3 failed in 2.5s ==========");
        let s = extract(&input);

        let tr = s.tests.as_ref().expect("should have test results");
        assert_eq!(tr.passed, 15);
        assert_eq!(tr.failed, 3);
    }

    #[test]
    fn test_extract_test_results_npm() {
        let input = lines("Tests: 8 passed, 1 failed");
        let s = extract(&input);

        let tr = s.tests.as_ref().expect("should have test results");
        assert_eq!(tr.passed, 8);
        assert_eq!(tr.failed, 1);
    }

    #[test]
    fn test_extract_git_operations() {
        let input = lines(
            "Successfully committed changes\n\
             Pushed to origin/main\n\
             Merged branch feature/auth",
        );
        let s = extract(&input);

        assert!(s.git_ops.contains(&"committed".to_string()));
        assert!(s.git_ops.contains(&"pushed".to_string()));
        assert!(s.git_ops.contains(&"merged".to_string()));
        assert!(s.confidence() >= 0.3);
    }

    #[test]
    fn test_extract_errors() {
        let input = lines(
            "error[E0308]: mismatched types\n\
             --> src/main.rs:10:5\n\
             error: could not compile `my_crate`",
        );
        let s = extract(&input);

        assert_eq!(s.errors.len(), 2);
        assert!(s.errors[0].contains("mismatched types"));
        assert!(s.confidence() >= 0.4, "errors should boost confidence");
    }

    #[test]
    fn test_extract_tool_usage() {
        let input = lines(
            "⠋ (Bash) running cargo test\n\
             ── Read src/lib.rs\n\
             ── Edit src/main.rs\n\
             (Grep) searching for pattern",
        );
        let s = extract(&input);

        assert!(s.tools_used.contains("Bash"));
        assert!(s.tools_used.contains("Read"));
        assert!(s.tools_used.contains("Edit"));
        assert!(s.tools_used.contains("Grep"));
    }

    #[test]
    fn test_extract_build_status() {
        let input = lines(
            "Compiling commander-core v0.1.0\n\
             Compiling commander-gui v0.1.0\n\
             Finished release target(s) in 30.2s",
        );
        let s = extract(&input);

        assert!(s.build_status.is_some());
    }

    #[test]
    fn test_extract_mixed_output() {
        let input = lines(
            "── Edit src/lib.rs\n\
             Modified src/lib.rs\n\
             ── Bash\n\
             running 10 tests\n\
             test result: ok. 10 passed; 0 failed; 0 ignored\n\
             Successfully committed changes\n\
             Pushed to origin/main",
        );
        let s = extract(&input);

        assert!(!s.files_edited.is_empty());
        assert!(s.tests.is_some());
        assert!(!s.git_ops.is_empty());
        assert!(s.tools_used.contains("Edit"));
        assert!(s.tools_used.contains("Bash"));
        // Mixed structured output should have high confidence
        assert!(
            s.confidence() >= 0.5,
            "mixed structured output should have confidence >= 0.5, got {}",
            s.confidence()
        );
    }

    #[test]
    fn test_pure_explanatory_text_low_confidence() {
        let input = lines(
            "I've analyzed the codebase and here's what I found.\n\
             The architecture follows a layered pattern with clear separation of concerns.\n\
             The data layer communicates with PostgreSQL through sqlx.\n\
             The service layer handles business logic and validation.\n\
             The API layer uses axum for HTTP routing.\n\
             There are some areas that could be improved.\n\
             First, the error handling could be more consistent.\n\
             Second, some modules are tightly coupled.\n\
             Third, test coverage is low in the service layer.\n\
             I recommend addressing these issues in order of priority.",
        );
        let s = extract(&input);

        // Should have no structured data, only key_lines
        assert!(s.files_edited.is_empty());
        assert!(s.tests.is_none());
        assert!(s.git_ops.is_empty());
        assert!(s.errors.is_empty());
        assert!(!s.key_lines.is_empty());
        // Low confidence because nothing was classified structurally
        assert!(
            s.confidence() < 0.5,
            "pure explanatory text should have low confidence, got {}",
            s.confidence()
        );
    }

    #[test]
    fn test_short_simple_output() {
        let input = lines("Created src/new_module.rs");
        let s = extract(&input);

        assert_eq!(s.files_edited.len(), 1);
        assert_eq!(s.total_lines, 1);
        // Short + structured = high confidence
        assert!(
            s.confidence() >= 0.7,
            "short structured output should have high confidence, got {}",
            s.confidence()
        );
    }

    #[test]
    fn test_to_summary_errors_first() {
        let input = lines(
            "error: could not compile\n\
             Modified src/main.rs",
        );
        let s = extract(&input);
        let summary = s.to_summary();

        assert!(
            summary.starts_with("Error:"),
            "summary should lead with errors: '{}'",
            summary
        );
    }

    #[test]
    fn test_to_summary_concise() {
        let input = lines(
            "Modified src/lib.rs\n\
             Modified src/main.rs\n\
             Modified src/config.rs\n\
             Modified src/util.rs\n\
             Modified src/handler.rs\n\
             test result: ok. 42 passed; 0 failed; 0 ignored",
        );
        let s = extract(&input);
        let summary = s.to_summary();

        // Should mention test results and file count
        assert!(summary.contains("42 passed"), "summary: '{}'", summary);
        assert!(summary.contains("5 files"), "summary: '{}'", summary);
    }

    #[test]
    fn test_to_context_for_llm() {
        let input = lines(
            "Modified src/lib.rs\n\
             Modified src/main.rs\n\
             test result: ok. 10 passed; 0 failed; 0 ignored\n\
             Successfully committed changes",
        );
        let s = extract(&input);
        let ctx = s.to_context();

        assert!(ctx.starts_with("Structured facts:"));
        assert!(ctx.contains("Edited"));
        assert!(ctx.contains("Tests:"));
        assert!(ctx.contains("Git ops:"));
    }

    #[test]
    fn test_empty_input() {
        let s = extract(&[]);
        assert_eq!(s.total_lines, 0);
        assert_eq!(s.confidence(), 0.0);
        assert_eq!(s.to_summary(), "No significant output detected.");
        assert!(s.to_context().is_empty());
    }

    #[test]
    fn test_dedup_file_edits() {
        let input = lines(
            "Modified src/lib.rs\n\
             Updated src/lib.rs\n\
             Wrote src/lib.rs",
        );
        let s = extract(&input);

        // The same file path should not appear multiple times
        assert_eq!(s.files_edited.len(), 1);
    }

    #[test]
    fn test_has_structured_data() {
        let empty = extract(&[]);
        assert!(!empty.has_structured_data());

        let with_edit = extract(&lines("Modified src/lib.rs"));
        assert!(with_edit.has_structured_data());
    }

    #[test]
    fn test_summary_tools_fallback() {
        let input = lines(
            "── Bash\n\
             ── Read",
        );
        let s = extract(&input);
        let summary = s.to_summary();

        assert!(
            summary.contains("Used tools:"),
            "should fall back to tool listing: '{}'",
            summary
        );
    }
}
