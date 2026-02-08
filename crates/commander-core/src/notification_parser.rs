//! Notification parser for extracting structured data from timer notifications.
//!
//! Parses notifications like:
//! ```text
//! [timer] 1 new session(s) waiting for input:
//!    @izzie-33 - masa@Masas-Studio:/Users/masa/Projects/izzie2 (main*?) [model|Claude MPM|70%]
//! ```
//!
//! Into structured `ParsedSessionStatus` with session name, path, branch, model, and context usage.

use regex::Regex;
use std::sync::LazyLock;

/// Regex to strip ANSI escape codes from strings.
static ANSI_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1B\[[0-9;]*[a-zA-Z]").expect("Invalid ANSI regex"));

/// Regex to extract session name from @mention (requires whitespace or start-of-line before @).
static SESSION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)@([a-zA-Z0-9_-]+)").expect("Invalid session regex"));

/// Regex to extract user@host:path.
static PATH_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([^@\s]+)@([^:]+):([^\s(]+)").expect("Invalid path regex"));

/// Regex to extract branch and git status from (branch*?) pattern.
/// Requires at least 2 characters to avoid matching `(s)` from "session(s)".
static BRANCH_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(([a-zA-Z0-9_/.-]{2,})([*?!+-]*)\)").expect("Invalid branch regex"));

/// Regex to extract model info [model|framework|usage%].
static MODEL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^|\]]+)\|([^|\]]+)\|([0-9]+)%\]").expect("Invalid model regex"));

/// Parsed session status extracted from a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSessionStatus {
    /// Session name without @ prefix (e.g., "izzie-33").
    pub name: String,
    /// Working directory path (e.g., "/Users/masa/Projects/izzie2").
    pub path: Option<String>,
    /// Git branch name (e.g., "main").
    pub branch: Option<String>,
    /// Git status flags (e.g., "*?" for modified + untracked).
    pub git_status: Option<String>,
    /// Model identifier (e.g., "us.anthropic.claude-opus-4-5-20251101-v1:0").
    pub model: Option<String>,
    /// Framework name (e.g., "Claude MPM").
    pub framework: Option<String>,
    /// Context window usage percentage (0-100).
    pub context_usage: Option<u8>,
}

impl ParsedSessionStatus {
    /// Convert to human-readable conversational text.
    ///
    /// Transforms raw session data into natural language descriptions.
    ///
    /// # Example
    /// ```
    /// use commander_core::notification_parser::ParsedSessionStatus;
    ///
    /// let status = ParsedSessionStatus {
    ///     name: "izzie-33".to_string(),
    ///     path: Some("/Users/masa/Projects/izzie2".to_string()),
    ///     branch: Some("main".to_string()),
    ///     git_status: Some("*?".to_string()),
    ///     model: Some("claude-opus-4-5".to_string()),
    ///     framework: Some("Claude MPM".to_string()),
    ///     context_usage: Some(68),
    /// };
    ///
    /// let text = status.to_conversational();
    /// assert!(text.contains("izzie-33"));
    /// assert!(text.contains("izzie2"));
    /// ```
    pub fn to_conversational(&self) -> String {
        let mut parts = Vec::new();

        // Session name
        parts.push(format!("\"{}\"", self.name));

        // Project from path
        if let Some(ref path) = self.path {
            let project = path.rsplit('/').next().unwrap_or(path);
            parts.push(format!("project {}", project));
        }

        // Branch with git status
        if let Some(ref branch) = self.branch {
            let status_desc = self.git_status.as_ref().map_or(String::new(), |s| {
                describe_git_status(s)
            });
            if status_desc.is_empty() {
                parts.push(format!("branch {}", branch));
            } else {
                parts.push(format!("branch {} ({})", branch, status_desc));
            }
        }

        // Context usage
        if let Some(usage) = self.context_usage {
            let context_desc = if usage >= 90 {
                format!("{}% context (critical)", usage)
            } else if usage >= 70 {
                format!("{}% context (getting full)", usage)
            } else {
                format!("{}% context", usage)
            };
            parts.push(context_desc);
        }

        // Model (simplified)
        if let Some(ref model) = self.model {
            let model_short = simplify_model_name(model);
            if !model_short.is_empty() {
                parts.push(model_short);
            }
        }

        if parts.len() == 1 {
            format!("Session {}", parts[0])
        } else {
            format!("Session {}: {}", parts[0], parts[1..].join(", "))
        }
    }

