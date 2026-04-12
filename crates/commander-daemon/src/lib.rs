//! Core daemon library for ai-commander.
//!
//! This crate provides the central daemon service that manages all sessions,
//! handles IPC communication with clients, and provides a unified interface
//! for TUI, GUI, and Telegram bot clients.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                  commander-daemon                       │
//! │  ┌─────────────────────────────────────────────────┐    │
//! │  │              Core Service                       │    │
//! │  │  • Session Management                           │    │
//! │  │  • Project Orchestration                       │    │
//! │  │  • Memory Monitoring                           │    │
//! │  │  • State Persistence                           │    │
//! │  │  • Health Checking                             │    │
//! │  └─────────────────────────────────────────────────┘    │
//! │  ┌─────────────────────────────────────────────────┐    │
//! │  │              IPC Layer                         │    │
//! │  │  • Unix Domain Sockets / Named Pipes          │    │
//! │  │  • JSON-RPC Protocol                          │    │
//! │  │  • Authentication                             │    │
//! │  └─────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────┘
//! ```

pub mod error;
pub mod health;
pub mod idle_tracker;
pub mod message_poller;
pub mod monitoring;
pub mod pairing;
pub mod service;
pub mod sessions;
pub mod ipc;

// Re-export main types
pub use error::{DaemonError, Result};
pub use health::{HealthChecker, HealthResult, HealthStatus};
pub use service::DaemonService;
pub use sessions::SessionManager;
pub use monitoring::MemoryMonitor;
pub use pairing::PairingManager;
