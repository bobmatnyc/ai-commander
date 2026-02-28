//! IPC server implementation for handling client connections.

use std::sync::Arc;

use tokio::net::UnixListener;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::error::{DaemonError, Result};
use crate::service::DaemonServiceHandle;

use super::{IpcConfig, JsonRpcRequest, JsonRpcResponse, JsonRpcError, RpcMethod};

/// IPC server for handling client connections.
pub struct IpcServer {
    /// Server configuration
    config: IpcConfig,
    /// Reference to daemon service handle
    service: Arc<RwLock<DaemonServiceHandle>>,
    /// Server task handle
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl IpcServer {
    /// Create a new IPC server.
    pub fn new(config: IpcConfig, service: Arc<RwLock<DaemonServiceHandle>>) -> Self {
        Self {
            config,
            service,
            server_handle: None,
        }
    }

    /// Start the IPC server.
    pub async fn start(&mut self) -> Result<()> {
        if self.server_handle.is_some() {
            return Err(DaemonError::Configuration("IPC server already running".to_string()));
        }

        // Remove existing socket file if it exists
        if self.config.socket_path.exists() {
            std::fs::remove_file(&self.config.socket_path)
                .map_err(|e| DaemonError::Ipc(format!("Failed to remove existing socket: {}", e)))?;
        }

        // Ensure the parent directory exists
        if let Some(parent) = self.config.socket_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DaemonError::Ipc(format!("Failed to create socket directory: {}", e)))?;
        }

        let listener = UnixListener::bind(&self.config.socket_path)
            .map_err(|e| DaemonError::Ipc(format!("Failed to bind Unix socket: {}", e)))?;

        let config = self.config.clone();
        let service = Arc::clone(&self.service);

        let handle = tokio::spawn(async move {
            info!(socket_path = %config.socket_path.display(), "IPC server started");

            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        debug!(addr = ?addr, "New IPC connection");

                        let service = Arc::clone(&service);
                        let config = config.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, service, config).await {
                                error!(error = %e, "Error handling IPC connection");
                            }
                        });
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to accept IPC connection");
                    }
                }
            }
        });

        self.server_handle = Some(handle);
        Ok(())
    }

    /// Stop the IPC server.
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(handle) = self.server_handle.take() {
            handle.abort();

            // Clean up socket file
            if self.config.socket_path.exists() {
                std::fs::remove_file(&self.config.socket_path)
                    .map_err(|e| DaemonError::Ipc(format!("Failed to remove socket file: {}", e)))?;
            }

            info!("IPC server stopped");
        }
        Ok(())
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        if let Some(handle) = self.server_handle.take() {
            handle.abort();
        }

        // Best effort cleanup
        let _ = std::fs::remove_file(&self.config.socket_path);
    }
}

/// Handle a single IPC connection.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    service: Arc<RwLock<DaemonServiceHandle>>,
    config: IpcConfig,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader_stream, mut writer_stream) = stream.into_split();
    let mut reader = BufReader::new(reader_stream);
    let mut buffer = String::with_capacity(config.buffer_size);

    loop {
        buffer.clear();

        // Read a line (JSON-RPC messages are line-delimited)
        match tokio::time::timeout(
            std::time::Duration::from_millis(config.timeout_ms),
            reader.read_line(&mut buffer)
        ).await {
            Ok(Ok(0)) => {
                // Connection closed
                debug!("IPC connection closed by client");
                break;
            }
            Ok(Ok(_)) => {
                // Process the request
                let response = process_request(&buffer, &service).await;

                // Send response
                let response_json = serde_json::to_string(&response)
                    .map_err(|e| DaemonError::Ipc(format!("Failed to serialize response: {}", e)))?;

                if let Err(e) = writer_stream.write_all(format!("{}\n", response_json).as_bytes()).await {
                    error!(error = %e, "Failed to write response");
                    break;
                }

                if let Err(e) = writer_stream.flush().await {
                    error!(error = %e, "Failed to flush response");
                    break;
                }
            }
            Ok(Err(e)) => {
                error!(error = %e, "Error reading from IPC connection");
                break;
            }
            Err(_) => {
                warn!("IPC connection timed out");
                break;
            }
        }
    }

    Ok(())
}

