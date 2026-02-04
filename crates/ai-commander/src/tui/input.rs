//! Input handling for the TUI.
//!
//! Contains methods for cursor movement, character input,
//! command history navigation, and input submission.

use std::path::PathBuf;

use super::app::{App, Message};
use crate::filesystem;

impl App {
    /// Handle character input.
    pub fn enter_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
    }

    /// Delete character before cursor.
    pub fn delete_char(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.input.remove(self.cursor_pos);
        }
    }

    /// Move cursor left.
    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    /// Move cursor right.
    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos += 1;
        }
    }

    /// Clear the input.
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_pos = 0;
    }

    /// Submit the current input.
    pub fn submit(&mut self) {
        let input = std::mem::take(&mut self.input);
        self.cursor_pos = 0;
        self.history_index = None;
        self.saved_input.clear();

        if input.is_empty() {
            return;
        }

        // Add to history (avoid duplicates of last entry)
        if self.command_history.last() != Some(&input) {
            self.command_history.push(input.clone());
        }

        // Handle commands
        if let Some(cmd) = input.strip_prefix('/') {
            self.handle_command(cmd);
        } else if input.starts_with('@') {
            // @ routing syntax
            self.handle_route(&input);
        } else if self.project.is_some() {
            // Check for filesystem commands first
            let working_dir = self.project_path.as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

            if let Some(fs_cmd) = filesystem::parse_command(&input, &working_dir) {
                // Execute filesystem command locally
                let result = filesystem::execute(&fs_cmd, &working_dir);
                let project = self.project.clone().unwrap_or_default();

                self.messages.push(Message::sent(project.clone(), input.clone()));

                if result.success {
                    self.messages.push(Message::received(project.clone(), result.message));
                    if let Some(details) = result.details {
                        for line in details.lines() {
                            self.messages.push(Message::received(project.clone(), line.to_string()));
                        }
                    }
                } else {
                    self.messages.push(Message::system(format!("Error: {}", result.message)));
                }
                self.scroll_to_bottom();
            } else {
                // Send to connected project
                if let Err(e) = self.send_message(&input) {
                    self.messages.push(Message::system(format!("Error: {}", e)));
                }
            }
        } else {
            // Not connected - treat as Commander instruction
            // Show available options for ambiguous commands
            self.messages.push(Message::system(format!("Commander: {}", input)));
            self.messages.push(Message::system(""));
            self.messages.push(Message::system("Did you mean to route this to a session?"));
            self.messages.push(Message::system("  @<session> <message>  - Send to specific session"));
            self.messages.push(Message::system("  /connect <name>       - Connect to a session first"));
            self.messages.push(Message::system("  /sessions             - List available sessions"));
        }
    }

    /// Navigate to previous command in history (Up arrow).
    pub fn history_prev(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // First time pressing up - save current input and go to last history item
                self.saved_input = std::mem::take(&mut self.input);
                self.history_index = Some(self.command_history.len() - 1);
                self.input = self.command_history.last().cloned().unwrap_or_default();
            }
            Some(idx) if idx > 0 => {
                // Move to earlier history
                self.history_index = Some(idx - 1);
                self.input = self.command_history.get(idx - 1).cloned().unwrap_or_default();
            }
            _ => {
                // Already at oldest entry
            }
        }
        self.cursor_pos = self.input.len();
    }

    /// Navigate to next command in history (Down arrow).
    pub fn history_next(&mut self) {
        match self.history_index {
            Some(idx) => {
                if idx + 1 < self.command_history.len() {
                    // Move to more recent history
                    self.history_index = Some(idx + 1);
                    self.input = self.command_history.get(idx + 1).cloned().unwrap_or_default();
                } else {
                    // Return to saved input
                    self.history_index = None;
                    self.input = std::mem::take(&mut self.saved_input);
                }
                self.cursor_pos = self.input.len();
            }
            None => {
                // Not in history mode
            }
        }
    }
}
