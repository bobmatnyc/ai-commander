//! MPM SDK event-driven adapter.
//!
//! Wraps `mpm_sdk::MpmClient` and translates its `AgentEvent` stream into
//! [`RuntimeEvent`]s. Supports two backends:
//!
//! 1. **Serve daemon** (preferred) — uses `claude-mpm serve` HTTP daemon
//!    (port 7777) via `UiServiceClient` for persistent sessions.
//! 2. **Subprocess** (fallback) — spawns a new `claude-mpm run --headless`
//!    process per session via `MpmClient`.
//!
//! The adapter lazily discovers/starts the daemon on the first `start_session`
//! call. If the daemon cannot be reached or started, it falls back to the
//! subprocess model transparently.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;

use mpm_sdk::{AgentEvent, CreateSessionRequest, MpmClient, ServeManager, UiServiceClient};

use crate::event_driven::{EventDrivenAdapter, EventStream, RuntimeEvent, SessionHandle};
use crate::traits::AdapterInfo;

/// Default agent id used by the MPM SDK adapter. Matches the telegram bot's
/// choice (see `commander-telegram/src/handlers.rs::spawn_agent_with_streaming`).
const DEFAULT_AGENT_ID: &str = "pm";

/// Channel buffer for agent event streams.
const EVENT_CHANNEL_BUFFER: usize = 64;

/// Default port for the `claude-mpm serve` daemon.
const SERVE_PORT: u16 = 7777;

/// Timeout in seconds when waiting for the daemon to become ready.
const DAEMON_STARTUP_TIMEOUT_SECS: u64 = 30;

/// Which backend is currently active.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BackendMode {
    /// Haven't attempted daemon discovery yet.
    Pending,
    /// Using the `claude-mpm serve` HTTP daemon.
    Serve,
    /// Fallback: per-session subprocess via `MpmClient`.
    Subprocess,
}

/// State tracked for each session running against the serve daemon.
#[derive(Debug, Clone)]
struct ServeSessionState {
    /// The session id assigned by the serve daemon.
    serve_session_id: String,
    /// The project path this session was started for.
    project_path: String,
}

/// Event-driven adapter that drives `claude-mpm` via either the persistent
/// serve daemon or a per-session headless subprocess.
///
/// On the first [`start_session`] call the adapter tries to connect to (or
/// start) the `claude-mpm serve` daemon. If the daemon is unavailable, it
/// falls back to spawning subprocess clients — the same behaviour as before
/// the daemon support was added.
///
/// [`start_session`]: EventDrivenAdapter::start_session
pub struct MpmSdkAdapter {
    info: AdapterInfo,
    backend_mode: Arc<Mutex<BackendMode>>,
    // -- Serve backend state --
    serve_manager: Arc<Mutex<Option<ServeManager>>>,
    serve_sessions: Arc<Mutex<HashMap<String, ServeSessionState>>>,
    // -- Subprocess backend state --
    subprocess_sessions: Arc<Mutex<HashMap<String, Arc<Mutex<MpmClient>>>>>,
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
            backend_mode: Arc::new(Mutex::new(BackendMode::Pending)),
            serve_manager: Arc::new(Mutex::new(None)),
            serve_sessions: Arc::new(Mutex::new(HashMap::new())),
            subprocess_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns the serve-daemon session id for the given adapter handle, if
    /// the session is running against the serve backend.
    ///
    /// This allows callers (e.g. the telegram layer) to persist the daemon
    /// session id for later resume.
    pub async fn get_serve_session_id(&self, handle: &SessionHandle) -> Option<String> {
        let sessions = self.serve_sessions.lock().await;
        sessions
            .get(&handle.id)
            .map(|s| s.serve_session_id.clone())
    }

    // -----------------------------------------------------------------------
    // Daemon lifecycle
    // -----------------------------------------------------------------------

