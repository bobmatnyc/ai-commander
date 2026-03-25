//! Self-contained IPC client for communicating with the commander-daemon.
//!
//! Uses a minimal JSON-RPC 2.0 implementation over Unix domain sockets without
//! depending on the commander-daemon crate (which pulls in heavy transitive deps).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::debug;

use crate::error::{Result, TelegramError};

// ---------------------------------------------------------------------------
// Minimal JSON-RPC 2.0 types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    method: String,
    params: Value,
    id: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    result: Option<Value>,
    error: Option<JsonRpcErrorObj>,
    #[allow(dead_code)]
    id: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcErrorObj {
    #[allow(dead_code)]
    code: i32,
    message: String,
}

// ---------------------------------------------------------------------------
// Session info returned by session.list
// ---------------------------------------------------------------------------

/// Minimal session info as returned by the daemon's `session.list` method.
#[derive(Debug, Clone, Deserialize)]
pub struct DaemonSessionInfo {
    pub id: String,
    pub name: Option<String>,
    pub adapter: String,
    pub status: String,
}

// ---------------------------------------------------------------------------
// DaemonClient
// ---------------------------------------------------------------------------

/// Lightweight IPC client for the commander-daemon Unix socket.
///
/// Each method opens a fresh connection, sends one request, reads the response,
/// and closes the connection. This is intentionally simple — no connection
/// pooling required for the low-frequency Telegram use case.
pub struct DaemonClient {
    socket_path: PathBuf,
    next_id: std::sync::atomic::AtomicU64,
}

impl DaemonClient {
    /// Create a client pointing at the daemon socket.
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Create a client using the default daemon socket path.
    pub fn default_path() -> Self {
        let path = commander_core::config::runtime_state_dir().join("daemon.sock");
        Self::new(path)
    }

    /// Returns `true` if the daemon socket file exists on disk.
    pub fn is_daemon_running(&self) -> bool {
        self.socket_path.exists()
    }

    /// Send a raw JSON-RPC call and return the `result` value on success.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
            id,
        };

        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| TelegramError::IoError(e))?;

        // Write request line
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        stream
            .write_all(line.as_bytes())
            .await
            .map_err(|e| TelegramError::IoError(e))?;
        stream.flush().await.map_err(|e| TelegramError::IoError(e))?;

        // Read response line
        let (reader_half, _writer_half) = stream.into_split();
        let mut buf_reader = BufReader::new(reader_half);
        let mut response_line = String::new();
        buf_reader
            .read_line(&mut response_line)
            .await
            .map_err(|e| TelegramError::IoError(e))?;

        debug!(method = %method, response = %response_line.trim(), "IPC response received");

        if response_line.trim().is_empty() {
            return Err(TelegramError::SessionError(
                "Daemon closed connection without responding (possible daemon crash or panic)"
                    .to_string(),
            ));
        }
        let response: JsonRpcResponse = serde_json::from_str(response_line.trim())?;

        if let Some(err) = response.error {
            return Err(TelegramError::SessionError(format!(
                "Daemon RPC error ({}): {}",
                method, err.message
            )));
        }

        response.result.ok_or_else(|| {
            TelegramError::SessionError(format!(
                "Daemon returned empty result for method: {}",
                method
            ))
        })
    }

    /// Send a message to a daemon session.
    ///
    /// Maps to: `session.send` with `{ session_id, message }`.
    pub async fn session_send(&self, session_id: &str, message: &str) -> Result<()> {
        let params = serde_json::json!({
            "session_id": session_id,
            "message": message,
        });
        self.call("session.send", params).await?;
        Ok(())
    }

    /// List all sessions known to the daemon.
    ///
    /// Maps to: `session.list` — returns `{ sessions: [...] }`.
    pub async fn session_list(&self) -> Result<Vec<DaemonSessionInfo>> {
        let result = self.call("session.list", serde_json::json!({})).await?;
        let sessions = result
            .get("sessions")
            .and_then(|v| serde_json::from_value::<Vec<DaemonSessionInfo>>(v.clone()).ok())
            .unwrap_or_default();
        Ok(sessions)
    }

    /// Create a new session in the daemon.
    ///
    /// Maps to: `session.create` — returns the new session ID.
    pub async fn session_create(
        &self,
        project_path: Option<&str>,
        name: Option<&str>,
    ) -> Result<String> {
        let params = serde_json::json!({
            "project_path": project_path,
            "adapter": null,
            "name": name,
        });
        let result = self.call("session.create", params).await?;
        result
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                TelegramError::SessionError(
                    "Daemon session.create response missing session_id".to_string(),
                )
            })
    }

    /// Check daemon health. Returns `true` if the daemon responds as healthy.
    pub async fn health_check(&self) -> Result<bool> {
        let result = self.call("status.health", serde_json::json!({})).await?;
        let ok = result
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| s == "healthy")
            .unwrap_or(false);
        Ok(ok)
    }
}
