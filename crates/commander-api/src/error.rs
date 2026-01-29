//! API error types.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Result type for API operations.
pub type Result<T> = std::result::Result<T, ApiError>;

/// API error type for consistent error responses.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Resource not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Bad request - invalid input.
    #[error("bad request: {0}")]
    BadRequest(String),

    /// Internal server error.
    #[error("internal error: {0}")]
    Internal(String),

    /// Conflict - resource already exists.
    #[error("conflict: {0}")]
    Conflict(String),

    /// Service unavailable.
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
}

impl ApiError {
    /// Returns the HTTP status code for this error.
    pub fn status_code(&self) -> StatusCode {
        match self {
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(json!({
            "error": self.to_string()
        }));
        (status, body).into_response()
    }
}

impl From<commander_runtime::RuntimeError> for ApiError {
    fn from(err: commander_runtime::RuntimeError) -> Self {
        match err {
            commander_runtime::RuntimeError::InstanceNotFound(id) => {
                ApiError::NotFound(format!("instance not found: {}", id))
            }
            commander_runtime::RuntimeError::InstanceExists(id) => {
                ApiError::Conflict(format!("instance already exists: {}", id))
            }
            commander_runtime::RuntimeError::MaxInstancesReached(max) => {
                ApiError::ServiceUnavailable(format!("max instances reached: {}", max))
            }
            _ => ApiError::Internal(err.to_string()),
        }
    }
}

impl From<commander_events::EventError> for ApiError {
    fn from(err: commander_events::EventError) -> Self {
        match err {
            commander_events::EventError::NotFound(id) => {
                ApiError::NotFound(format!("event not found: {}", id))
            }
            commander_events::EventError::InvalidState(msg) => {
                ApiError::BadRequest(format!("invalid state: {}", msg))
            }
            _ => ApiError::Internal(err.to_string()),
        }
    }
}

impl From<commander_work::WorkError> for ApiError {
    fn from(err: commander_work::WorkError) -> Self {
        match err {
            commander_work::WorkError::NotFound(id) => {
                ApiError::NotFound(format!("work item not found: {}", id))
            }
            commander_work::WorkError::InvalidState(msg) => {
                ApiError::BadRequest(format!("invalid state: {}", msg))
            }
            _ => ApiError::Internal(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_status_codes() {
        assert_eq!(
            ApiError::NotFound("test".into()).status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            ApiError::BadRequest("test".into()).status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ApiError::Internal("test".into()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            ApiError::Conflict("test".into()).status_code(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            ApiError::ServiceUnavailable("test".into()).status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn test_api_error_display() {
        let err = ApiError::NotFound("project-1".into());
        assert_eq!(err.to_string(), "not found: project-1");
    }
}
