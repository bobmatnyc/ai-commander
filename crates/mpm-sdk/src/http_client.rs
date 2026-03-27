//! Optional HTTP client for the MPM message injection API.
//!
//! This endpoint is only available when MPM is started with `--sdk --inject-port PORT`.

use crate::types::{AgentResult, MpmError};

/// HTTP client for the MPM inject API (e.g. `http://127.0.0.1:7856`).
pub struct MpmHttpClient {
    base_url: String,
    client: reqwest::Client,
}

impl MpmHttpClient {
    /// Create a new client targeting the given port on localhost.
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{}", port),
            client: reqwest::Client::new(),
        }
    }

    /// POST /inject — execute a prompt and return the result (blocking).
    pub async fn inject(
        &self,
        prompt: &str,
        session_id: Option<&str>,
    ) -> Result<AgentResult, MpmError> {
        #[derive(serde::Serialize)]
        struct InjectRequest<'a> {
            prompt: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            session_id: Option<&'a str>,
        }

        #[derive(serde::Deserialize)]
        struct InjectResponse {
            text: String,
            session_id: Option<String>,
            cost_usd: Option<f64>,
            duration_ms: Option<u64>,
            is_error: bool,
        }

        let url = format!("{}/inject", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&InjectRequest { prompt, session_id })
            .send()
            .await
            .map_err(|e| MpmError::HttpError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(MpmError::HttpError(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }

        let body: InjectResponse = resp
            .json()
            .await
            .map_err(|e| MpmError::ParseError(e.to_string()))?;

        Ok(AgentResult {
            text: body.text,
            session_id: body.session_id,
            cost_usd: body.cost_usd,
            duration_ms: body.duration_ms.unwrap_or(0),
            is_error: body.is_error,
        })
    }

    /// GET /status — returns raw JSON status value.
    pub async fn status(&self) -> Result<serde_json::Value, MpmError> {
        let url = format!("{}/status", self.base_url);
        self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| MpmError::HttpError(e.to_string()))?
            .json()
            .await
            .map_err(|e| MpmError::ParseError(e.to_string()))
    }

    /// Returns true if the inject API is reachable (GET /status returns 200).
    pub async fn is_ready(&self) -> bool {
        let url = format!("{}/status", self.base_url);
        self.client
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}
