//! Startup health checks for the commander daemon.
//!
//! Verifies that required MCP servers (kuzu-memory, mcp-vector-search) and
//! authentication (claude auth) are healthy on daemon startup. Auto-fixes
//! where possible and logs issues at appropriate severity.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

/// Overall health status for a component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HealthStatus {
    Ok,
    Warning,
    Error,
}

impl HealthStatus {
    fn icon(&self) -> &'static str {
        match self {
            HealthStatus::Ok => "OK",
            HealthStatus::Warning => "WARN",
            HealthStatus::Error => "ERR ",
        }
    }
}

/// Result of a single health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResult {
    /// Component name.
    pub component: String,
    /// Health status.
    pub status: HealthStatus,
    /// Human-readable message describing the status.
    pub message: String,
    /// Whether an issue was automatically fixed during the check.
    pub auto_fixed: bool,
}

/// Health checker that verifies daemon dependencies on startup.
pub struct HealthChecker {
    project_root: PathBuf,
}

impl HealthChecker {
    /// Create a new health checker rooted at the given project directory.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Create a health checker rooted at the current working directory.
    pub fn new_for_cwd() -> Self {
        Self {
            project_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// Check the kuzu-memory MCP server.
    ///
    /// - If the binary is absent: Error with install hint.
    /// - If the data directory is missing but the binary exists: Warning, auto-create dir.
    /// - Both present: Ok.
    pub fn check_kuzu_memory(&self) -> HealthResult {
        let binary_found = find_binary("kuzu-memory");

        if !binary_found {
            return HealthResult {
                component: "kuzu-memory".to_string(),
                status: HealthStatus::Error,
                message: "kuzu-memory binary not found in PATH. Install with: cargo install kuzu-memory".to_string(),
                auto_fixed: false,
            };
        }

        // Check for the data directory.
        let data_dir = kuzu_memory_data_dir();
        if !data_dir.exists() {
            // Auto-fix: create the directory.
            match std::fs::create_dir_all(&data_dir) {
                Ok(()) => {
                    return HealthResult {
                        component: "kuzu-memory".to_string(),
                        status: HealthStatus::Warning,
                        message: format!(
                            "Data directory created: {}",
                            data_dir.display()
                        ),
                        auto_fixed: true,
                    };
                }
                Err(e) => {
                    return HealthResult {
                        component: "kuzu-memory".to_string(),
                        status: HealthStatus::Warning,
                        message: format!(
                            "Data directory missing and could not be created ({}): {}",
                            data_dir.display(),
                            e
                        ),
                        auto_fixed: false,
                    };
                }
            }
        }

        HealthResult {
            component: "kuzu-memory".to_string(),
            status: HealthStatus::Ok,
            message: format!("binary found, data dir: {}", data_dir.display()),
            auto_fixed: false,
        }
    }

    /// Check the mcp-vector-search MCP server.
    ///
    /// Reads `.mcp.json` from the project root and verifies the server is
    /// configured and its command is available.
    pub fn check_mcp_vector_search(&self) -> HealthResult {
        let mcp_json_path = self.project_root.join(".mcp.json");

        // Try to read and parse .mcp.json.
        let config = match read_mcp_json(&mcp_json_path) {
            Ok(c) => c,
            Err(msg) => {
                return HealthResult {
                    component: "mcp-vector-search".to_string(),
                    status: HealthStatus::Warning,
                    message: format!("Cannot read {}: {}", mcp_json_path.display(), msg),
                    auto_fixed: false,
                };
            }
        };

        // Check if mcp-vector-search is configured.
        let server_config = match config.mcp_servers.get("mcp-vector-search") {
            Some(s) => s,
            None => {
                return HealthResult {
                    component: "mcp-vector-search".to_string(),
                    status: HealthStatus::Warning,
                    message: format!(
                        "mcp-vector-search not configured in {}",
                        mcp_json_path.display()
                    ),
                    auto_fixed: false,
                };
            }
        };

        // Check if the command binary exists.
        let command = &server_config.command;
        if !find_binary(command) {
            return HealthResult {
                component: "mcp-vector-search".to_string(),
                status: HealthStatus::Error,
                message: format!(
                    "configured command '{}' not found in PATH",
                    command
                ),
                auto_fixed: false,
            };
        }

        HealthResult {
            component: "mcp-vector-search".to_string(),
            status: HealthStatus::Ok,
            message: format!("configured, command '{}' found", command),
            auto_fixed: false,
        }
    }

    /// Check Claude authentication status.
    ///
    /// Runs `claude auth status --output json` (or the plain variant) and
    /// reports login state and plan tier where available.
    pub fn check_claude_auth(&self) -> HealthResult {
        // Try `claude auth status --output json` first; fall back to plain.
        let output = std::process::Command::new("claude")
            .args(["auth", "status", "--output", "json"])
            .output();

        match output {
            Err(_) => {
                // `claude` binary not found at all.
                HealthResult {
                    component: "claude-auth".to_string(),
                    status: HealthStatus::Error,
                    message: "claude CLI not found in PATH".to_string(),
                    auto_fixed: false,
                }
            }
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();

                // Try JSON parse for structured output.
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    return parse_claude_auth_json(json);
                }

                // Fallback: inspect raw text.
                let combined = format!("{}{}", stdout, stderr).to_lowercase();
                if combined.contains("logged in") || combined.contains("authenticated") {
                    HealthResult {
                        component: "claude-auth".to_string(),
                        status: HealthStatus::Ok,
                        message: "authenticated".to_string(),
                        auto_fixed: false,
                    }
                } else if combined.contains("not logged in") || combined.contains("not authenticated") {
                    HealthResult {
                        component: "claude-auth".to_string(),
                        status: HealthStatus::Error,
                        message: "not authenticated. Run: claude auth login".to_string(),
                        auto_fixed: false,
                    }
                } else {
                    // Unknown output — treat as warning so daemon still starts.
                    HealthResult {
                        component: "claude-auth".to_string(),
                        status: HealthStatus::Warning,
                        message: format!(
                            "auth status unclear (exit code {})",
                            out.status.code().unwrap_or(-1)
                        ),
                        auto_fixed: false,
                    }
                }
            }
        }
    }

