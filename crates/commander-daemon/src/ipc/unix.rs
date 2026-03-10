//! Unix-specific IPC implementation using domain sockets.

#[cfg(unix)]
pub mod unix_socket {
    use std::path::Path;
    use tokio::net::UnixStream;
    use crate::error::{DaemonError, Result};

    /// Connect to Unix domain socket.
    pub async fn connect<P: AsRef<Path>>(socket_path: P) -> Result<UnixStream> {
        UnixStream::connect(socket_path)
            .await
            .map_err(|e| DaemonError::Ipc(format!("Failed to connect to Unix socket: {}", e)))
    }

    /// Check if socket file exists and is accessible.
    pub fn socket_exists<P: AsRef<Path>>(socket_path: P) -> bool {
        socket_path.as_ref().exists()
    }
}
