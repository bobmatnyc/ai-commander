//! Client adapter trait for rendering session output across different frontends.
//!
//! The interpretation pipeline (capture → clean → detect → interpret) is shared.
//! Each client adapter renders the results differently based on its capabilities.

use serde::{Deserialize, Serialize};

/// What a client can render.
#[derive(Debug, Clone)]
pub struct ClientCapabilities {
    pub markdown: bool,
    pub html: bool,
    pub collapsible_sections: bool,
    pub live_streaming: bool,
    pub raw_toggle: bool,
    pub inline_buttons: bool,
    pub max_message_length: usize,
    pub rate_limit_ms: u64,
}

/// Interpreted output from a session — client-agnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterpretedOutput {
    pub raw: String,
    pub cleaned: String,
    pub summary: Option<String>,
    /// Adapter name: "Claude", "Shell", or "Unknown"
    pub adapter: String,
    pub is_idle: bool,
    pub new_lines: Vec<String>,
    pub has_selector: bool,
    pub selector_question: Option<String>,
    pub selector_options: Vec<String>,
}

/// Session status for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session_name: String,
    pub adapter: String,
    pub is_idle: bool,
    pub interpretation: Option<String>,
}

/// A rendered fragment from a client adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientFragment {
    Text(String),
    StatusUpdate(String),
    Selector {
        question: String,
        options: Vec<String>,
    },
    ToolUse {
        name: String,
        detail: Option<String>,
    },
    Error(String),
    Progress {
        line_count: usize,
        summary: Option<String>,
    },
}

/// Trait for client-specific rendering.
pub trait ClientRenderer: Send + Sync {
    fn capabilities(&self) -> &ClientCapabilities;
    fn render_output(&self, output: &InterpretedOutput) -> Vec<ClientFragment>;
    fn render_status(&self, status: &SessionStatus) -> ClientFragment;
    fn render_error(&self, error: &str) -> ClientFragment;
    fn truncate(&self, text: &str, max_len: usize) -> String;
}

/// Run the shared interpretation pipeline on raw tmux output.
///
/// This is client-agnostic — call this, then pass results to a [`ClientRenderer`].
///
/// # Arguments
/// * `raw`          - Raw terminal output (e.g. from `tmux capture-pane`).
/// * `prev_output`  - Previous capture for diff-based new-line extraction.
///                    Pass `None` on the first call to treat all lines as new.
pub fn interpret_output(raw: &str, prev_output: Option<&str>) -> InterpretedOutput {
    use crate::output_filter;

    let cleaned = output_filter::clean_response(raw);
    let adapter = output_filter::detect_adapter(&cleaned);
    let is_idle =
        output_filter::is_claude_ready(&cleaned) || output_filter::is_mpm_ready(&cleaned);

    let new_lines = match prev_output {
        Some(prev) => output_filter::find_new_lines(prev, &cleaned),
        None => cleaned.lines().map(|l| l.to_string()).collect(),
    };

    let selector = output_filter::detect_selector(&cleaned);
    let (has_selector, selector_question, selector_options) = match selector {
        Some(s) => (true, Some(s.question.clone()), s.options.clone()),
        None => (false, None, Vec::new()),
    };

    InterpretedOutput {
        raw: raw.to_string(),
        cleaned,
        // Summary is populated async by the caller if needed.
        summary: None,
        adapter: format!("{:?}", adapter),
        is_idle,
        new_lines,
        has_selector,
        selector_question,
        selector_options,
    }
}

/// Run interpretation and populate `summary` via LLM screen-context analysis.
///
/// Calls [`interpret_output`] then attempts to produce a one-sentence
/// interpretation using [`crate::summarizer::interpret_screen_context`].
/// Falls back gracefully — if the LLM call fails `summary` is left as `None`.
///
/// # Arguments
/// * `raw`         - Raw terminal output.
/// * `prev_output` - Previous capture for diff-based new-line extraction.
pub fn interpret_output_with_summary(raw: &str, prev_output: Option<&str>) -> InterpretedOutput {
    let mut output = interpret_output(raw, prev_output);

    let interpretation =
        crate::summarizer::interpret_screen_context(&output.cleaned, output.is_idle);
    if let Some(text) = interpretation {
        if !text.is_empty() {
            output.summary = Some(text);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpret_output_empty() {
        let result = interpret_output("", None);
        assert!(result.raw.is_empty());
        assert!(result.cleaned.is_empty());
        assert!(!result.is_idle);
        assert!(result.new_lines.is_empty());
        assert!(!result.has_selector);
        assert!(result.summary.is_none());
    }

    #[test]
    fn test_interpret_output_new_lines_from_diff() {
        let prev = "line1\nline2\n";
        let current = "line1\nline2\nline3\n";
        let result = interpret_output(current, Some(prev));
        assert!(result.new_lines.contains(&"line3".to_string()));
        assert!(!result.new_lines.contains(&"line1".to_string()));
    }

    #[test]
    fn test_interpret_output_all_lines_when_no_prev() {
        let raw = "line1\nline2\n";
        let result = interpret_output(raw, None);
        assert!(result.new_lines.contains(&"line1".to_string()));
        assert!(result.new_lines.contains(&"line2".to_string()));
    }

    #[test]
    fn test_interpret_output_adapter_field_is_string() {
        let result = interpret_output("", None);
        // adapter must be one of the Debug representations of output_filter::Adapter
        assert!(
            result.adapter == "Claude" || result.adapter == "Shell" || result.adapter == "Unknown"
        );
    }

    #[test]
    fn test_interpreted_output_serializable() {
        let output = InterpretedOutput {
            raw: "raw".to_string(),
            cleaned: "cleaned".to_string(),
            summary: None,
            adapter: "Unknown".to_string(),
            is_idle: false,
            new_lines: vec![],
            has_selector: false,
            selector_question: None,
            selector_options: vec![],
        };
        let serialized = serde_json::to_string(&output).expect("serialize");
        assert!(serialized.contains("cleaned"));
    }

    #[test]
    fn test_client_fragment_serializable() {
        let fragment = ClientFragment::Selector {
            question: "Which option?".to_string(),
            options: vec!["A".to_string(), "B".to_string()],
        };
        let serialized = serde_json::to_string(&fragment).expect("serialize");
        assert!(serialized.contains("Which option?"));
    }

    #[test]
    fn test_session_status_serializable() {
        let status = SessionStatus {
            session_name: "my-session".to_string(),
            adapter: "Claude".to_string(),
            is_idle: true,
            interpretation: Some("Claude is asking: proceed?".to_string()),
        };
        let serialized = serde_json::to_string(&status).expect("serialize");
        assert!(serialized.contains("my-session"));
    }
}
