//! HTTP client for the `claude-mpm serve` ui_service daemon (port 7777).

use futures_util::StreamExt;
use tokio::sync::mpsc::Sender;

use crate::types::{AgentEvent, AgentResult, CreateSessionRequest, MpmError, ServeSession,
    ServeStreamEvent, SessionContext};

/// HTTP client for the ui_service FastAPI daemon started by `claude-mpm serve start`.
pub struct UiServiceClient {
    base_url: String,
    client: reqwest::Client,
}

impl UiServiceClient {
    /// Create a client targeting localhost on the given port.
    pub fn new(port: u16) -> Self {
        Self::with_host("127.0.0.1", port)
    }

    /// Create a client targeting an arbitrary host and port.
    pub fn with_host(host: &str, port: u16) -> Self {
        Self {
            base_url: format!("http://{}:{}", host, port),
            client: reqwest::Client::new(),
        }
    }

    // --- Health ---

    /// GET /api/v1/health — returns true if the daemon reports healthy.
    pub async fn health(&self) -> Result<bool, MpmError> {
        let url = format!("{}/api/v1/health", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| MpmError::HttpError(e.to_string()))?;
        Ok(resp.status().is_success())
    }

    // --- Sessions ---

    /// GET /api/v1/sessions — list all active sessions.
    pub async fn list_sessions(&self) -> Result<Vec<ServeSession>, MpmError> {
        let url = format!("{}/api/v1/sessions", self.base_url);
        let resp = self.get_ok(&url).await?;
        resp.json::<Vec<ServeSession>>()
            .await
            .map_err(|e| MpmError::ParseError(e.to_string()))
    }

    /// POST /api/v1/sessions — create or resume a session.
    pub async fn create_session(&self, req: CreateSessionRequest) -> Result<ServeSession, MpmError> {
        let url = format!("{}/api/v1/sessions", self.base_url);
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

        resp.json::<ServeSession>()
            .await
            .map_err(|e| MpmError::ParseError(e.to_string()))
    }

    /// GET /api/v1/sessions/{id} — get session state.
    pub async fn get_session(&self, id: &str) -> Result<ServeSession, MpmError> {
        let url = format!("{}/api/v1/sessions/{}", self.base_url, id);
        let resp = self.get_ok(&url).await?;
        resp.json::<ServeSession>()
            .await
            .map_err(|e| MpmError::ParseError(e.to_string()))
    }

    /// DELETE /api/v1/sessions/{id} — terminate session (204).
    pub async fn delete_session(&self, id: &str) -> Result<(), MpmError> {
        let url = format!("{}/api/v1/sessions/{}", self.base_url, id);
        let resp = self
            .client
            .delete(&url)
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
        Ok(())
    }

    // --- Messages ---

    /// POST /api/v1/sessions/{id}/messages — send a message and collect the full response.
    pub async fn send_message(
        &self,
        session_id: &str,
        content: &str,
    ) -> Result<AgentResult, MpmError> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            content: &'a str,
            stream: bool,
        }

        #[derive(serde::Deserialize)]
        struct Resp {
            events: Vec<ServeStreamEvent>,
        }

        let url = format!("{}/api/v1/sessions/{}/messages", self.base_url, session_id);
        let resp = self
            .client
            .post(&url)
            .json(&Req { content, stream: false })
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

        let body: Resp = resp
            .json()
            .await
            .map_err(|e| MpmError::ParseError(e.to_string()))?;

        let mut text = String::new();
        for ev in &body.events {
            if ev.event_type == "text" || ev.event_type == "assistant" {
                if let Some(c) = &ev.content {
                    text.push_str(c);
                }
            }
        }

        Ok(AgentResult {
            text,
            session_id: Some(session_id.to_string()),
            cost_usd: None,
            duration_ms: 0,
            is_error: body.events.iter().any(|e| e.event_type == "error"),
            num_turns: None,
            runtime: None,
        })
    }

    /// POST /api/v1/sessions/{id}/messages with stream=true — SSE streaming.
    ///
    /// Parses `text/event-stream` lines and sends `AgentEvent` values to `tx`.
    /// Accumulates text chunks and sends a final `AgentEvent::Complete` on `message_stop`.
    pub async fn send_message_streaming(
        &self,
        session_id: &str,
        content: &str,
        tx: Sender<AgentEvent>,
    ) -> Result<(), MpmError> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            content: &'a str,
            stream: bool,
        }

        let url = format!("{}/api/v1/sessions/{}/messages", self.base_url, session_id);
        let resp = self
            .client
            .post(&url)
            .json(&Req { content, stream: true })
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

        let mut stream = resp.bytes_stream();
        let mut accumulated = String::new();
        let mut line_buf = String::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| MpmError::HttpError(e.to_string()))?;
            let text = String::from_utf8_lossy(&bytes);

            for ch in text.chars() {
                if ch == '\n' {
                    let line = line_buf.trim().to_string();
                    line_buf.clear();

                    if let Some(json) = line.strip_prefix("data: ") {
                        if let Ok(ev) = serde_json::from_str::<ServeStreamEvent>(json) {
                            match ev.event_type.as_str() {
                                "text" | "assistant" => {
                                    if let Some(c) = &ev.content {
                                        accumulated.push_str(c);
                                        let _ = tx.send(AgentEvent::Text(c.clone())).await;
                                    }
                                }
                                "tool_use" => {
                                    if let Some(name) = &ev.name {
                                        let _ = tx.send(AgentEvent::ToolUse(name.clone())).await;
                                    }
                                }
                                "message_stop" | "result" => {
                                    let result = AgentResult {
                                        text: accumulated.clone(),
                                        session_id: Some(session_id.to_string()),
                                        cost_usd: None,
                                        duration_ms: 0,
                                        is_error: false,
                                        num_turns: None,
                                        runtime: None,
                                    };
                                    let _ = tx.send(AgentEvent::Complete(result)).await;
                                }
                                "error" => {
                                    let msg = ev.content.unwrap_or_default();
                                    let _ = tx.send(AgentEvent::Error(msg)).await;
                                }
                                _ => {}
                            }
                        }
                    }
                } else {
                    line_buf.push(ch);
                }
            }
        }

        Ok(())
    }

    // --- Context ---

    /// GET /api/v1/sessions/{id}/context — token usage.
    pub async fn get_context(&self, session_id: &str) -> Result<SessionContext, MpmError> {
        let url = format!("{}/api/v1/sessions/{}/context", self.base_url, session_id);
        let resp = self.get_ok(&url).await?;
        resp.json::<SessionContext>()
            .await
            .map_err(|e| MpmError::ParseError(e.to_string()))
    }

    // --- Control ---

    /// POST /api/v1/sessions/{id}/interrupt — send SIGINT to subprocess.
    pub async fn interrupt(&self, session_id: &str) -> Result<(), MpmError> {
        let url = format!("{}/api/v1/sessions/{}/interrupt", self.base_url, session_id);
        self.post_empty(&url).await
    }

    /// DELETE /api/v1/sessions/{id}/messages — clear message history.
    pub async fn clear_messages(&self, session_id: &str) -> Result<(), MpmError> {
        let url = format!("{}/api/v1/sessions/{}/messages", self.base_url, session_id);
        let resp = self
            .client
            .delete(&url)
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
        Ok(())
    }

    // --- Helpers ---

    async fn get_ok(&self, url: &str) -> Result<reqwest::Response, MpmError> {
        let resp = self
            .client
            .get(url)
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
        Ok(resp)
    }

    async fn post_empty(&self, url: &str) -> Result<(), MpmError> {
        let resp = self
            .client
            .post(url)
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
        Ok(())
    }
}
