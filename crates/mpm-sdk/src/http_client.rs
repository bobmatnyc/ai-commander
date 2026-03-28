//! Optional HTTP client for the MPM message injection API.
//!
//! This endpoint is only available when MPM is started with `--sdk --inject-port PORT`.

use crate::types::{AgentResult, MpmError};

/// Full request body for POST /inject.
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct InjectRequest {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
}

#[derive(serde::Deserialize)]
struct InjectResponse {
    text: String,
    session_id: Option<String>,
    cost_usd: Option<f64>,
    duration_ms: Option<u64>,
    is_error: bool,
    num_turns: Option<u32>,
    runtime: Option<String>,
}

impl InjectResponse {
    fn into_result(self) -> AgentResult {
        AgentResult {
            text: self.text,
            session_id: self.session_id,
            cost_usd: self.cost_usd,
            duration_ms: self.duration_ms.unwrap_or(0),
            is_error: self.is_error,
            num_turns: self.num_turns,
            runtime: self.runtime,
        }
    }
}

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
        self.inject_with_options(InjectRequest {
            prompt: prompt.to_string(),
            session_id: session_id.map(str::to_string),
            ..Default::default()
        })
        .await
    }

    /// POST /inject — full request with all optional fields.
    pub async fn inject_with_options(&self, req: InjectRequest) -> Result<AgentResult, MpmError> {
        let url = format!("{}/inject", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&req)
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

        Ok(body.into_result())
    }

    /// GET /status — returns raw JSON status value.
    pub async fn status(&self) -> Result<serde_json::Value, MpmError> {
        self.get_json("/status").await
    }

    /// GET /session — returns current SDK session state.
    pub async fn session(&self) -> Result<serde_json::Value, MpmError> {
        self.get_json("/session").await
    }

    /// GET /activity — returns recent agent events.
    pub async fn activity(&self, limit: Option<u32>) -> Result<serde_json::Value, MpmError> {
        let path = match limit {
            Some(n) => format!("/activity?limit={}", n),
            None => "/activity".to_string(),
        };
        self.get_json(&path).await
    }

    /// GET /history — returns last 50 injected prompts.
    pub async fn history(&self) -> Result<serde_json::Value, MpmError> {
        self.get_json("/history").await
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

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, MpmError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
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

        resp.json()
            .await
            .map_err(|e| MpmError::ParseError(e.to_string()))
    }
}
