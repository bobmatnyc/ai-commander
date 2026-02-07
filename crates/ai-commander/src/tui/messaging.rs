//! Message handling for the TUI.
//!
//! Contains methods for sending messages, polling output,
//! and handling summarization of responses.

use std::sync::mpsc;
use std::time::Instant;

use commander_core::{find_new_lines, is_claude_ready, summarize_blocking_with_fallback};

use super::app::{App, Message};

impl App {
    /// Send a message to the connected project.
    pub fn send_message(&mut self, message: &str) -> Result<(), String> {
        let project = self.project.as_ref()
            .ok_or_else(|| "Not connected to any project".to_string())?;

        let session = self.sessions.get(project)
            .ok_or_else(|| "Session not found".to_string())?;

        let tmux = self.tmux.as_ref()
            .ok_or_else(|| "Tmux not available".to_string())?;

        // Capture initial output for comparison
        self.last_output = tmux.capture_output(session, None, Some(200))
            .unwrap_or_default();

        // Send the message
        tmux.send_line(session, None, message)
            .map_err(|e| format!("Failed to send: {}", e))?;

        // Add sent message to output and reset response collection
        self.messages.push(Message::sent(project.clone(), message));
        self.pending_query = Some(message.to_string());
        self.response_buffer.clear();
        self.last_activity = Some(Instant::now());
        self.is_working = true;
        self.is_summarizing = false;
        self.progress = 0.0;
        self.scroll_to_bottom();

        Ok(())
    }

    /// Poll for new output from tmux and trigger summarization when idle.
    pub fn poll_output(&mut self) {
        // Check for summarization results first
        if let Some(rx) = &self.summarizer_rx {
            if let Ok(summary) = rx.try_recv() {
                // Got summary result
                if let Some(project) = &self.project {
                    self.messages.push(Message::received(project.clone(), summary));
                }
                self.summarizer_rx = None;
                self.is_summarizing = false;
                self.is_working = false;
                self.response_buffer.clear();
                self.pending_query = None;
                self.scroll_to_bottom();
                return;
            }
        }

        if !self.is_working || self.is_summarizing {
            // Update progress animation if summarizing
            if self.is_summarizing {
                self.progress = (self.progress + 0.03) % 1.0;
            }
            return;
        }

        let Some(project) = &self.project else { return };
        let Some(session) = self.sessions.get(project) else { return };
        let Some(tmux) = &self.tmux else { return };

        // Capture current output
        let current_output = match tmux.capture_output(session, None, Some(200)) {
            Ok(output) => output,
            Err(_) => return,
        };

        // Check for new content
        if current_output != self.last_output {
            let new_lines = find_new_lines(&self.last_output, &current_output);
            for line in new_lines {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    self.response_buffer.push(trimmed.to_string());
                }
            }
            self.last_output = current_output.clone();
            self.last_activity = Some(Instant::now());
        }

        // Check if Claude Code is idle (prompt visible and no activity for 1.5s)
        let is_idle = self.last_activity
            .map(|t| t.elapsed().as_millis() > 1500)
            .unwrap_or(false);

        // Check for various Claude Code idle patterns
        let has_prompt = is_claude_ready(&current_output);

        if is_idle && has_prompt && !self.response_buffer.is_empty() {
            // Trigger summarization
            self.trigger_summarization();
        }

        // Update progress animation
        self.progress = (self.progress + 0.05) % 1.0;
    }

    /// Trigger async summarization of the response buffer.
    ///
    /// Uses the agent orchestrator for LLM-based analysis when available,
    /// falling back to direct summarization otherwise.
    pub(super) fn trigger_summarization(&mut self) {
        let raw_response = self.response_buffer.join("\n");
        let query = self.pending_query.clone().unwrap_or_default();

        // Set summarizing state (status bar will show it)
        self.is_summarizing = true;

        // Create channel for result
        let (tx, rx) = mpsc::channel();
        self.summarizer_rx = Some(rx);

        // Try to use orchestrator for LLM analysis (agents feature)
        #[cfg(feature = "agents")]
        {
            if let Some(summary) = self.try_orchestrator_analysis(&raw_response) {
                // Got synchronous result from orchestrator
                let _ = tx.send(summary);
                return;
            }
        }

        // Fallback: Spawn thread for blocking HTTP call
        std::thread::spawn(move || {
            let summary = summarize_blocking_with_fallback(&query, &raw_response);
            let _ = tx.send(summary);
        });
    }

    /// Try to analyze output using the agent orchestrator.
    ///
    /// Returns Some(summary) if orchestrator analysis succeeded, None to fall back.
    /// This runs synchronously using block_on since we need mutable access to orchestrator.
    #[cfg(feature = "agents")]
    fn try_orchestrator_analysis(&mut self, output: &str) -> Option<String> {
        // Need runtime handle for async operation
        let handle = self.runtime_handle.as_ref()?.clone();

        // Need orchestrator
        let orchestrator = self.orchestrator.as_mut()?;

        // Get session info for the orchestrator
        let session_name = self.project.as_ref()
            .and_then(|p| self.sessions.get(p))?
            .clone();

        // Determine adapter type from project config (default to claude_code)
        let adapter_type = "claude_code";

        // Run async analysis synchronously
        // This blocks briefly but provides LLM-based semantic understanding
        let output = output.to_string();
        match handle.block_on(orchestrator.process_session_output(&session_name, adapter_type, &output)) {
            Ok(analysis) => {
                // Build summary from OutputAnalysis
                let mut summary = analysis.summary.clone();

                // Add context about state
                if analysis.waiting_for_input {
                    if !summary.is_empty() {
                        summary.push_str("\n\n");
                    }
                    summary.push_str("[Ready for input]");
                }

                if analysis.detected_completion {
                    if !summary.is_empty() {
                        summary.push_str("\n\n");
                    }
                    summary.push_str("[Task completed]");
                }

                if let Some(error) = &analysis.error_detected {
                    if !summary.is_empty() {
                        summary.push_str("\n\n");
                    }
                    summary.push_str(&format!("[Error: {}]", error));
                }

                if !analysis.files_changed.is_empty() {
                    if !summary.is_empty() {
                        summary.push_str("\n\n");
                    }
                    summary.push_str(&format!("Files changed: {}", analysis.files_changed.join(", ")));
                }

                Some(summary)
            }
            Err(e) => {
                tracing::debug!(error = %e, "Orchestrator analysis failed, falling back");
                None
            }
        }
    }

    /// Stop the working indicator.
    pub fn stop_working(&mut self) {
        self.is_working = false;
        self.is_summarizing = false;
        self.progress = 0.0;
        self.response_buffer.clear();
        self.pending_query = None;
    }

    /// Check if currently summarizing.
    pub fn is_summarizing(&self) -> bool {
        self.is_summarizing
    }

    /// Get the number of lines in the response buffer.
    pub fn response_buffer_len(&self) -> usize {
        self.response_buffer.len()
    }
}