    /// Convert to a brief status line for notifications.
    ///
    /// Returns project name, branch info, and context usage (not session name).
    ///
    /// # Example
    /// ```
    /// use commander_core::notification_parser::ParsedSessionStatus;
    ///
    /// let status = ParsedSessionStatus {
    ///     name: "izzie-33".to_string(),
    ///     path: Some("/Users/masa/Projects/izzie2".to_string()),
    ///     branch: Some("main".to_string()),
    ///     git_status: Some("*".to_string()),
    ///     model: None,
    ///     framework: None,
    ///     context_usage: Some(68),
    /// };
    ///
    /// let brief = status.to_brief();
    /// assert!(brief.contains("izzie2"));  // Project name from path
    /// assert!(brief.contains("main"));    // Branch name
    /// ```
    pub fn to_brief(&self) -> String {
        let project = self.path.as_ref()
            .map(|p| p.rsplit('/').next().unwrap_or(p))
            .unwrap_or(&self.name);

        let branch_info = self.branch.as_ref().map_or(String::new(), |b| {
            let status = self.git_status.as_ref()
                .map(|s| if s.contains('*') || s.contains('?') { " with changes" } else { "" })
                .unwrap_or("");
            format!(" on {}{}", b, status)
        });

        let context = self.context_usage
            .map(|u| format!(" ({}% ctx)", u))
            .unwrap_or_default();

        format!("{}{}{}", project, branch_info, context)
    }
}

/// Describe git status flags in human-readable terms.
fn describe_git_status(flags: &str) -> String {
    let mut descriptions = Vec::new();

    if flags.contains('*') {
        descriptions.push("modified");
    }
    if flags.contains('?') {
        descriptions.push("untracked files");
    }
    if flags.contains('+') {
        descriptions.push("staged");
    }
    if flags.contains('-') {
        descriptions.push("deleted");
    }
    if flags.contains('!') {
        descriptions.push("ignored");
    }

    descriptions.join(", ")
}

/// Simplify long model names to human-friendly versions.
fn simplify_model_name(model: &str) -> String {
    let lower = model.to_lowercase();

    if lower.contains("opus") {
        "Claude Opus".to_string()
    } else if lower.contains("sonnet") {
        "Claude Sonnet".to_string()
    } else if lower.contains("haiku") {
        "Claude Haiku".to_string()
    } else if lower.contains("claude") {
        "Claude".to_string()
    } else if lower.contains("gpt-4") {
        "GPT-4".to_string()
    } else if lower.contains("gpt-3") {
        "GPT-3.5".to_string()
    } else {
        // Return empty for unknown models to avoid clutter
        String::new()
    }
}

/// Strip ANSI escape codes from a string.
///
/// # Example
/// ```
/// use commander_core::notification_parser::strip_ansi;
///
/// let input = "text \x1B[90mgrayed\x1B[0m normal";
/// assert_eq!(strip_ansi(input), "text grayed normal");
/// ```
pub fn strip_ansi(s: &str) -> String {
    ANSI_REGEX.replace_all(s, "").to_string()
}

/// Parse a notification string and extract session status information.
///
/// Returns `Some(ParsedSessionStatus)` if the notification contains a valid
/// session mention (@name), otherwise returns `None`.
///
/// # Example
/// ```
/// use commander_core::notification_parser::parse_notification;
///
/// let notification = r"[timer] 1 new session(s) waiting for input:
///    @izzie-33 - masa@Masas-Studio:/Users/masa/Projects/izzie2 (main*?)";
///
/// let parsed = parse_notification(notification);
/// assert!(parsed.is_some());
/// let status = parsed.unwrap();
/// assert_eq!(status.name, "izzie-33");
/// assert_eq!(status.path, Some("/Users/masa/Projects/izzie2".to_string()));
/// ```
pub fn parse_notification(raw: &str) -> Option<ParsedSessionStatus> {
    // Strip ANSI codes first
    let clean = strip_ansi(raw);

    // Extract session name (required)
    let session_cap = SESSION_REGEX.captures(&clean)?;
    let name = session_cap.get(1)?.as_str().to_string();

    // Extract path info (optional)
    let (path, _user_host) = if let Some(path_cap) = PATH_REGEX.captures(&clean) {
        let user = path_cap.get(1).map(|m| m.as_str());
        let host = path_cap.get(2).map(|m| m.as_str());
        let p = path_cap.get(3).map(|m| m.as_str().to_string());
        let uh = match (user, host) {
            (Some(u), Some(h)) => Some(format!("{}@{}", u, h)),
            _ => None,
        };
        (p, uh)
    } else {
        (None, None)
    };

    // Extract branch and git status (optional)
    let (branch, git_status) = if let Some(branch_cap) = BRANCH_REGEX.captures(&clean) {
        let b = branch_cap.get(1).map(|m| m.as_str().to_string());
        let gs = branch_cap.get(2).and_then(|m| {
            let s = m.as_str();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        });
        (b, gs)
    } else {
        (None, None)
    };

    // Extract model info (optional)
    let (model, framework, context_usage) = if let Some(model_cap) = MODEL_REGEX.captures(&clean) {
        let m = model_cap.get(1).map(|c| c.as_str().to_string());
        let f = model_cap.get(2).map(|c| c.as_str().to_string());
        let cu = model_cap
            .get(3)
            .and_then(|c| c.as_str().parse::<u8>().ok());
        (m, f, cu)
    } else {
        (None, None, None)
    };

    Some(ParsedSessionStatus {
        name,
        path,
        branch,
        git_status,
        model,
        framework,
        context_usage,
    })
}

