//! NDJSON output parser for Claude Code stream-json format.
//!
//! Parses lines produced by `claude-mpm run --headless --output-format stream-json`.
//!
//! Line formats:
//! ```json
//! {"type":"system","subtype":"init","session_id":"abc123"}
//! {"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}
//! {"type":"tool_use","name":"Bash","input":{}}
//! {"type":"result","subtype":"success","result":"...","cost_usd":0.003}
//! {"type":"result","subtype":"error","error":"..."}
//! ```

use crate::types::{AgentEvent, AgentResult};

/// Parse a single NDJSON line into an `AgentEvent`, if the line is recognised.
pub fn parse_ndjson_line(line: &str) -> Option<AgentEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let type_str = v.get("type")?.as_str()?;

    match type_str {
        "assistant" => {
            // {"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}
            let content = v
                .pointer("/message/content")
                .and_then(|c| c.as_array())?;
            let mut text = String::new();
            for item in content {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        text.push_str(t);
                    }
                }
            }
            if text.is_empty() {
                None
            } else {
                Some(AgentEvent::Text(text))
            }
        }
        "tool_use" => {
            let name = v.get("name")?.as_str().unwrap_or("unknown").to_string();
            Some(AgentEvent::ToolUse(name))
        }
        "result" => {
            let subtype = v.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
            let is_error = subtype == "error";
            let text = if is_error {
                v.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown error")
                    .to_string()
            } else {
                v.get("result")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            let session_id = v
                .get("session_id")
                .and_then(|s| s.as_str())
                .map(String::from);
            let cost_usd = v.get("cost_usd").and_then(|c| c.as_f64());
            let duration_ms = v
                .get("duration_ms")
                .and_then(|d| d.as_u64())
                .unwrap_or(0);

            Some(AgentEvent::Complete(AgentResult {
                text,
                session_id,
                cost_usd,
                duration_ms,
                is_error,
            }))
        }
        _ => None,
    }
}

/// Extract the session ID from a system/init line.
pub fn extract_session_id(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("type")?.as_str()? == "system"
        && v.get("subtype").and_then(|s| s.as_str()) == Some("init")
    {
        v.get("session_id")
            .and_then(|s| s.as_str())
            .map(String::from)
    } else {
        None
    }
}

/// Returns true if this line signals that the agent run is complete.
pub fn is_completion_line(line: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
        return false;
    };
    v.get("type").and_then(|t| t.as_str()) == Some("result")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_assistant_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello!"}]}}"#;
        match parse_ndjson_line(line) {
            Some(AgentEvent::Text(t)) => assert_eq!(t, "Hello!"),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn test_parse_tool_use() {
        let line = r#"{"type":"tool_use","name":"Bash","input":{"command":"ls"}}"#;
        match parse_ndjson_line(line) {
            Some(AgentEvent::ToolUse(n)) => assert_eq!(n, "Bash"),
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn test_parse_result_success() {
        let line = r#"{"type":"result","subtype":"success","result":"done","cost_usd":0.01,"duration_ms":1200}"#;
        match parse_ndjson_line(line) {
            Some(AgentEvent::Complete(r)) => {
                assert_eq!(r.text, "done");
                assert!(!r.is_error);
                assert_eq!(r.cost_usd, Some(0.01));
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn test_parse_result_error() {
        let line = r#"{"type":"result","subtype":"error","error":"agent failed"}"#;
        match parse_ndjson_line(line) {
            Some(AgentEvent::Complete(r)) => {
                assert!(r.is_error);
                assert_eq!(r.text, "agent failed");
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn test_extract_session_id() {
        let line = r#"{"type":"system","subtype":"init","session_id":"sess-123"}"#;
        assert_eq!(extract_session_id(line), Some("sess-123".to_string()));
    }

    #[test]
    fn test_is_completion_line() {
        assert!(is_completion_line(r#"{"type":"result","subtype":"success"}"#));
        assert!(!is_completion_line(r#"{"type":"assistant","message":{}}"#));
    }
}
