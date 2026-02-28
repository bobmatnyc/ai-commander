//! Inter-process communication layer for daemon-client communication.
//!
//! This module provides a JSON-RPC protocol over Unix domain sockets (Unix)
//! or named pipes (Windows) for structured communication between the daemon
//! and its clients.

use serde::{Deserialize, Serialize};

pub mod protocol;
pub mod server;

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
pub mod windows;

// Re-export main types
pub use protocol::{JsonRpcRequest, JsonRpcResponse, JsonRpcError, RpcMethod};
pub use server::IpcServer;

/// IPC configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcConfig {
    /// Socket file path (Unix) or pipe name (Windows)
    pub socket_path: std::path::PathBuf,
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Request timeout in milliseconds
    pub timeout_ms: u64,
    /// Buffer size for reading/writing
    pub buffer_size: usize,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            socket_path: commander_core::config::runtime_state_dir().join("daemon.sock"),
            max_connections: 50,
            timeout_ms: 30000, // 30 seconds
            buffer_size: 64 * 1024, // 64KB
        }
    }
}
