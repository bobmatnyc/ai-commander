//! Context-aware session management for different agent types.
//!
//! This module provides context tracking and automatic actions when context
//! usage reaches critical thresholds. Different strategies are supported:
//!
//! - **MPM**: Pause and resume sessions when context is low
//! - **Claude Code**: Trigger context compaction and continue
//! - **Generic**: Warn user and continue
//!
//! # Example
//!
//! ```
//! use commander_agent::context_manager::{ContextManager, ContextStrategy, ContextAction};
//!
//! let strategy = ContextStrategy::Compaction;
//! let mut manager = ContextManager::new(strategy, 200_000);
//!
//! // Update with current token usage
//! let action = manager.update(180_000);
//!
//! match action {
//!     ContextAction::Continue => println!("Context OK"),
//!     ContextAction::Warn { remaining_percent } => {
//!         println!("Warning: {}% context remaining", remaining_percent * 100.0);
//!     }
//!     ContextAction::Critical { action } => {
//!         println!("Critical: {:?}", action);
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};

/// Strategy for handling context limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextStrategy {
    /// MPM: Pause and resume sessions when context is low.
    PauseResume {
        /// Command to pause the session.
        pause_command: String,
        /// Command to resume the session.
        resume_command: String,
    },
    /// Claude Code: Compact context and continue.
    Compaction,
    /// Generic: Warn user and continue.
    WarnAndContinue,
}

impl Default for ContextStrategy {
    fn default() -> Self {
        Self::WarnAndContinue
    }
}

/// Action to take based on context status.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextAction {
    /// Context is fine, continue normally.
    Continue,
    /// Context is getting low, warn.
    Warn {
        /// Remaining context percentage (0.0 to 1.0).
        remaining_percent: f32,
    },
    /// Context critical, take action.
    Critical {
        /// The critical action to take.
        action: CriticalAction,
    },
}

/// Critical action to take when context is exhausted.
#[derive(Debug, Clone, PartialEq)]
pub enum CriticalAction {
    /// Pause the session (MPM strategy).
    Pause {
        /// Command to execute for pausing.
        command: String,
        /// Summary of current state for resumption.
        state_summary: String,
    },
    /// Compact context (Claude Code strategy).
    Compact {
        /// Number of messages to summarize.
        messages_to_summarize: usize,
    },
    /// Alert user (Generic strategy).
    Alert {
        /// Alert message to display.
        message: String,
    },
}

/// Context manager for tracking and managing context window usage.
///
/// Monitors token usage and triggers appropriate actions based on
/// the configured strategy when thresholds are reached.
#[derive(Debug, Clone)]
pub struct ContextManager {
    /// Maximum context tokens for the model.
    max_tokens: usize,
    /// Current estimated usage.
    current_tokens: usize,
    /// Warning threshold (default 20%).
    warning_threshold: f32,
    /// Critical threshold (default 10%).
    critical_threshold: f32,
    /// Strategy for this agent type.
    strategy: ContextStrategy,
    /// State summary for pause/resume operations.
    state_summary: String,
    /// Number of messages to consider for compaction.
    compaction_target: usize,
}

impl ContextManager {
    /// Create a new context manager with the given strategy and max tokens.
    ///
    /// # Arguments
    /// * `strategy` - The context handling strategy
    /// * `max_tokens` - Maximum tokens for the model's context window
    pub fn new(strategy: ContextStrategy, max_tokens: usize) -> Self {
        Self {
            max_tokens,
            current_tokens: 0,
            warning_threshold: 0.20,
            critical_threshold: 0.10,
            strategy,
            state_summary: String::new(),
            compaction_target: 10, // Default: summarize 10 oldest messages
        }
    }