    /// Ensures the serve daemon is running and returns a fresh
    /// `UiServiceClient`. On first call (mode == `Pending`) this will
    /// attempt discovery, health-check, and — if needed — start the daemon.
    ///
    /// Returns `Ok(client)` when the daemon is healthy, or `Err(reason)` if
    /// it could not be reached/started (caller should fall back to subprocess).
    async fn ensure_daemon_running(&self) -> Result<UiServiceClient, String> {
        let mut mode = self.backend_mode.lock().await;

        match *mode {
            BackendMode::Subprocess => {
                return Err("backend already set to subprocess".to_string());
            }
            BackendMode::Serve => {
                // Daemon was previously healthy — do a quick health check.
                let client = UiServiceClient::new(SERVE_PORT);
                if client.health().await.unwrap_or(false) {
                    return Ok(client);
                }
                // Daemon went away — try to restart.
                let mut mgr_guard = self.serve_manager.lock().await;
                if let Some(mgr) = mgr_guard.as_ref() {
                    match mgr.start_and_wait(DAEMON_STARTUP_TIMEOUT_SECS).await {
                        Ok(c) => return Ok(c),
                        Err(e) => {
                            eprintln!("[mpm-sdk] serve daemon restart failed, falling back to subprocess: {e}");
                            *mode = BackendMode::Subprocess;
                            *mgr_guard = None;
                            return Err(e.to_string());
                        }
                    }
                }
                // No manager — switch to subprocess.
                *mode = BackendMode::Subprocess;
                Err("serve manager lost".to_string())
            }
            BackendMode::Pending => {
                // First attempt: discover binary, check health, start if needed.
                match ServeManager::discover(SERVE_PORT) {
                    Ok(mgr) => {
                        // Already running?
                        let client = mgr.client();
                        if client.health().await.unwrap_or(false) {
                            eprintln!("[mpm-sdk] serve daemon already running on port {SERVE_PORT}");
                            *mode = BackendMode::Serve;
                            *self.serve_manager.lock().await = Some(mgr);
                            return Ok(client);
                        }
                        // Try to start.
                        match mgr.start_and_wait(DAEMON_STARTUP_TIMEOUT_SECS).await {
                            Ok(client) => {
                                eprintln!("[mpm-sdk] serve daemon started on port {SERVE_PORT}");
                                *mode = BackendMode::Serve;
                                *self.serve_manager.lock().await = Some(mgr);
                                Ok(client)
                            }
                            Err(e) => {
                                eprintln!(
                                    "[mpm-sdk] failed to start serve daemon, falling back to subprocess: {e}"
                                );
                                *mode = BackendMode::Subprocess;
                                Err(e.to_string())
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[mpm-sdk] claude-mpm binary not found for serve mode, falling back to subprocess: {e}"
                        );
                        *mode = BackendMode::Subprocess;
                        Err(e.to_string())
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Subprocess helpers (unchanged from original)
    // -----------------------------------------------------------------------

    /// Locates the `claude-mpm` binary and constructs an `MpmClient`
    /// rooted at the given project path.
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

    /// Runs a single turn against the given subprocess client and returns an
    /// [`EventStream`] of [`RuntimeEvent`]s.
    fn run_subprocess_turn(client: Arc<Mutex<MpmClient>>, prompt: String) -> EventStream {
        let (tx, rx) = tokio::sync::mpsc::channel::<AgentEvent>(EVENT_CHANNEL_BUFFER);

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

        let stream = ReceiverStream::new(rx).map(map_agent_event);
        Box::pin(stream)
    }

    // -----------------------------------------------------------------------
    // Serve-daemon streaming helper
    // -----------------------------------------------------------------------

    /// Sends a message to a serve-daemon session and returns a stream of
    /// [`RuntimeEvent`]s.
    ///
    /// Because `UiServiceClient` is not `Clone`, a fresh client is created
    /// per background task (constructing one is cheap — it just builds a
    /// `reqwest::Client`).
    fn run_serve_turn(serve_session_id: String, message: String) -> EventStream {
        let (tx, rx) = tokio::sync::mpsc::channel::<AgentEvent>(EVENT_CHANNEL_BUFFER);

        tokio::spawn(async move {
            let client = UiServiceClient::new(SERVE_PORT);
            if let Err(e) = client
                .send_message_streaming(&serve_session_id, &message, tx.clone())
                .await
            {
                let _ = tx.send(AgentEvent::Error(e.to_string())).await;
            }
        });

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
        resume_id: Option<&str>,
    ) -> Result<(SessionHandle, EventStream), String> {
        let handle_id = uuid::Uuid::new_v4().to_string();

        // Try the serve daemon first.
        if let Ok(_client) = self.ensure_daemon_running().await {
            let req = CreateSessionRequest {
                cwd: Some(project_path.to_string()),
                project_root: Some(project_path.to_string()),
                resume_id: resume_id.map(|s| s.to_string()),
                ..Default::default()
            };

            // Create a fresh client for the create_session call (the one from
            // ensure_daemon_running was only used for health probing).
            let client = UiServiceClient::new(SERVE_PORT);
            match client.create_session(req).await {
                Ok(session) => {
                    let serve_session_id = session.id.clone();

                    {
                        let mut sessions = self.serve_sessions.lock().await;
                        sessions.insert(
                            handle_id.clone(),
                            ServeSessionState {
                                serve_session_id: serve_session_id.clone(),
                                project_path: project_path.to_string(),
                            },
                        );
                    }

                    let stream =
                        Self::run_serve_turn(serve_session_id, prompt.to_string());
                    return Ok((SessionHandle { id: handle_id }, stream));
                }
                Err(e) => {
                    eprintln!("[mpm-sdk] serve create_session failed, falling back to subprocess: {e}");
                    // Fall through to subprocess path below.
                }
            }
        }

        // Subprocess fallback.
        let mut client = Self::build_client(project_path)?;
        // Pre-seed the session ID so `run_streaming` will use `--resume`.
        if let Some(rid) = resume_id {
            client.set_last_session_id(rid.to_string());
        }
        let client = Arc::new(Mutex::new(client));

        {
            let mut sessions = self.subprocess_sessions.lock().await;
            sessions.insert(handle_id.clone(), Arc::clone(&client));
        }

        let stream = Self::run_subprocess_turn(client, prompt.to_string());
        Ok((SessionHandle { id: handle_id }, stream))
    }

    async fn send(
        &self,
        handle: &SessionHandle,
        message: &str,
    ) -> Result<EventStream, String> {
        // Check serve sessions first.
        {
            let sessions = self.serve_sessions.lock().await;
            if let Some(state) = sessions.get(&handle.id) {
                return Ok(Self::run_serve_turn(
                    state.serve_session_id.clone(),
                    message.to_string(),
                ));
            }
        }

        // Check subprocess sessions.
        {
            let sessions = self.subprocess_sessions.lock().await;
            if let Some(client) = sessions.get(&handle.id) {
                return Ok(Self::run_subprocess_turn(
                    Arc::clone(client),
                    message.to_string(),
                ));
            }
        }

        Err(format!("unknown session: {}", handle.id))
    }

    async fn stop(&self, handle: SessionHandle) -> Result<(), String> {
        // Try removing from serve sessions and cleaning up the daemon session.
        let serve_state = {
            let mut sessions = self.serve_sessions.lock().await;
            sessions.remove(&handle.id)
        };

        if let Some(state) = serve_state {
            let client = UiServiceClient::new(SERVE_PORT);
            if let Err(e) = client.delete_session(&state.serve_session_id).await {
                eprintln!("[mpm-sdk] failed to delete serve session {}: {e}", state.serve_session_id);
            }
            return Ok(());
        }

        // Try removing from subprocess sessions. Dropping the Arc triggers cleanup.
        let mut sessions = self.subprocess_sessions.lock().await;
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

    #[tokio::test]
    async fn test_backend_mode_starts_pending() {
        let adapter = MpmSdkAdapter::new();
        let mode = adapter.backend_mode.lock().await;
        assert_eq!(*mode, BackendMode::Pending);
    }

    #[tokio::test]
    async fn test_serve_session_state_mapping() {
        let adapter = MpmSdkAdapter::new();
        let handle_id = "test-handle-1".to_string();
        let serve_id = "srv-session-abc".to_string();

        // Insert a serve session state.
        {
            let mut sessions = adapter.serve_sessions.lock().await;
            sessions.insert(
                handle_id.clone(),
                ServeSessionState {
                    serve_session_id: serve_id.clone(),
                    project_path: "/tmp/project".to_string(),
                },
            );
        }

        // Look up via public API.
        let handle = SessionHandle {
            id: handle_id.clone(),
        };
        let found = adapter.get_serve_session_id(&handle).await;
        assert_eq!(found, Some(serve_id));

        // Unknown handle returns None.
        let unknown = SessionHandle {
            id: "no-such-handle".to_string(),
        };
        assert_eq!(adapter.get_serve_session_id(&unknown).await, None);

        // Remove and verify gone.
        {
            let mut sessions = adapter.serve_sessions.lock().await;
            sessions.remove(&handle_id);
        }
        assert_eq!(adapter.get_serve_session_id(&handle).await, None);
    }
}
