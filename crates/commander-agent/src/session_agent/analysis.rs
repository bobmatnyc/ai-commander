//! Output analysis logic for SessionAgent.

use tracing::{debug, trace};

use commander_core::{ChangeNotification, ChangeType, Significance};

use crate::client::ChatMessage;
use crate::error::Result;

use super::state::OutputAnalysis;
use super::SessionAgent;
use super::DEFAULT_SYSTEM_PROMPT;

impl SessionAgent {
    /// Process session output with smart change detection.
    ///
    /// This method uses deterministic change detection to avoid unnecessary
    /// LLM calls. It only invokes the LLM for significant changes (errors,
    /// completion, waiting for input).
    ///
    /// # Returns
    ///
    /// - `Ok(Some(notification))` if user should be notified
    /// - `Ok(None)` if change was not significant enough for notification
    /// - `Err(_)` if LLM analysis failed
    ///
    /// # Example
    ///
    /// ```ignore
    /// let notification = agent.process_output_change(output).await?;
    /// if let Some(notif) = notification {
    ///     if notif.requires_action {
    ///         // Alert user immediately
    ///     }
    /// }
    /// ```
    pub async fn process_output_change(
        &mut self,
        output: &str,
    ) -> Result<Option<ChangeNotification>> {
        // Stage 1: Deterministic change detection (no LLM call)
        let change = self.change_detector.detect(output);

        debug!(
            session_id = %self.session_id,
            change_type = ?change.change_type,
            significance = ?change.significance,
            new_lines = change.diff_lines.len(),
            "Change detected"
        );

        // Stage 2: Return early if not significant enough
        if !change.is_meaningful() {
            trace!(
                session_id = %self.session_id,
                "Change not significant, skipping LLM analysis"
            );
            return Ok(None);
        }

        // Stage 3: For significant changes, optionally do LLM analysis
        // Only invoke LLM for high-significance changes to get better summary
        let (summary, requires_action) = if change.significance >= Significance::High {
            // Do LLM analysis for high-significance changes
            let analysis = self.analyze_output(output).await?;

            let requires_action = analysis.waiting_for_input || analysis.error_detected.is_some();
            let summary = if analysis.summary.is_empty() {
                change.summary.clone()
            } else {
                analysis.summary
            };

            (summary, requires_action)
        } else {
            // For medium significance, use the pattern-based summary
            (change.summary.clone(), false)
        };

        // Stage 4: Determine if user needs to know
        let should_notify = change.requires_notification()
            || requires_action
            || matches!(change.change_type, ChangeType::Error | ChangeType::WaitingForInput);

        if should_notify {
            Ok(Some(ChangeNotification {
                session_id: self.session_id.clone(),
                summary,
                requires_action,
                change_type: change.change_type,
                significance: change.significance,
            }))
        } else {
            Ok(None)
        }
    }

    /// Analyze raw output from the session.
    ///
    /// This method uses the LLM to analyze session output and extract
    /// progress indicators, completion status, errors, and file changes.
    pub async fn analyze_output(&mut self, output: &str) -> Result<OutputAnalysis> {
        // Store the output
        self.session_state.set_last_output(output);

        let analysis_prompt = format!(
            r#"Analyze the following session output and extract:
1. Whether a task was completed (look for success messages, "done", completion indicators)
2. Whether the session is waiting for user input (prompts, questions, input requests)
3. Any errors or warnings (error messages, failures, stack traces)
4. Files that were modified (created, edited, deleted)

Output to analyze:
```
{}
```

Provide a brief summary and structured analysis."#,
            output.chars().take(4000).collect::<String>() // Limit output size
        );

        // Build messages for analysis
        let messages = vec![
            ChatMessage::system(
                self.config
                    .system_prompt
                    .as_deref()
                    .unwrap_or(DEFAULT_SYSTEM_PROMPT),
            ),
            ChatMessage::user(analysis_prompt),
        ];

        // Send request without tools for direct analysis
        let response = self
            .client
            .chat(&self.config, messages, None)
            .await?;

        let content = response
            .message()
            .and_then(|m| m.content.clone())
            .unwrap_or_default();

        // Parse the response to extract structured analysis
        let analysis = self.parse_analysis_response(&content, output);

        // Update state based on analysis
        self.update_state(&analysis);

        Ok(analysis)
    }

    /// Parse the LLM's analysis response into structured data.
    pub(super) fn parse_analysis_response(&self, response: &str, _output: &str) -> OutputAnalysis {
        let response_lower = response.to_lowercase();

        let mut analysis = OutputAnalysis::with_summary(
            response.lines().next().unwrap_or("Analysis complete").to_string()
        );

        // Detect completion
        analysis.detected_completion = response_lower.contains("completed")
            || response_lower.contains("success")
            || response_lower.contains("finished")
            || response_lower.contains("done");

        // Detect waiting for input
        analysis.waiting_for_input = response_lower.contains("waiting for input")
            || response_lower.contains("requires input")
            || response_lower.contains("user input needed")
            || response_lower.contains("prompt");

        // Detect errors
        if response_lower.contains("error") || response_lower.contains("failed") {
            // Try to extract error message
            for line in response.lines() {
                let line_lower = line.to_lowercase();
                if line_lower.contains("error") || line_lower.contains("failed") {
                    analysis.error_detected = Some(line.trim().to_string());
                    break;
                }
            }
        }

        // Extract file changes (simple heuristic)
        for line in response.lines() {
            let line_lower = line.to_lowercase();
            if line_lower.contains("modified:") || line_lower.contains("created:") || line_lower.contains("edited:") {
                // Try to extract file path
                if let Some(path_start) = line.find(':') {
                    let path = line[path_start + 1..].trim();
                    if !path.is_empty() {
                        analysis.files_changed.push(path.to_string());
                    }
                }
            }
        }

        analysis
    }

    /// Update session state based on output analysis.
    pub fn update_state(&mut self, analysis: &OutputAnalysis) {
        // Add detected files
        for file in &analysis.files_changed {
            self.session_state.add_modified_file(file);
        }

        // Update progress based on completion
        if analysis.detected_completion {
            self.session_state.set_progress(1.0);
            self.session_state.clear_current_task();
        }

        // Add blocker if error detected
        if let Some(ref error) = analysis.error_detected {
            self.session_state.add_blocker(error.clone());
        }

        // Store summary for context
        if !analysis.summary.is_empty() {
            self.context.set_summarized_history(format!(
                "Last analysis: {}",
                analysis.summary
            ));
        }
    }
}