    /// Create a context manager with custom thresholds.
    ///
    /// # Arguments
    /// * `strategy` - The context handling strategy
    /// * `max_tokens` - Maximum tokens for the model's context window
    /// * `warning_threshold` - Threshold for warnings (0.0 to 1.0)
    /// * `critical_threshold` - Threshold for critical actions (0.0 to 1.0)
    pub fn with_thresholds(
        strategy: ContextStrategy,
        max_tokens: usize,
        warning_threshold: f32,
        critical_threshold: f32,
    ) -> Self {
        Self {
            max_tokens,
            current_tokens: 0,
            warning_threshold: warning_threshold.clamp(0.0, 1.0),
            critical_threshold: critical_threshold.clamp(0.0, 1.0),
            strategy,
            state_summary: String::new(),
            compaction_target: 10,
        }
    }

    /// Update token count and determine action.
    ///
    /// Returns the appropriate action based on current context usage.
    pub fn update(&mut self, estimated_tokens: usize) -> ContextAction {
        self.current_tokens = estimated_tokens;
        let remaining_percent = self.remaining_percent();

        if remaining_percent <= self.critical_threshold {
            self.critical_action()
        } else if remaining_percent <= self.warning_threshold {
            ContextAction::Warn { remaining_percent }
        } else {
            ContextAction::Continue
        }
    }

    /// Calculate remaining context percentage.
    pub fn remaining_percent(&self) -> f32 {
        if self.max_tokens == 0 {
            return 0.0;
        }
        1.0 - (self.current_tokens as f32 / self.max_tokens as f32)
    }

    /// Get the current token usage.
    pub fn current_tokens(&self) -> usize {
        self.current_tokens
    }

    /// Get the maximum token capacity.
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    /// Get the warning threshold.
    pub fn warning_threshold(&self) -> f32 {
        self.warning_threshold
    }

    /// Get the critical threshold.
    pub fn critical_threshold(&self) -> f32 {
        self.critical_threshold
    }

    /// Get the context strategy.
    pub fn strategy(&self) -> &ContextStrategy {
        &self.strategy
    }

    /// Set the state summary for pause/resume operations.
    pub fn set_state_summary(&mut self, summary: impl Into<String>) {
        self.state_summary = summary.into();
    }

    /// Set the number of messages to target for compaction.
    pub fn set_compaction_target(&mut self, target: usize) {
        self.compaction_target = target;
    }

    /// Generate a state summary based on current context.
    fn generate_state_summary(&self) -> String {
        if self.state_summary.is_empty() {
            format!(
                "Session paused at {:.1}% context usage ({}/{} tokens)",
                (1.0 - self.remaining_percent()) * 100.0,
                self.current_tokens,
                self.max_tokens
            )
        } else {
            self.state_summary.clone()
        }
    }

    /// Calculate number of messages to compact based on current usage.
    fn calculate_compaction_target(&self) -> usize {
        // Increase compaction target as context gets more critical
        let overage_ratio = if self.remaining_percent() > 0.0 {
            self.critical_threshold / self.remaining_percent()
        } else {
            2.0
        };

        ((self.compaction_target as f32 * overage_ratio).ceil() as usize).max(self.compaction_target)
    }

    /// Determine the critical action based on strategy.
    fn critical_action(&self) -> ContextAction {
        match &self.strategy {
            ContextStrategy::PauseResume { pause_command, .. } => ContextAction::Critical {
                action: CriticalAction::Pause {
                    command: pause_command.clone(),
                    state_summary: self.generate_state_summary(),
                },
            },
            ContextStrategy::Compaction => ContextAction::Critical {
                action: CriticalAction::Compact {
                    messages_to_summarize: self.calculate_compaction_target(),
                },
            },
            ContextStrategy::WarnAndContinue => ContextAction::Critical {
                action: CriticalAction::Alert {
                    message: format!(
                        "Context is at {:.0}% capacity. Consider starting a new session.",
                        self.remaining_percent() * 100.0
                    ),
                },
            },
        }
    }

    /// Check if context is at warning level.
    pub fn is_warning(&self) -> bool {
        let remaining = self.remaining_percent();
        remaining <= self.warning_threshold && remaining > self.critical_threshold
    }