/// Parse a session preview line to extract status information.
///
/// This is used when parsing individual session lines from multi-session notifications.
/// The session name is provided separately (already extracted from @mention).
///
/// # Example
/// ```
/// use commander_core::notification_parser::parse_session_preview;
///
/// let preview = "masa@Masas-Studio:/Users/masa/Projects/izzie2 (main*?) [model|Claude MPM|70%]";
/// let status = parse_session_preview("izzie-33", preview);
///
/// assert_eq!(status.name, "izzie-33");
/// assert_eq!(status.path, Some("/Users/masa/Projects/izzie2".to_string()));
/// assert_eq!(status.context_usage, Some(70));
/// ```
pub fn parse_session_preview(session_name: &str, preview: &str) -> ParsedSessionStatus {
    // Strip ANSI codes
    let clean = strip_ansi(preview);

    // Extract path info
    let path = PATH_REGEX
        .captures(&clean)
        .and_then(|cap| cap.get(3).map(|m| m.as_str().to_string()));

    // Extract branch and git status
    let (branch, git_status) = if let Some(branch_cap) = BRANCH_REGEX.captures(&clean) {
        let b = branch_cap.get(1).map(|m| m.as_str().to_string());
        let gs = branch_cap.get(2).and_then(|m| {
            let s = m.as_str();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        });
        (b, gs)
    } else {
        (None, None)
    };

    // Extract model info
    let (model, framework, context_usage) = if let Some(model_cap) = MODEL_REGEX.captures(&clean) {
        let m = model_cap.get(1).map(|c| c.as_str().to_string());
        let f = model_cap.get(2).map(|c| c.as_str().to_string());
        let cu = model_cap
            .get(3)
            .and_then(|c| c.as_str().parse::<u8>().ok());
        (m, f, cu)
    } else {
        (None, None, None)
    };

    ParsedSessionStatus {
        name: session_name.to_string(),
        path,
        branch,
        git_status,
        model,
        framework,
        context_usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_basic() {
        let input = "text \x1B[90mgrayed\x1B[0m normal";
        assert_eq!(strip_ansi(input), "text grayed normal");
    }

    #[test]
    fn test_strip_ansi_multiple_codes() {
        let input = "\x1B[1m\x1B[31mbold red\x1B[0m and \x1B[32mgreen\x1B[0m";
        assert_eq!(strip_ansi(input), "bold red and green");
    }

    #[test]
    fn test_strip_ansi_no_codes() {
        let input = "plain text without codes";
        assert_eq!(strip_ansi(input), "plain text without codes");
    }

    #[test]
    fn test_parse_notification_full_format() {
        let notification = "[timer] 1 new session(s) waiting for input:\n   @izzie-33 - masa@Masas-Studio:/Users/masa/Projects/izzie2 (main*?) \x1B[90m[us.anthropic.claude-opus-4-5-20251101-v1:0|Claude MPM|70%]\x1B[0m";

        let parsed = parse_notification(notification).expect("Should parse successfully");

        assert_eq!(parsed.name, "izzie-33");
        assert_eq!(
            parsed.path,
            Some("/Users/masa/Projects/izzie2".to_string())
        );
        assert_eq!(parsed.branch, Some("main".to_string()));
        assert_eq!(parsed.git_status, Some("*?".to_string()));
        assert_eq!(
            parsed.model,
            Some("us.anthropic.claude-opus-4-5-20251101-v1:0".to_string())
        );
        assert_eq!(parsed.framework, Some("Claude MPM".to_string()));
        assert_eq!(parsed.context_usage, Some(70));
    }

    #[test]
    fn test_parse_notification_minimal() {
        let notification = "@test-session is ready";

        let parsed = parse_notification(notification).expect("Should parse successfully");

        assert_eq!(parsed.name, "test-session");
        assert_eq!(parsed.path, None);
        assert_eq!(parsed.branch, None);
        assert_eq!(parsed.git_status, None);
        assert_eq!(parsed.model, None);
        assert_eq!(parsed.framework, None);
        assert_eq!(parsed.context_usage, None);
    }

    #[test]
    fn test_parse_notification_with_path_only() {
        let notification = "@dev-42 - user@host:/home/user/project";

        let parsed = parse_notification(notification).expect("Should parse successfully");

        assert_eq!(parsed.name, "dev-42");
        assert_eq!(parsed.path, Some("/home/user/project".to_string()));
        assert_eq!(parsed.branch, None);
    }

    #[test]
    fn test_parse_notification_with_branch_no_status() {
        let notification = "@session-1 - user@host:/path (feature/new)";

        let parsed = parse_notification(notification).expect("Should parse successfully");

        assert_eq!(parsed.name, "session-1");
        assert_eq!(parsed.branch, Some("feature/new".to_string()));
        assert_eq!(parsed.git_status, None);
    }

    #[test]
    fn test_parse_notification_no_session() {
        let notification = "Some random notification without session mention";

        let parsed = parse_notification(notification);

        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_notification_inbox_format() {
        let notification = "[inbox] @my-session is ready";

        let parsed = parse_notification(notification).expect("Should parse successfully");

        assert_eq!(parsed.name, "my-session");
    }

    #[test]
    fn test_parse_notification_clock_format() {
        let notification = "[clock] 2 new session(s) waiting for input:\n   @session-a\n   @session-b";

        // parse_notification only extracts the first session
        let parsed = parse_notification(notification).expect("Should parse successfully");

        assert_eq!(parsed.name, "session-a");
    }

    #[test]
    fn test_parse_session_preview_full() {
        let preview = "masa@Masas-Studio:/Users/masa/Projects/izzie2 (main*?) \x1B[90m[us.anthropic.claude-opus-4-5-20251101-v1:0|Claude MPM|70%]\x1B[0m";

        let status = parse_session_preview("izzie-33", preview);

        assert_eq!(status.name, "izzie-33");
        assert_eq!(
            status.path,
            Some("/Users/masa/Projects/izzie2".to_string())
        );
        assert_eq!(status.branch, Some("main".to_string()));
        assert_eq!(status.git_status, Some("*?".to_string()));
        assert_eq!(
            status.model,
            Some("us.anthropic.claude-opus-4-5-20251101-v1:0".to_string())
        );
        assert_eq!(status.framework, Some("Claude MPM".to_string()));
        assert_eq!(status.context_usage, Some(70));
    }

    #[test]
    fn test_parse_session_preview_minimal() {
        let preview = "Some basic preview text";

        let status = parse_session_preview("test", preview);

        assert_eq!(status.name, "test");
        assert_eq!(status.path, None);
        assert_eq!(status.branch, None);
    }

    #[test]
    fn test_context_usage_edge_cases() {
        // 100% usage
        let notification = "@session - [model|framework|100%]";
        let parsed = parse_notification(notification).expect("Should parse successfully");
        assert_eq!(parsed.context_usage, Some(100));

        // 0% usage
        let notification = "@session - [model|framework|0%]";
        let parsed = parse_notification(notification).expect("Should parse successfully");
        assert_eq!(parsed.context_usage, Some(0));
    }

    #[test]
    fn test_branch_with_various_git_status_flags() {
        // All common flags: * = modified, ? = untracked, ! = ignored, + = staged, - = deleted
        let test_cases = vec![
            ("@s - (main*)", "main", Some("*".to_string())),
            ("@s - (main?)", "main", Some("?".to_string())),
            ("@s - (main*?)", "main", Some("*?".to_string())),
            ("@s - (main+)", "main", Some("+".to_string())),
            ("@s - (develop)", "develop", None),
            (
                "@s - (feature/test-123*?)",
                "feature/test-123",
                Some("*?".to_string()),
            ),
        ];

        for (notification, expected_branch, expected_status) in test_cases {
            let parsed = parse_notification(notification).expect("Should parse successfully");
            assert_eq!(
                parsed.branch,
                Some(expected_branch.to_string()),
                "Failed for: {}",
                notification
            );
            assert_eq!(
                parsed.git_status, expected_status,
                "Failed for: {}",
                notification
            );
        }
    }

    #[test]
    fn test_session_names_with_special_chars() {
        let test_cases = vec![
            "@simple", "@with-dash", "@with_underscore", "@mixed-name_123",
        ];

        for notification in test_cases {
            let parsed = parse_notification(notification);
            assert!(
                parsed.is_some(),
                "Should parse session from: {}",
                notification
            );
        }
    }

    #[test]
    fn test_path_with_spaces_not_supported() {
        // Current regex doesn't support paths with spaces - this is expected behavior
        let notification = "@session - user@host:/path with spaces/project";
        let parsed = parse_notification(notification).expect("Should parse session");

        // Path parsing stops at whitespace
        assert_eq!(parsed.path, Some("/path".to_string()));
    }

    #[test]
    fn test_to_conversational_full() {
        let status = ParsedSessionStatus {
            name: "izzie-33".to_string(),
            path: Some("/Users/masa/Projects/izzie2".to_string()),
            branch: Some("main".to_string()),
            git_status: Some("*?".to_string()),
            model: Some("claude-opus-4-5".to_string()),
            framework: Some("Claude MPM".to_string()),
            context_usage: Some(68),
        };

        let text = status.to_conversational();
        assert!(text.contains("izzie-33"), "Should contain session name");
        assert!(text.contains("izzie2"), "Should contain project name");
        assert!(text.contains("main"), "Should contain branch");
        assert!(text.contains("modified"), "Should describe git status");
        assert!(text.contains("68%"), "Should contain context usage");
        assert!(text.contains("Opus"), "Should contain model name");
    }

    #[test]
    fn test_to_conversational_minimal() {
        let status = ParsedSessionStatus {
            name: "test".to_string(),
            path: None,
            branch: None,
            git_status: None,
            model: None,
            framework: None,
            context_usage: None,
        };

        let text = status.to_conversational();
        assert_eq!(text, "Session \"test\"");
    }

    #[test]
    fn test_to_brief() {
        let status = ParsedSessionStatus {
            name: "izzie-33".to_string(),
            path: Some("/Users/masa/Projects/izzie2".to_string()),
            branch: Some("main".to_string()),
            git_status: Some("*".to_string()),
            model: None,
            framework: None,
            context_usage: Some(68),
        };

        let brief = status.to_brief();
        assert!(brief.contains("izzie2"), "Should contain project name");
        assert!(brief.contains("main"), "Should contain branch");
        assert!(brief.contains("68%"), "Should contain context");
    }

    #[test]
    fn test_describe_git_status() {
        assert!(describe_git_status("*").contains("modified"));
        assert!(describe_git_status("?").contains("untracked"));
        assert!(describe_git_status("*?").contains("modified"));
        assert!(describe_git_status("*?").contains("untracked"));
        assert!(describe_git_status("+").contains("staged"));
    }

    #[test]
    fn test_simplify_model_name() {
        assert_eq!(simplify_model_name("claude-opus-4-5-20251101"), "Claude Opus");
        assert_eq!(simplify_model_name("claude-sonnet-4-20250514"), "Claude Sonnet");
        assert_eq!(simplify_model_name("claude-3-haiku"), "Claude Haiku");
        assert_eq!(simplify_model_name("gpt-4-turbo"), "GPT-4");
        assert_eq!(simplify_model_name("unknown-model"), "");
    }

    #[test]
    fn test_context_usage_descriptions() {
        // High context usage
        let status = ParsedSessionStatus {
            name: "test".to_string(),
            path: None,
            branch: None,
            git_status: None,
            model: None,
            framework: None,
            context_usage: Some(95),
        };
        let text = status.to_conversational();
        assert!(text.contains("critical"), "95% should be critical");

        // Medium-high context usage
        let status = ParsedSessionStatus {
            name: "test".to_string(),
            path: None,
            branch: None,
            git_status: None,
            model: None,
            framework: None,
            context_usage: Some(75),
        };
        let text = status.to_conversational();
        assert!(text.contains("getting full"), "75% should be getting full");
    }
}
