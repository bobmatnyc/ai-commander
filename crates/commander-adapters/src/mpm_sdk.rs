//! MPM SDK event-driven adapter.
//!
//! Wraps `mpm_sdk::MpmClient` and translates its `AgentEvent` stream into
//! [`RuntimeEvent`]s. One `MpmClient` is created per session and stored
//! keyed by the session handle id, so follow-up `send` calls reuse the same
//! client (which internally tracks `last_session_id` for `--resume`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;

use mpm_sdk::{AgentEvent, MpmClient};

use crate::event_driven::{EventDrivenAdapter, EventStream, RuntimeEvent, SessionHandle};
use crate::traits::AdapterInfo;

/// Default agent id used by the MPM SDK adapter. Matches the telegram bot's
/// choice (see `commander-telegram/src/handlers.rs::spawn_agent_with_streaming`).
const DEFAULT_AGENT_ID: &str = "pm";

/// Channel buffer for agent event streams.
const EVENT_CHANNEL_BUFFER: usize = 64;

/// Event-driven adapter that drives `claude-mpm` via the headless SDK.
///
/// Each call to [`start_session`] creates a new [`MpmClient`] and stores it
/// keyed by a freshly generated UUID. Follow-up [`send`] calls look up the
/// client and invoke `run_streaming` again — the client auto-resumes via its
/// internal `last_session_id`.
///
/// [`start_session`]: EventDrivenAdapter::start_session
/// [`send`]: EventDrivenAdapter::send
pub struct MpmSdkAdapter {
    info: AdapterInfo,
    /// Per-session `MpmClient`s keyed by `SessionHandle.id`.
    ///
    /// Each client is wrapped in its own `Mutex` so concurrent sessions can
    /// run in parallel while only serializing calls within a single session.
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<MpmClient>>>>>,
}

