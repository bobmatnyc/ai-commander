//! Blocker classification logic for the User Agent.
//!
//! Contains methods to classify errors as blockers and extract
//! blocker information from agent responses.

use crate::completion_driver::{Blocker, BlockerType};
use crate::error::AgentError;

use super::UserAgent;

impl UserAgent {
    /// Classify an error to determine if it should create a blocker.
    pub(crate) fn classify_error_as_blocker(&self, error: &AgentError) -> Option<Blocker> {
        match error {
            AgentError::Configuration(msg) => {
                Some(Blocker::external(format!("Configuration error: {}", msg)))
            }
            AgentError::MaxIterationsExceeded(_) => Some(Blocker::new(
                "Maximum iterations reached - may need guidance",
                BlockerType::DecisionNeeded,
            )),
            AgentError::ToolExecution { tool_name, message } => {
                // Some tool errors are recoverable
                if message.contains("not found") || message.contains("permission") {
                    Some(Blocker::error_judgment(
                        format!("Tool '{}' failed: {}", tool_name, message),
                        vec![
                            "Retry".into(),
                            "Skip this step".into(),
                            "Try alternative".into(),
                        ],
                    ))
                } else {
                    None // Recoverable
                }
            }
            _ => None, // Most errors are recoverable
        }
    }

    /// Extract blocker reason from response text.
    pub(crate) fn extract_blocker_reason(&self, content: &str) -> String {
        // Look for text after [BLOCKED] marker
        if let Some(idx) = content.to_lowercase().find("[blocked]") {
            let after = &content[idx + 9..];
            let reason = after
                .lines()
                .next()
                .unwrap_or("User input needed")
                .trim()
                .trim_start_matches(':')
                .trim();
            if !reason.is_empty() {
                return reason.to_string();
            }
        }

        // Look for "need" phrases
        for line in content.lines() {
            let lower = line.to_lowercase();
            if lower.contains("need")
                && (lower.contains("input")
                    || lower.contains("decision")
                    || lower.contains("information"))
            {
                return line.trim().to_string();
            }
        }

        "User input needed to proceed".to_string()
    }

    /// Classify the type of blocker from response text.
    pub(crate) fn classify_blocker_type(&self, content: &str) -> BlockerType {
        let lower = content.to_lowercase();

        if lower.contains("decision") || lower.contains("choose") || lower.contains("option") {
            BlockerType::DecisionNeeded
        } else if lower.contains("credential")
            || lower.contains("api key")
            || lower.contains("access")
        {
            BlockerType::ExternalDependency
        } else if lower.contains("error") || lower.contains("failed") {
            BlockerType::ErrorRequiresJudgment
        } else if lower.contains("unclear") || lower.contains("ambiguous") || lower.contains("which")
        {
            BlockerType::AmbiguousRequirements
        } else {
            BlockerType::InformationNeeded
        }
    }

    /// Extract options from response text.
    pub(crate) fn extract_options(&self, content: &str) -> Vec<String> {
        let mut options = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            // Look for numbered options like "1. " or "1) "
            if trimmed.len() > 2 {
                let first_char = trimmed.chars().next().unwrap_or(' ');
                if first_char.is_ascii_digit() {
                    let rest = trimmed[1..].trim_start_matches(['.', ')', ':', ' '].as_ref());
                    if !rest.is_empty() {
                        options.push(rest.to_string());
                    }
                }
            }
        }

        options
    }
}