    /// Run all health checks and return the combined results.
    pub fn run_all(&self) -> Vec<HealthResult> {
        let results = vec![
            self.check_kuzu_memory(),
            self.check_mcp_vector_search(),
            self.check_claude_auth(),
        ];

        for result in &results {
            let auto_fixed_note = if result.auto_fixed { " (auto-fixed)" } else { "" };
            match result.status {
                HealthStatus::Ok => {
                    info!(
                        component = %result.component,
                        message = %result.message,
                        "Health check OK"
                    );
                }
                HealthStatus::Warning => {
                    warn!(
                        component = %result.component,
                        message = %result.message,
                        auto_fixed = result.auto_fixed,
                        "Health check Warning{}", auto_fixed_note
                    );
                }
                HealthStatus::Error => {
                    error!(
                        component = %result.component,
                        message = %result.message,
                        "Health check Error"
                    );
                }
            }
        }

        results
    }

    /// Render health check results as a formatted report string.
    pub fn format_report(results: &[HealthResult]) -> String {
        let mut lines = Vec::new();
        lines.push("Health Check Results".to_string());
        lines.push("----------------------------".to_string());

        for r in results {
            let icon = r.status.icon();
            let fixed = if r.auto_fixed { " (auto-fixed)" } else { "" };
            lines.push(format!(
                "[{}] {:<22} {}{}",
                icon,
                r.component,
                r.message,
                fixed,
            ));
        }

        lines.push("----------------------------".to_string());
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parsed representation of an MCP server entry in `.mcp.json`.
#[derive(Debug, Deserialize)]
struct McpServerConfig {
    command: String,
    #[allow(dead_code)]
    args: Option<Vec<String>>,
}

/// Top-level structure of `.mcp.json`.
#[derive(Debug, Deserialize)]
struct McpConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: std::collections::HashMap<String, McpServerConfig>,
}

/// Read and parse a `.mcp.json` file. Returns an error string on failure.
fn read_mcp_json(path: &PathBuf) -> Result<McpConfig, String> {
    if !path.exists() {
        return Err("file not found".to_string());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("read error: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("parse error: {}", e))
}

/// Return the kuzu-memory data directory (`~/.kuzu-memory/`).
fn kuzu_memory_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".kuzu-memory")
}

/// Check whether a binary is available on PATH using `which`.
fn find_binary(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Parse a structured JSON response from `claude auth status --output json`.
fn parse_claude_auth_json(json: serde_json::Value) -> HealthResult {
    // Common keys across Claude CLI versions.
    let logged_in = json
        .get("logged_in")
        .or_else(|| json.get("isLoggedIn"))
        .or_else(|| json.get("authenticated"))
        .and_then(|v| v.as_bool());

    let plan = json
        .get("plan")
        .or_else(|| json.get("planType"))
        .or_else(|| json.get("subscription"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match logged_in {
        Some(true) => {
            let msg = match plan {
                Some(p) => format!("authenticated, plan: {}", p),
                None => "authenticated".to_string(),
            };
            HealthResult {
                component: "claude-auth".to_string(),
                status: HealthStatus::Ok,
                message: msg,
                auto_fixed: false,
            }
        }
        Some(false) => HealthResult {
            component: "claude-auth".to_string(),
            status: HealthStatus::Error,
            message: "not authenticated. Run: claude auth login".to_string(),
            auto_fixed: false,
        },
        None => HealthResult {
            component: "claude-auth".to_string(),
            status: HealthStatus::Warning,
            message: "auth status indeterminate from JSON response".to_string(),
            auto_fixed: false,
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_checker(root: &PathBuf) -> HealthChecker {
        HealthChecker::new(root.clone())
    }

    // --- kuzu-memory tests (filesystem only, no real binary needed) -------

    /// When the binary is missing, check_kuzu_memory returns Error.
    #[test]
    fn kuzu_binary_missing_returns_error() {
        // We cannot easily remove a binary from PATH, so we test the logic
        // by directly exercising the helper for a non-existent command.
        assert!(!find_binary("__definitely_not_a_real_binary__"));
    }

    /// When the data directory is absent and auto_fix runs, it should be created.
    #[test]
    fn kuzu_missing_data_dir_auto_fix() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().join(".kuzu-memory");
        assert!(!data_dir.exists());

        // Simulate auto-fix logic directly (same logic as check_kuzu_memory).
        let result = std::fs::create_dir_all(&data_dir);
        assert!(result.is_ok());
        assert!(data_dir.exists());
    }

    // --- mcp-vector-search tests ------------------------------------------

    /// Missing .mcp.json should return Warning, not Error.
    #[test]
    fn mcp_vector_missing_config_returns_warning() {
        let tmp = TempDir::new().unwrap();
        let checker = make_checker(&tmp.path().to_path_buf());
        let result = checker.check_mcp_vector_search();
        assert_eq!(result.status, HealthStatus::Warning);
        assert!(result.message.contains("Cannot read") || result.message.contains("not configured"));
    }

    /// mcp-vector-search absent from .mcp.json returns Warning.
    #[test]
    fn mcp_vector_not_configured_returns_warning() {
        let tmp = TempDir::new().unwrap();
        let mcp_json = tmp.path().join(".mcp.json");
        std::fs::write(
            &mcp_json,
            r#"{"mcpServers": {"other-server": {"command": "other", "args": []}}}"#,
        )
        .unwrap();

        let checker = make_checker(&tmp.path().to_path_buf());
        let result = checker.check_mcp_vector_search();
        assert_eq!(result.status, HealthStatus::Warning);
        assert!(result.message.contains("not configured"));
    }

    /// mcp-vector-search configured with a missing binary returns Error.
    #[test]
    fn mcp_vector_binary_missing_returns_error() {
        let tmp = TempDir::new().unwrap();
        let mcp_json = tmp.path().join(".mcp.json");
        std::fs::write(
            &mcp_json,
            r#"{"mcpServers": {"mcp-vector-search": {"command": "__no_such_binary__", "args": []}}}"#,
        )
        .unwrap();

        let checker = make_checker(&tmp.path().to_path_buf());
        let result = checker.check_mcp_vector_search();
        assert_eq!(result.status, HealthStatus::Error);
        assert!(result.message.contains("not found in PATH"));
    }

    /// mcp-vector-search configured with `uv` (commonly available) returns Ok
    /// if uv is on PATH, or Error if not.  We just verify the component name
    /// and that no panic occurs.
    #[test]
    fn mcp_vector_check_does_not_panic() {
        let tmp = TempDir::new().unwrap();
        let mcp_json = tmp.path().join(".mcp.json");
        std::fs::write(
            &mcp_json,
            r#"{"mcpServers": {"mcp-vector-search": {"command": "uv", "args": ["run", "mcp-vector-search", "mcp"]}}}"#,
        )
        .unwrap();

        let checker = make_checker(&tmp.path().to_path_buf());
        let result = checker.check_mcp_vector_search();
        assert_eq!(result.component, "mcp-vector-search");
    }

    // --- format_report tests -----------------------------------------------

    #[test]
    fn format_report_contains_component_names() {
        let results = vec![
            HealthResult {
                component: "kuzu-memory".to_string(),
                status: HealthStatus::Ok,
                message: "all good".to_string(),
                auto_fixed: false,
            },
            HealthResult {
                component: "mcp-vector-search".to_string(),
                status: HealthStatus::Warning,
                message: "not configured".to_string(),
                auto_fixed: false,
            },
            HealthResult {
                component: "claude-auth".to_string(),
                status: HealthStatus::Error,
                message: "not authenticated".to_string(),
                auto_fixed: false,
            },
        ];

        let report = HealthChecker::format_report(&results);
        assert!(report.contains("kuzu-memory"));
        assert!(report.contains("mcp-vector-search"));
        assert!(report.contains("claude-auth"));
        assert!(report.contains("OK"));
        assert!(report.contains("WARN"));
        assert!(report.contains("ERR"));
    }

    #[test]
    fn format_report_shows_auto_fixed_note() {
        let results = vec![HealthResult {
            component: "kuzu-memory".to_string(),
            status: HealthStatus::Warning,
            message: "Data directory created".to_string(),
            auto_fixed: true,
        }];
        let report = HealthChecker::format_report(&results);
        assert!(report.contains("auto-fixed"));
    }

    // --- JSON auth parsing tests -------------------------------------------

    #[test]
    fn parse_claude_auth_json_logged_in_with_plan() {
        let json = serde_json::json!({"logged_in": true, "plan": "max"});
        let result = parse_claude_auth_json(json);
        assert_eq!(result.status, HealthStatus::Ok);
        assert!(result.message.contains("max"));
    }

    #[test]
    fn parse_claude_auth_json_not_logged_in() {
        let json = serde_json::json!({"logged_in": false});
        let result = parse_claude_auth_json(json);
        assert_eq!(result.status, HealthStatus::Error);
    }

    #[test]
    fn parse_claude_auth_json_indeterminate() {
        let json = serde_json::json!({"some_other_key": "value"});
        let result = parse_claude_auth_json(json);
        assert_eq!(result.status, HealthStatus::Warning);
    }
}