    /// Check if context is at critical level.
    pub fn is_critical(&self) -> bool {
        self.remaining_percent() <= self.critical_threshold
    }

    /// Reset the context manager for a new session.
    pub fn reset(&mut self) {
        self.current_tokens = 0;
        self.state_summary.clear();
    }
}

/// Default model context sizes for common models.
pub mod model_contexts {
    /// Claude 3.5 Sonnet context window (200K tokens).
    pub const CLAUDE_3_5_SONNET: usize = 200_000;
    /// Claude 3 Haiku context window (200K tokens).
    pub const CLAUDE_3_HAIKU: usize = 200_000;
    /// Claude 3 Opus context window (200K tokens).
    pub const CLAUDE_3_OPUS: usize = 200_000;
    /// GPT-4 Turbo context window (128K tokens).
    pub const GPT_4_TURBO: usize = 128_000;
    /// Default context window for unknown models.
    pub const DEFAULT: usize = 100_000;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_manager_new() {
        let manager = ContextManager::new(ContextStrategy::Compaction, 200_000);

        assert_eq!(manager.max_tokens(), 200_000);
        assert_eq!(manager.current_tokens(), 0);
        assert_eq!(manager.warning_threshold(), 0.20);
        assert_eq!(manager.critical_threshold(), 0.10);
    }

    #[test]
    fn test_context_manager_with_thresholds() {
        let manager = ContextManager::with_thresholds(
            ContextStrategy::WarnAndContinue,
            100_000,
            0.30,
            0.15,
        );

        assert_eq!(manager.warning_threshold(), 0.30);
        assert_eq!(manager.critical_threshold(), 0.15);
    }

    #[test]
    fn test_threshold_clamping() {
        let manager = ContextManager::with_thresholds(
            ContextStrategy::WarnAndContinue,
            100_000,
            1.5,  // Should clamp to 1.0
            -0.5, // Should clamp to 0.0
        );

        assert_eq!(manager.warning_threshold(), 1.0);
        assert_eq!(manager.critical_threshold(), 0.0);
    }

