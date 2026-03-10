//! JSON-RPC protocol implementation for IPC communication.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request method name
    pub method: String,
    /// Request parameters
    pub params: Option<Value>,
    /// Request ID (for matching responses)
    pub id: Option<Value>,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request.
    pub fn new(method: String, params: Option<Value>, id: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
            id,
        }
    }

    /// Create a notification (request without ID).
    pub fn notification(method: String, params: Option<Value>) -> Self {
        Self::new(method, params, None)
    }

    /// Parse parameters as a specific type.
    pub fn parse_params<T: for<'de> Deserialize<'de>>(&self) -> Result<T, JsonRpcError> {
        match &self.params {
            Some(params) => serde_json::from_value(params.clone())
                .map_err(|e| JsonRpcError::invalid_params(format!("Invalid parameters: {}", e))),
            None => serde_json::from_value(Value::Null)
                .map_err(|e| JsonRpcError::invalid_params(format!("Missing parameters: {}", e))),
        }
    }
}

/// JSON-RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Response result (success case)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Response error (error case)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// Request ID (matches original request)
    pub id: Option<Value>,
}

impl JsonRpcResponse {
    /// Create a successful response.
    pub fn success(result: Value, id: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response.
    pub fn error(error: JsonRpcError, id: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }
}

/// JSON-RPC error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    /// Parse error (-32700)
    pub fn parse_error(message: String) -> Self {
        Self {
            code: -32700,
            message,
            data: None,
        }
    }

    /// Invalid request (-32600)
    pub fn invalid_request(message: String) -> Self {
        Self {
            code: -32600,
            message,
            data: None,
        }
    }

    /// Method not found (-32601)
    pub fn method_not_found(method: String) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {}", method),
            data: None,
        }
    }

    /// Invalid params (-32602)
    pub fn invalid_params(message: String) -> Self {
        Self {
            code: -32602,
            message,
            data: None,
        }
    }

    /// Internal error (-32603)
    pub fn internal_error(message: String) -> Self {
        Self {
            code: -32603,
            message,
            data: None,
        }
    }

    /// Application error (custom code)
    pub fn application_error(code: i32, message: String, data: Option<Value>) -> Self {
        Self {
            code,
            message,
            data,
        }
    }
}

/// RPC method definitions.
pub enum RpcMethod {
    // Session management
    SessionCreate,
    SessionList,
    SessionGet,
    SessionTerminate,
    SessionSend,

    // Pairing
    PairingGenerate,
    PairingValidate,

    // Status and monitoring
    StatusHealth,
    StatusMemory,

    // Daemon control
    DaemonStop,
    DaemonRestart,
}

impl RpcMethod {
    /// Parse method from string.
    pub fn from_str(method: &str) -> Option<Self> {
        match method {
            "session.create" => Some(Self::SessionCreate),
            "session.list" => Some(Self::SessionList),
            "session.get" => Some(Self::SessionGet),
            "session.terminate" => Some(Self::SessionTerminate),
            "session.send" => Some(Self::SessionSend),
            "pairing.generate" => Some(Self::PairingGenerate),
            "pairing.validate" => Some(Self::PairingValidate),
            "status.health" => Some(Self::StatusHealth),
            "status.memory" => Some(Self::StatusMemory),
            "daemon.stop" => Some(Self::DaemonStop),
            "daemon.restart" => Some(Self::DaemonRestart),
            _ => None,
        }
    }

    /// Convert to string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionCreate => "session.create",
            Self::SessionList => "session.list",
            Self::SessionGet => "session.get",
            Self::SessionTerminate => "session.terminate",
            Self::SessionSend => "session.send",
            Self::PairingGenerate => "pairing.generate",
            Self::PairingValidate => "pairing.validate",
            Self::StatusHealth => "status.health",
            Self::StatusMemory => "status.memory",
            Self::DaemonStop => "daemon.stop",
            Self::DaemonRestart => "daemon.restart",
        }
    }
}

/// Session creation parameters.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionCreateParams {
    pub project_path: Option<PathBuf>,
    pub adapter: Option<String>,
    pub name: Option<String>,
}

/// Session list response.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionInfo>,
}

/// Session information.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: Option<String>,
    pub adapter: String,
    pub project_path: Option<PathBuf>,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
    pub memory_usage: Option<crate::monitoring::MemoryUsage>,
}

/// Session message parameters.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSendParams {
    pub session_id: String,
    pub message: String,
}

/// Pairing generation parameters.
#[derive(Debug, Serialize, Deserialize)]
pub struct PairingGenerateParams {
    pub session_id: Option<String>,
    pub project_path: Option<PathBuf>,
}

/// Pairing generation response.
#[derive(Debug, Serialize, Deserialize)]
pub struct PairingGenerateResponse {
    pub code: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Pairing validation parameters.
#[derive(Debug, Serialize, Deserialize)]
pub struct PairingValidateParams {
    pub code: String,
    pub client_info: Option<String>,
}

/// Health status response.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthStatusResponse {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub active_sessions: usize,
    pub memory_usage: HashMap<String, crate::monitoring::MemoryUsage>,
    pub system_info: SystemInfo,
}

/// System information.
#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub architecture: String,
    pub total_memory_mb: u64,
    pub available_memory_mb: u64,
}

/// Memory status response.
#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryStatusResponse {
    pub total_usage_mb: u64,
    pub max_memory_mb: u64,
    pub usage_percentage: f32,
    pub sessions: HashMap<String, crate::monitoring::SessionMemoryInfo>,
    pub cleanup_triggered: bool,
}
