//! Event-driven adapter trait.
//!
//! Sibling to `RuntimeAdapter`. Where `RuntimeAdapter` consumes a terminal's
//! raw output stream and infers state from patterns, `EventDrivenAdapter`
//! produces a typed stream of events directly from the underlying runtime
//! (e.g. from an SDK that already speaks a structured event protocol like
//! NDJSON stream-json).
//!
//! This module provides only the trait and supporting types. Consumers
//! (executor, poller, TUI, REPL, telegram) are integrated in a later phase.

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use crate::traits::AdapterInfo;

/// Opaque handle to an active event-driven session.
///
/// Adapters may attach their own state keyed by `id`; consumers should treat
/// the handle as opaque and only use it to refer back to a previously started
/// session.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    /// Adapter-specific session identifier.
    pub id: String,
}

/// Events emitted by an event-driven adapter during a single turn.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// Streaming text chunk (partial response).
    TextChunk(String),
    /// Tool being invoked by the agent.
    ToolUse { name: String },
    /// Session turn completed with optional summary.
    Complete { summary: Option<String> },
    /// Error occurred during session.
    Error(String),
}

/// Alias for the event stream produced by an event-driven adapter.
pub type EventStream = Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>;

/// An adapter that produces a stream of typed events rather than relying on
/// terminal-output pattern matching.
#[async_trait]
pub trait EventDrivenAdapter: Send + Sync {
    /// Returns static info about this adapter (id, name, description).
    fn info(&self) -> &AdapterInfo;

    /// Starts a new session (or resumes an existing one) for the given project
    /// path and initial prompt.
    ///
    /// If `resume_id` is `Some`, the adapter should attempt to resume the
    /// session with that identifier (e.g. the serve-daemon session ID)
    /// instead of creating a brand-new session.
    ///
    /// Returns a handle that identifies the session for follow-up calls, and
    /// a stream of events for this first turn.
    async fn start_session(
        &self,
        project_path: &str,
        prompt: &str,
        resume_id: Option<&str>,
    ) -> Result<(SessionHandle, EventStream), String>;

    /// Sends a follow-up message to an existing session.
    ///
    /// Returns a new event stream for this turn. The session continues to be
    /// referenced via its handle.
    async fn send(
        &self,
        handle: &SessionHandle,
        message: &str,
    ) -> Result<EventStream, String>;

    /// Stops and cleans up a session. Must be idempotent: calling `stop` on
    /// an unknown/already-stopped handle should succeed.
    async fn stop(&self, handle: SessionHandle) -> Result<(), String>;
}