impl MpmSdkAdapter {
    /// Creates a new MPM SDK adapter with default metadata.
    pub fn new() -> Self {
        Self {
            info: AdapterInfo {
                id: "mpm-sdk".to_string(),
                name: "MPM SDK".to_string(),
                description: "Headless MPM via event-stream SDK".to_string(),
                command: "claude-mpm".to_string(),
                default_args: vec![],
            },
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Locates the `claude-mpm` binary and constructs an `MpmClient`
    /// rooted at the given project path.
    ///
    /// `MpmClient::discover()` would use the process's current working
    /// directory, which isn't what we want — each session runs against a
    /// specific `project_path`. We resolve the binary via `which` here and
    /// then call `MpmClient::new` with our chosen cwd.
    fn build_client(project_path: &str) -> Result<MpmClient, String> {
        let output = std::process::Command::new("which")
            .arg("claude-mpm")
            .output()
            .map_err(|e| format!("failed to run `which claude-mpm`: {}", e))?;

        if !output.status.success() {
            return Err("claude-mpm not found in PATH".to_string());
        }

        let binary = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if binary.is_empty() {
            return Err("`which claude-mpm` returned empty path".to_string());
        }

        Ok(MpmClient::new(binary, PathBuf::from(project_path)))
    }

    /// Runs a single turn against the given client and returns an
    /// [`EventStream`] of [`RuntimeEvent`]s.
    ///
    /// Spawns a background task that drives `run_streaming`, feeding
    /// `AgentEvent`s through an `mpsc` channel. The returned stream maps
    /// each event into a `RuntimeEvent`.
    fn run_turn(client: Arc<Mutex<MpmClient>>, prompt: String) -> EventStream {
        let (tx, rx) = tokio::sync::mpsc::channel::<AgentEvent>(EVENT_CHANNEL_BUFFER);

        // Background task: acquire the per-session lock and drive the client.
        tokio::spawn(async move {
            let err_tx = tx.clone();
            let mut guard = client.lock().await;
            if let Err(e) = guard
                .run_streaming(DEFAULT_AGENT_ID, &prompt, tx)
                .await
            {
                let _ = err_tx.send(AgentEvent::Error(e.to_string())).await;
            }
        });

        // Map each AgentEvent → RuntimeEvent.
        let stream = ReceiverStream::new(rx).map(map_agent_event);
        Box::pin(stream)
    }
}

impl Default for MpmSdkAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventDrivenAdapter for MpmSdkAdapter {
    fn info(&self) -> &AdapterInfo {
        &self.info
    }

    async fn start_session(
        &self,
        project_path: &str,
        prompt: &str,
    ) -> Result<(SessionHandle, EventStream), String> {
        let client = Self::build_client(project_path)?;
        let client = Arc::new(Mutex::new(client));

        let id = uuid::Uuid::new_v4().to_string();
        {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(id.clone(), Arc::clone(&client));
        }

        let stream = Self::run_turn(client, prompt.to_string());
        Ok((SessionHandle { id }, stream))
    }

    async fn send(
        &self,
        handle: &SessionHandle,
        message: &str,
    ) -> Result<EventStream, String> {
        let client = {
            let sessions = self.sessions.lock().await;
            sessions
                .get(&handle.id)
                .cloned()
                .ok_or_else(|| format!("unknown session: {}", handle.id))?
        };

        Ok(Self::run_turn(client, message.to_string()))
    }

    async fn stop(&self, handle: SessionHandle) -> Result<(), String> {
        // Idempotent: removing a missing key is fine. Dropping the last Arc
        // to the client triggers its cleanup.
        let mut sessions = self.sessions.lock().await;
        sessions.remove(&handle.id);
        Ok(())
    }
}

/// Maps an `mpm_sdk::AgentEvent` into a [`RuntimeEvent`].
fn map_agent_event(event: AgentEvent) -> RuntimeEvent {
    match event {
        AgentEvent::Text(s) => RuntimeEvent::TextChunk(s),
        AgentEvent::ToolUse(name) => RuntimeEvent::ToolUse { name },
        AgentEvent::Complete(result) => {
            // Build a brief summary from the AgentResult.
            // `result.text` is typically the full response; we surface it as
            // the summary. Callers that care about size can truncate.
            let summary = if result.text.is_empty() {
                None
            } else {
                Some(result.text)
            };
            RuntimeEvent::Complete { summary }
        }
        AgentEvent::Error(e) => RuntimeEvent::Error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_info() {
        let adapter = MpmSdkAdapter::new();
        let info = adapter.info();
        assert_eq!(info.id, "mpm-sdk");
        assert_eq!(info.name, "MPM SDK");
        assert_eq!(info.command, "claude-mpm");
        assert!(info.default_args.is_empty());
        assert!(info.description.to_lowercase().contains("mpm"));
    }

    #[test]
    fn test_map_agent_event_text() {
        let e = map_agent_event(AgentEvent::Text("hello".into()));
        match e {
            RuntimeEvent::TextChunk(s) => assert_eq!(s, "hello"),
            _ => panic!("expected TextChunk"),
        }
    }

    #[test]
    fn test_map_agent_event_tool_use() {
        let e = map_agent_event(AgentEvent::ToolUse("grep".into()));
        match e {
            RuntimeEvent::ToolUse { name } => assert_eq!(name, "grep"),
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn test_map_agent_event_error() {
        let e = map_agent_event(AgentEvent::Error("boom".into()));
        match e {
            RuntimeEvent::Error(s) => assert_eq!(s, "boom"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_map_agent_event_complete_with_text() {
        use mpm_sdk::AgentResult;
        let result = AgentResult {
            text: "done.".to_string(),
            session_id: Some("s1".into()),
            cost_usd: None,
            duration_ms: 100,
            is_error: false,
            num_turns: Some(1),
            runtime: None,
        };
        let e = map_agent_event(AgentEvent::Complete(result));
        match e {
            RuntimeEvent::Complete { summary } => assert_eq!(summary.as_deref(), Some("done.")),
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn test_map_agent_event_complete_empty_text() {
        use mpm_sdk::AgentResult;
        let result = AgentResult {
            text: String::new(),
            session_id: None,
            cost_usd: None,
            duration_ms: 0,
            is_error: false,
            num_turns: None,
            runtime: None,
        };
        let e = map_agent_event(AgentEvent::Complete(result));
        match e {
            RuntimeEvent::Complete { summary } => assert!(summary.is_none()),
            _ => panic!("expected Complete"),
        }
    }

    #[tokio::test]
    async fn test_stop_idempotent_on_unknown_handle() {
        let adapter = MpmSdkAdapter::new();
        let handle = SessionHandle {
            id: "nonexistent".to_string(),
        };
        // Should succeed even for a handle that was never registered.
        assert!(adapter.stop(handle).await.is_ok());
    }

    #[tokio::test]
    async fn test_send_unknown_session_returns_error() {
        let adapter = MpmSdkAdapter::new();
        let handle = SessionHandle {
            id: "nonexistent".to_string(),
        };
        let result = adapter.send(&handle, "hi").await;
        match result {
            Err(msg) => assert!(msg.contains("unknown session"), "got: {}", msg),
            Ok(_) => panic!("expected unknown session error"),
        }
    }
}
