//! Tab completion for TUI slash commands.

use super::App;

/// Available slash commands for completion.
pub const COMMANDS: &[&str] = &[
    "/clear", "/connect", "/disconnect", "/help", "/inspect",
    "/list", "/quit", "/rename", "/send", "/sessions", "/status",
    "/stop", "/telegram",
];

impl App {
    /// Perform tab completion on the current input.
    pub fn complete_command(&mut self) {
        // Only complete if input starts with /
        if !self.input.starts_with('/') {
            self.completions.clear();
            self.completion_index = None;
            return;
        }

        // Build completions if not already built for this prefix
        if self.completions.is_empty() || self.completion_index.is_none() {
            self.completions = COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(self.input.as_str()))
                .map(|s| s.to_string())
                .collect();
            self.completion_index = if self.completions.is_empty() {
                None
            } else {
                Some(0)
            };
        } else {
            // Cycle through completions
            if let Some(idx) = self.completion_index {
                self.completion_index = Some((idx + 1) % self.completions.len());
            }
        }

        // Apply completion
        if let Some(idx) = self.completion_index {
            if let Some(completion) = self.completions.get(idx) {
                self.input = completion.clone();
                self.cursor_pos = self.input.len();
            }
        }
    }

    /// Reset completion state (called when input changes).
    pub fn reset_completions(&mut self) {
        self.completions.clear();
        self.completion_index = None;
    }
}
