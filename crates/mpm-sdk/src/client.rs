//! MpmClient — spawns `claude-mpm` as a subprocess and collects results.

use std::path::PathBuf;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::parser::{extract_session_id, is_completion_line, parse_ndjson_line};
use crate::types::{AgentEvent, AgentInfo, AgentResult, MpmError, MpmStatus};

/// Default timeout for agent runs (5 minutes).
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Client that runs `claude-mpm` as a child process.
pub struct MpmClient {
    /// Path to the `claude-mpm` binary.
    binary: String,
    /// Working directory for spawned processes.
    cwd: PathBuf,
    /// Timeout for agent runs in seconds.
    timeout_secs: u64,
    /// Session ID from the last run, used for `--resume`.
    last_session_id: Option<String>,
}

impl MpmClient {
    /// Create a new client with explicit binary path and working directory.
    pub fn new(binary: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            binary: binary.into(),
            cwd,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            last_session_id: None,
        }
    }

    /// Discover the `claude-mpm` binary via `which` and use the current directory.
    pub fn discover() -> Result<Self, MpmError> {
        // Try `which claude-mpm` via std::process so we can return synchronously.
        let output = std::process::Command::new("which")
            .arg("claude-mpm")
            .output()
            .map_err(|e| MpmError::BinaryNotFound(e.to_string()))?;

        if !output.status.success() {
            return Err(MpmError::BinaryNotFound(
                "claude-mpm not found in PATH".to_string(),
            ));
        }

        let binary = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if binary.is_empty() {
            return Err(MpmError::BinaryNotFound(
                "which returned empty path".to_string(),
            ));
        }

        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/tmp"));

        Ok(Self::new(binary, cwd))
    }

    /// Set the timeout for agent runs.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Return the session ID from the last completed run.
    pub fn last_session_id(&self) -> Option<&str> {
        self.last_session_id.as_deref()
    }

    /// Get MPM system status: version, binary path, agent count, health.
    pub async fn status(&self) -> Result<MpmStatus, MpmError> {
        // Run `claude-mpm --version`
        let output = Command::new(&self.binary)
            .arg("--version")
            .current_dir(&self.cwd)
            .output()
            .await?;

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let healthy = output.status.success();

        // Count agents from the cache directory (best-effort).
        let agent_count = count_cached_agents();

        Ok(MpmStatus {
            version,
            binary_path: self.binary.clone(),
            agent_count,
            healthy,
        })
    }

    /// List available agents by running `claude-mpm agents list --json`.
    /// Falls back to text parsing and cache directory listing if JSON is unavailable.
    pub async fn list_agents(&self) -> Result<Vec<AgentInfo>, MpmError> {
        // Try JSON output first.
        if let Ok(agents) = self.list_agents_json().await {
            return Ok(agents);
        }
        // Fall back to text parsing.
        if let Ok(agents) = self.list_agents_text().await {
            return Ok(agents);
        }
        // Last resort: read cache directory.
        Ok(list_agents_from_cache())
    }

    async fn list_agents_json(&self) -> Result<Vec<AgentInfo>, MpmError> {
        let output = Command::new(&self.binary)
            .args(["agents", "list", "--json"])
            .current_dir(&self.cwd)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let agents: Vec<AgentInfo> = serde_json::from_str(&stdout)
            .map_err(|e| MpmError::ParseError(e.to_string()))?;
        Ok(agents)
    }

    async fn list_agents_text(&self) -> Result<Vec<AgentInfo>, MpmError> {
        let output = Command::new(&self.binary)
            .args(["agents", "list"])
            .current_dir(&self.cwd)
            .output()
            .await?;

        if !output.status.success() {
            return Err(MpmError::AgentError("agents list failed".to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let agents = stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                // Each line may be "id - description" or just "id".
                let (id, description) = if let Some((a, b)) = line.split_once(" - ") {
                    (a.trim().to_string(), b.trim().to_string())
                } else {
                    let id = line.trim().to_string();
                    (id.clone(), id)
                };
                AgentInfo {
                    name: id.clone(),
                    id,
                    description,
                }
            })
            .collect();

        Ok(agents)
    }

    /// Run an agent synchronously and return the full result.
    pub async fn run(
        &mut self,
        agent_id: &str,
        prompt: &str,
    ) -> Result<AgentResult, MpmError> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let timeout = Duration::from_secs(self.timeout_secs);
        let run_fut = self.run_streaming_inner(agent_id, prompt, tx);
        tokio::time::timeout(timeout, run_fut)
            .await
            .map_err(|_| MpmError::Timeout(self.timeout_secs))??;

        // Drain the channel looking for the Complete event.
        let mut result: Option<AgentResult> = None;
        while let Ok(event) = rx.try_recv() {
            if let AgentEvent::Complete(r) = event {
                result = Some(r);
            }
        }

        result.ok_or_else(|| MpmError::AgentError("no result received".to_string()))
    }

    /// Run an agent and stream events to the provided channel.
    pub async fn run_streaming(
        &mut self,
        agent_id: &str,
        prompt: &str,
        tx: tokio::sync::mpsc::Sender<AgentEvent>,
    ) -> Result<(), MpmError> {
        let timeout = Duration::from_secs(self.timeout_secs);
        tokio::time::timeout(timeout, self.run_streaming_inner(agent_id, prompt, tx))
            .await
            .map_err(|_| MpmError::Timeout(self.timeout_secs))?
    }

    async fn run_streaming_inner(
        &mut self,
        agent_id: &str,
        prompt: &str,
        tx: tokio::sync::mpsc::Sender<AgentEvent>,
    ) -> Result<(), MpmError> {
        let mut args = vec![
            "run".to_string(),
            "--headless".to_string(),
            "--non-interactive".to_string(),
            "--no-check-dependencies".to_string(),
            "--no-prompt".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
        ];

        // Optionally target a specific agent via --agent flag (if supported).
        // We append the agent_id into the prompt if not the default agent.
        let effective_prompt = if agent_id == "default" || agent_id.is_empty() {
            prompt.to_string()
        } else {
            // Prefix with agent specifier. MPM interprets @agent_id syntax.
            format!("@{} {}", agent_id, prompt)
        };

        if let Some(ref sid) = self.last_session_id.clone() {
            args.push("--resume".to_string());
            args.push(sid.clone());
        }

        args.push("-i".to_string());
        args.push(effective_prompt);

        debug!(binary = %self.binary, ?args, "Spawning claude-mpm");

        let mut child = Command::new(&self.binary)
            .args(&args)
            .current_dir(&self.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MpmError::AgentError("no stdout".to_string()))?;

        let mut reader = BufReader::new(stdout).lines();

        while let Some(line) = reader.next_line().await? {
            // Check for session ID in init line.
            if let Some(sid) = extract_session_id(&line) {
                self.last_session_id = Some(sid);
            }

            if let Some(event) = parse_ndjson_line(&line) {
                let is_done = matches!(event, AgentEvent::Complete(_));
                if tx.send(event).await.is_err() {
                    warn!("Receiver dropped, stopping stream");
                    break;
                }
                if is_done {
                    break;
                }
            } else if is_completion_line(&line) {
                // Completion line that didn't parse into an event — send empty result.
                let _ = tx
                    .send(AgentEvent::Complete(AgentResult {
                        text: String::new(),
                        session_id: self.last_session_id.clone(),
                        cost_usd: None,
                        duration_ms: 0,
                        is_error: false,
                    }))
                    .await;
                break;
            }
        }

        // Wait for process exit (best-effort).
        let _ = child.wait().await;
        Ok(())
    }
}

/// Count agents in `~/.claude-mpm/cache/agents/` (best-effort, returns 0 on error).
fn count_cached_agents() -> usize {
    let Some(home) = dirs::home_dir() else {
        return 0;
    };
    let cache_dir = home.join(".claude-mpm").join("cache").join("agents");
    std::fs::read_dir(cache_dir)
        .map(|rd| rd.count())
        .unwrap_or(0)
}

/// Read agent names from `~/.claude-mpm/cache/agents/` directory.
fn list_agents_from_cache() -> Vec<AgentInfo> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let cache_dir = home.join(".claude-mpm").join("cache").join("agents");
    let Ok(rd) = std::fs::read_dir(cache_dir) else {
        return Vec::new();
    };
    rd.filter_map(|entry| {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().to_string();
        // Strip extension if present.
        let id = name
            .strip_suffix(".json")
            .or_else(|| name.strip_suffix(".toml"))
            .unwrap_or(&name)
            .to_string();
        Some(AgentInfo {
            name: id.clone(),
            id,
            description: String::new(),
        })
    })
    .collect()
}
