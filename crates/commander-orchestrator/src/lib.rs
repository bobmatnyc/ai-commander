//! Agent orchestration layer for AI Commander multi-agent system.
//!
//! This crate provides the `AgentOrchestrator` which coordinates the User Agent
//! and Session Agents, providing a simple API for UI layers (TUI, REPL, Telegram).
//!
//! # Overview
//!
//! The orchestrator manages:
//! - A single User Agent for processing user input
//! - Multiple Session Agents (one per tmux/VS Code session)
//! - Shared memory store for agent memories
//! - Auto-eval for feedback tracking
//!
//! # Example
//!
//! ```ignore
//! use commander_orchestrator::AgentOrchestrator;
//!
//! # async fn example() -> commander_orchestrator::Result<()> {
//! let mut orchestrator = AgentOrchestrator::new().await?;
//!
//! // Process user input
//! let response = orchestrator.process_user_input("Help me refactor this code").await?;
//! println!("Agent: {}", response);
//!
//! // Process session output
//! let analysis = orchestrator.process_session_output("sess-1", "claude_code", "Tests passed!").await?;
//! if analysis.detected_completion {
//!     println!("Task completed!");
//! }
//! # Ok(())
//! # }
//! ```

mod error;
mod orchestrator;

pub use error::{OrchestratorError, Result};
pub use orchestrator::AgentOrchestrator;

// Re-export commonly used types from commander-agent
pub use commander_agent::{
    AgentContext, AgentResponse, FeedbackSummary, OutputAnalysis, SessionAgent, SessionState,
    UserAgent,
};
