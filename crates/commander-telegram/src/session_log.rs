//! Session logging for user↔assistant exchanges.
//!
//! Appends one JSON object per line to `~/.ai-commander/logs/sessions.jsonl`
//! for debugging and evals.  All I/O errors are silently swallowed so logging
//! never disrupts the main message flow.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::json;

/// Append-only logger that writes JSONL records to a session log file.
pub struct SessionLogger {
    log_path: PathBuf,
}

impl SessionLogger {
    /// Create a new logger whose output goes to `logs_dir/sessions.jsonl`.
    pub fn new(logs_dir: PathBuf) -> Self {
        Self {
            log_path: logs_dir.join("sessions.jsonl"),
        }
    }

    fn append(&self, record: serde_json::Value) {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = writeln!(f, "{}", record);
        }
    }

    /// Log a user message sent to a session.
    pub fn log_user_message(
        &self,
        chat_id: i64,
        session_id: &str,
        project: &str,
        message: &str,
        via_daemon: bool,
        msg_id: i32,
    ) {
        self.append(json!({
            "ts": Utc::now().to_rfc3339(),
            "event": "user_message",
            "chat_id": chat_id,
            "session_id": session_id,
            "project": project,
            "message": message,
            "via_daemon": via_daemon,
            "msg_id": msg_id,
        }));
    }

    /// Log an assistant response received from a session.
    pub fn log_assistant_response(
        &self,
        chat_id: i64,
        session_id: &str,
        project: &str,
        response: &str,
        latency_ms: u64,
        msg_id: i32,
    ) {
        self.append(json!({
            "ts": Utc::now().to_rfc3339(),
            "event": "assistant_response",
            "chat_id": chat_id,
            "session_id": session_id,
            "project": project,
            "response": response,
            "response_len": response.len(),
            "latency_ms": latency_ms,
            "msg_id": msg_id,
        }));
    }
}
