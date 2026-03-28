//! `mpm-sdk` — headless SDK for spawning and managing Claude MPM agents.
//!
//! Provides a process-based client (`MpmClient`) that spawns `claude-mpm run --headless`
//! as a child process and parses the NDJSON stream-json output. An optional HTTP client
//! (`MpmHttpClient`) is available for use with `--sdk --inject-port` mode.

pub mod client;
pub mod http_client;
pub mod parser;
pub mod serve_client;
pub mod serve_manager;
pub mod types;

pub use client::MpmClient;
pub use http_client::{InjectRequest, MpmHttpClient};
pub use serve_client::UiServiceClient;
pub use serve_manager::ServeManager;
pub use types::{
    AgentEvent, AgentInfo, AgentResult, AgentTask, CreateSessionRequest, MpmError, MpmStatus,
    ServeSession, ServeStreamEvent, SessionContext,
};