/// Process a single JSON-RPC request.
async fn process_request(
    request_str: &str,
    service: &Arc<RwLock<DaemonServiceHandle>>,
) -> JsonRpcResponse {
    // Parse JSON-RPC request
    let request: JsonRpcRequest = match serde_json::from_str(request_str.trim()) {
        Ok(req) => req,
        Err(e) => {
            return JsonRpcResponse::error(
                JsonRpcError::parse_error(format!("Invalid JSON: {}", e)),
                None,
            );
        }
    };

    // Validate JSON-RPC version
    if request.jsonrpc != "2.0" {
        return JsonRpcResponse::error(
            JsonRpcError::invalid_request("Invalid JSON-RPC version".to_string()),
            request.id,
        );
    }

    // Parse method
    let method = match RpcMethod::from_str(&request.method) {
        Some(method) => method,
        None => {
            return JsonRpcResponse::error(
                JsonRpcError::method_not_found(request.method),
                request.id,
            );
        }
    };

    // Dispatch request
    let result = dispatch_request(method, &request, service).await;

    // Create response
    match result {
        Ok(value) => JsonRpcResponse::success(value, request.id),
        Err(error) => JsonRpcResponse::error(error, request.id),
    }
}

/// Dispatch a request to the appropriate handler.
async fn dispatch_request(
    method: RpcMethod,
    request: &JsonRpcRequest,
    service: &Arc<RwLock<DaemonServiceHandle>>,
) -> std::result::Result<serde_json::Value, JsonRpcError> {
    match method {
        RpcMethod::SessionCreate => {
            let params: crate::ipc::protocol::SessionCreateParams = request.parse_params()?;
            let service = service.read().await;

            match service.create_session(params.project_path, params.adapter, params.name).await {
                Ok(session_id) => Ok(serde_json::json!({ "session_id": session_id })),
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::SessionList => {
            let service = service.read().await;
            match service.list_sessions().await {
                Ok(sessions) => {
                    let response = crate::ipc::protocol::SessionListResponse { sessions };
                    Ok(serde_json::to_value(response).unwrap())
                }
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::SessionGet => {
            #[derive(serde::Deserialize)]
            struct Params {
                session_id: String,
            }

            let params: Params = request.parse_params()?;
            let service = service.read().await;

            match service.get_session(&params.session_id).await {
                Ok(Some(session)) => Ok(serde_json::to_value(session).unwrap()),
                Ok(None) => Err(JsonRpcError::application_error(
                    404,
                    "Session not found".to_string(),
                    None,
                )),
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::SessionTerminate => {
            #[derive(serde::Deserialize)]
            struct Params {
                session_id: String,
            }

            let params: Params = request.parse_params()?;
            let service = service.write().await;

            match service.terminate_session(&params.session_id).await {
                Ok(_) => Ok(serde_json::json!({ "success": true })),
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::SessionSend => {
            let params: crate::ipc::protocol::SessionSendParams = request.parse_params()?;
            let service = service.read().await;

            match service.send_to_session(&params.session_id, &params.message).await {
                Ok(response) => Ok(serde_json::json!({ "response": response })),
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::PairingGenerate => {
            let params: crate::ipc::protocol::PairingGenerateParams = request.parse_params()?;
            let service = service.write().await;

            match service.generate_pairing_code(params.session_id, params.project_path).await {
                Ok((code, expires_at)) => {
                    let response = crate::ipc::protocol::PairingGenerateResponse { code, expires_at };
                    Ok(serde_json::to_value(response).unwrap())
                }
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::PairingValidate => {
            let params: crate::ipc::protocol::PairingValidateParams = request.parse_params()?;
            let service = service.write().await;

            match service.validate_pairing_code(&params.code, params.client_info).await {
                Ok(Some(entry)) => Ok(serde_json::to_value(entry).unwrap()),
                Ok(None) => Err(JsonRpcError::application_error(
                    404,
                    "Invalid or expired pairing code".to_string(),
                    None,
                )),
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::StatusHealth => {
            let service = service.read().await;
            match service.get_health_status().await {
                Ok(status) => Ok(serde_json::to_value(status).unwrap()),
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::StatusMemory => {
            let service = service.read().await;
            match service.get_memory_status().await {
                Ok(status) => Ok(serde_json::to_value(status).unwrap()),
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::DaemonStop => {
            let service = service.write().await;
            match service.shutdown().await {
                Ok(_) => Ok(serde_json::json!({ "success": true, "message": "Daemon stopping" })),
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }

        RpcMethod::DaemonRestart => {
            let service = service.write().await;
            match service.restart().await {
                Ok(_) => Ok(serde_json::json!({ "success": true, "message": "Daemon restarting" })),
                Err(e) => Err(JsonRpcError::internal_error(e.to_string())),
            }
        }
    }
}
