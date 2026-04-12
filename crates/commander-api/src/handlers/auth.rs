//! Authentication handlers for web client pairing.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};

use commander_daemon::PairingManager;

use crate::error::{ApiError, Result};
use crate::state::AppState;
use crate::types::{AuthStatusResponse, GenerateCodeResponse, PairRequest, PairResponse};

/// POST /api/auth/pair — validate a pairing code and return a session token.
pub async fn pair(
    State(state): State<AppState>,
    Json(req): Json<PairRequest>,
) -> Result<(StatusCode, Json<PairResponse>)> {
    if req.code.is_empty() {
        return Err(ApiError::BadRequest("code must not be empty".to_string()));
    }

    let code = req.code.trim().to_uppercase();

    // Validate the pairing code via PairingManager.
    // PairingManager uses a file-backed store; it is cheap to instantiate.
    let mut manager = PairingManager::new()
        .map_err(|e| ApiError::Internal(format!("pairing manager unavailable: {}", e)))?;

    let entry = manager
        .validate_code(&code, req.client_info.clone())
        .map_err(|e| ApiError::Internal(format!("pairing validation error: {}", e)))?;

    if entry.is_none() {
        return Err(ApiError::BadRequest(
            "invalid or expired pairing code".to_string(),
        ));
    }

    // Create a session token in the web client store.
    let client = state
        .web_clients
        .create_client(req.client_info)
        .map_err(|e| ApiError::Internal(format!("failed to create web client: {}", e)))?;

    Ok((
        StatusCode::CREATED,
        Json(PairResponse {
            token: client.token,
            paired_at: client.paired_at,
        }),
    ))
}

/// GET /api/auth/status — check whether a bearer token is valid.
pub async fn auth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<AuthStatusResponse> {
    let token = extract_bearer_token(&headers);

    match token.and_then(|t| state.web_clients.validate_token(t)) {
        Some(client) => {
            state.web_clients.update_last_seen(&client.token);
            Json(AuthStatusResponse {
                authenticated: true,
                paired_at: Some(client.paired_at),
                last_seen: Some(client.last_seen),
            })
        }
        None => Json(AuthStatusResponse {
            authenticated: false,
            paired_at: None,
            last_seen: None,
        }),
    }
}

/// POST /api/auth/generate-code — generate a new pairing code (GUI-facing).
pub async fn generate_code(State(_state): State<AppState>) -> Result<Json<GenerateCodeResponse>> {
    let mut manager = PairingManager::new()
        .map_err(|e| ApiError::Internal(format!("pairing manager unavailable: {}", e)))?;

    // Generate a code with no session or project binding — the web client
    // just needs authorisation, not automatic session attachment.
    let code = manager
        .generate_code(None, None)
        .map_err(|e| ApiError::Internal(format!("failed to generate pairing code: {}", e)))?;

    // Retrieve the entry to get the exact expiry timestamp.
    let entry = manager
        .get_entry(&code)
        .ok_or_else(|| ApiError::Internal("generated code not found in store".to_string()))?;

    let now = chrono::Utc::now();
    let expires_in_seconds = (entry.expires_at - now).num_seconds().max(0);

    Ok(Json(GenerateCodeResponse {
        code,
        expires_at: entry.expires_at,
        expires_in_seconds,
    }))
}

/// Extract the bearer token from `Authorization: Bearer <token>`.
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::AUTHORIZATION;
    use axum::http::HeaderValue;

    #[test]
    fn test_extract_bearer_token_present() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer my-secret-token"),
        );
        assert_eq!(extract_bearer_token(&headers), Some("my-secret-token"));
    }

    #[test]
    fn test_extract_bearer_token_missing() {
        let headers = HeaderMap::new();
        assert_eq!(extract_bearer_token(&headers), None);
    }

    #[test]
    fn test_extract_bearer_token_wrong_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Basic dXNlcjpwYXNz"),
        );
        assert_eq!(extract_bearer_token(&headers), None);
    }
}