    #[test]
    fn test_remaining_percent() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);

        // 0% used = 100% remaining
        assert!((manager.remaining_percent() - 1.0).abs() < 0.001);

        // 50% used = 50% remaining
        manager.current_tokens = 50_000;
        assert!((manager.remaining_percent() - 0.5).abs() < 0.001);

        // 90% used = 10% remaining
        manager.current_tokens = 90_000;
        assert!((manager.remaining_percent() - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_update_continue() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);

        // 50% used -> Continue
        let action = manager.update(50_000);
        assert_eq!(action, ContextAction::Continue);
    }

    #[test]
    fn test_update_warning() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);

        // 85% used = 15% remaining -> Warning (between 20% and 10%)
        let action = manager.update(85_000);
        match action {
            ContextAction::Warn { remaining_percent } => {
                assert!((remaining_percent - 0.15).abs() < 0.001);
            }
            _ => panic!("Expected Warn action"),
        }
    }

    #[test]
    fn test_update_critical_compaction() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);

        // 95% used = 5% remaining -> Critical
        let action = manager.update(95_000);
        match action {
            ContextAction::Critical { action } => match action {
                CriticalAction::Compact { messages_to_summarize } => {
                    assert!(messages_to_summarize >= 10);
                }
                _ => panic!("Expected Compact action"),
            },
            _ => panic!("Expected Critical action"),
        }
    }

    #[test]
    fn test_update_critical_pause_resume() {
        let mut manager = ContextManager::new(
            ContextStrategy::PauseResume {
                pause_command: "/mpm-session-pause".to_string(),
                resume_command: "/mpm-session-resume".to_string(),
            },
            100_000,
        );

        // 95% used = 5% remaining -> Critical with Pause
        let action = manager.update(95_000);
        match action {
            ContextAction::Critical { action } => match action {
                CriticalAction::Pause { command, .. } => {
                    assert_eq!(command, "/mpm-session-pause");
                }
                _ => panic!("Expected Pause action"),
            },
            _ => panic!("Expected Critical action"),
        }
    }

    #[test]
    fn test_update_critical_alert() {
        let mut manager = ContextManager::new(ContextStrategy::WarnAndContinue, 100_000);

        // 95% used = 5% remaining -> Critical with Alert
        let action = manager.update(95_000);
        match action {
            ContextAction::Critical { action } => match action {
                CriticalAction::Alert { message } => {
                    assert!(message.contains("capacity"));
                }
                _ => panic!("Expected Alert action"),
            },
            _ => panic!("Expected Critical action"),
        }
    }

    #[test]
    fn test_is_warning() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);

        manager.current_tokens = 50_000; // 50% remaining
        assert!(!manager.is_warning());

        manager.current_tokens = 85_000; // 15% remaining
        assert!(manager.is_warning());

        manager.current_tokens = 95_000; // 5% remaining (critical)
        assert!(!manager.is_warning());
    }

    #[test]
    fn test_is_critical() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);

        manager.current_tokens = 85_000; // 15% remaining
        assert!(!manager.is_critical());

        // 91% used = 9% remaining, which is < 10% threshold
        manager.current_tokens = 91_000;
        assert!(manager.is_critical());

        manager.current_tokens = 95_000; // 5% remaining
        assert!(manager.is_critical());
    }

    #[test]
    fn test_state_summary() {
        let mut manager = ContextManager::new(
            ContextStrategy::PauseResume {
                pause_command: "/pause".to_string(),
                resume_command: "/resume".to_string(),
            },
            100_000,
        );

        // Without custom summary
        manager.update(95_000);
        let summary = manager.generate_state_summary();
        assert!(summary.contains("95_000") || summary.contains("95.0%"));

        // With custom summary
        manager.set_state_summary("Custom pause state: working on feature X");
        let summary = manager.generate_state_summary();
        assert_eq!(summary, "Custom pause state: working on feature X");
    }

    #[test]
    fn test_reset() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);
        manager.update(50_000);
        manager.set_state_summary("Some state");

        assert_eq!(manager.current_tokens(), 50_000);

        manager.reset();

        assert_eq!(manager.current_tokens(), 0);
    }

    #[test]
    fn test_compaction_target_increases_with_criticality() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 100_000);
        manager.set_compaction_target(10);

        // At 15% remaining (warning level)
        manager.current_tokens = 85_000;
        let target_warning = manager.calculate_compaction_target();

        // At 5% remaining (critical level)
        manager.current_tokens = 95_000;
        let target_critical = manager.calculate_compaction_target();

        // More aggressive compaction when more critical
        assert!(target_critical >= target_warning);
    }

    #[test]
    fn test_strategy_serialization() {
        let strategy = ContextStrategy::PauseResume {
            pause_command: "/pause".to_string(),
            resume_command: "/resume".to_string(),
        };

        let json = serde_json::to_string(&strategy).unwrap();
        let parsed: ContextStrategy = serde_json::from_str(&json).unwrap();

        assert_eq!(strategy, parsed);
    }

    #[test]
    fn test_zero_max_tokens() {
        let mut manager = ContextManager::new(ContextStrategy::Compaction, 0);

        // Should handle zero gracefully
        assert_eq!(manager.remaining_percent(), 0.0);

        let action = manager.update(100);
        // Should be critical since 0% remaining
        matches!(action, ContextAction::Critical { .. });
    }

    #[test]
    fn test_model_context_sizes() {
        assert_eq!(model_contexts::CLAUDE_3_5_SONNET, 200_000);
        assert_eq!(model_contexts::CLAUDE_3_HAIKU, 200_000);
        assert_eq!(model_contexts::GPT_4_TURBO, 128_000);
    }
}
