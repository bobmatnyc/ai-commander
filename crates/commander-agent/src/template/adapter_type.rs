//! Adapter type enumeration for agent templates.

use serde::{Deserialize, Serialize};

use crate::error::{AgentError, Result};

/// Type of adapter that the agent is managing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AdapterType {
    /// Claude Code AI coding assistant.
    ClaudeCode,
    /// MPM multi-agent orchestration.
    Mpm,
    /// Generic terminal/shell session.
    Generic,
}

impl std::fmt::Display for AdapterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClaudeCode => write!(f, "claude_code"),
            Self::Mpm => write!(f, "mpm"),
            Self::Generic => write!(f, "generic"),
        }
    }
}

impl std::str::FromStr for AdapterType {
    type Err = AgentError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "claude_code" | "claudecode" | "claude-code" => Ok(Self::ClaudeCode),
            "mpm" => Ok(Self::Mpm),
            "generic" | "shell" => Ok(Self::Generic),
            _ => Err(AgentError::Configuration(format!(
                "unknown adapter type: {}",
                s
            ))),
        }
    }
}
