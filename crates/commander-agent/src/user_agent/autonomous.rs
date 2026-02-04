//! Autonomous execution logic for the User Agent.
//!
//! Implements "Ralph" style push-to-completion behavior where the agent
//! drives work forward, only stopping when blocked or complete.

use tracing::{debug, info, warn};

use crate::agent::Agent;
use crate::client::ChatMessage;
use crate::completion_driver::{
    AutonomousResult, Blocker, CompletionDriver, ContinueDecision, Goal, GoalStatus,
};
use crate::error::Result;

use super::UserAgent;

impl UserAgent {
    /// Process a user request autonomously until completion or blocker.
    ///
    /// This implements "Ralph" style push-to-completion behavior where the agent
    /// drives work forward, only stopping when:
    /// - All goals are complete
    /// - A blocker requires user input
    /// - Maximum iterations reached (safety limit)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = agent.process_autonomous("Implement user authentication").await?;
    /// match result {
    ///     AutonomousResult::Complete { summary, .. } => println!("Done: {}", summary),
    ///     AutonomousResult::NeedsInput { reason, blockers, .. } => {
    ///         println!("Blocked: {}", reason);
    ///         for blocker in blockers {
    ///             println!("- {}", blocker.reason);
    ///         }
    ///     }
    ///     AutonomousResult::CheckIn { progress, .. } => println!("Progress: {}", progress),
    /// }
    /// ```
    pub async fn process_autonomous(&mut self, initial_request: &str) -> Result<AutonomousResult> {
        info!(
            "Starting autonomous processing: {}...",
            &initial_request[..initial_request.len().min(50)]
        );

        // Initialize completion driver
        let mut driver = CompletionDriver::new();

        // Parse initial request into goals
        let goals = self.parse_goals(initial_request).await?;
        driver.set_goals(goals);

        info!("Parsed {} goals from request", driver.goals().len());

        // Main autonomous loop
        loop {
            match driver.should_continue() {
                ContinueDecision::Continue => {
                    // Execute next action
                    let action_result = self.execute_next_action(&mut driver).await;

                    match action_result {
                        Ok(Some(blocker)) => {
                            driver.add_blocker(blocker);
                        }
                        Ok(None) => {
                            // Action completed successfully
                        }
                        Err(e) => {
                            // Error occurred - determine if we should add a blocker
                            warn!("Action error: {}", e);
                            let blocker = self.classify_error_as_blocker(&e);
                            if let Some(b) = blocker {
                                driver.add_blocker(b);
                            } else {
                                // Recoverable error, continue
                                debug!("Error was recoverable, continuing");
                            }
                        }
                    }

                    driver.increment_iteration();
                }
                ContinueDecision::StopForUser { reason, blockers } => {
                    info!("Stopping for user input: {}", reason);
                    return Ok(AutonomousResult::NeedsInput {
                        reason,
                        blockers,
                        progress: driver.format_progress(),
                    });
                }
                ContinueDecision::CheckIn { reason, progress } => {
                    info!("Periodic check-in: {}", reason);
                    return Ok(AutonomousResult::CheckIn { reason, progress });
                }
                ContinueDecision::Complete { summary } => {
                    info!("All goals complete");
                    return Ok(AutonomousResult::Complete {
                        summary,
                        goals_achieved: driver.goals().to_vec(),
                    });
                }
            }
        }
    }

    /// Resume autonomous processing after user provides input.
    ///
    /// Call this after receiving user input that resolves blockers.
    pub async fn resume_autonomous(
        &mut self,
        user_input: &str,
        driver: &mut CompletionDriver,
    ) -> Result<AutonomousResult> {
        info!("Resuming autonomous processing with user input");

        // Clear blockers since user provided input
        driver.clear_blockers();
        driver.reset_iterations();

        // Process the user input to update context
        let context = self.context.clone();
        let _ = self.process(user_input, &context).await?;

        // Continue autonomous processing
        loop {
            match driver.should_continue() {
                ContinueDecision::Continue => {
                    let action_result = self.execute_next_action(driver).await;
                    if let Ok(Some(blocker)) = action_result {
                        driver.add_blocker(blocker);
                    }
                    driver.increment_iteration();
                }
                ContinueDecision::StopForUser { reason, blockers } => {
                    return Ok(AutonomousResult::NeedsInput {
                        reason,
                        blockers,
                        progress: driver.format_progress(),
                    });
                }
                ContinueDecision::CheckIn { reason, progress } => {
                    return Ok(AutonomousResult::CheckIn { reason, progress });
                }
                ContinueDecision::Complete { summary } => {
                    return Ok(AutonomousResult::Complete {
                        summary,
                        goals_achieved: driver.goals().to_vec(),
                    });
                }
            }
        }
    }

    /// Parse a user request into actionable goals.
    pub(crate) async fn parse_goals(&mut self, request: &str) -> Result<Vec<Goal>> {
        // Use the LLM to parse goals from the request
        let goal_prompt = format!(
            r#"Analyze this request and extract actionable goals.
Return goals as a simple numbered list, one goal per line.
Keep goals specific and actionable.

Request: {}

Goals:"#,
            request
        );

        let messages = vec![
            ChatMessage::system(
                "You are a task decomposition assistant. Extract clear, actionable goals from user requests.",
            ),
            ChatMessage::user(&goal_prompt),
        ];

        let response = self.client.chat(&self.config, messages, None).await?;

        let content = response
            .message()
            .and_then(|m| m.content.clone())
            .unwrap_or_default();

        // Parse the response into goals
        let goals: Vec<Goal> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                // Remove numbering like "1. " or "- "
                let cleaned = line.trim().trim_start_matches(|c: char| {
                    c.is_ascii_digit() || c == '.' || c == '-' || c == ' '
                });
                Goal::new(cleaned.trim())
            })
            .filter(|g| !g.description.is_empty())
            .collect();

        // If parsing failed, create a single goal from the original request
        if goals.is_empty() {
            Ok(vec![Goal::new(request)])
        } else {
            Ok(goals)
        }
    }

    /// Execute the next action toward completing goals.
    pub(crate) async fn execute_next_action(
        &mut self,
        driver: &mut CompletionDriver,
    ) -> Result<Option<Blocker>> {
        // Find the next goal to work on
        let next_goal = if let Some(current) = driver.current_goal() {
            current.description.clone()
        } else if let Some(pending) = driver.next_pending_goal() {
            // Mark it as in progress
            let desc = pending.description.clone();
            driver.update_goal_status(&desc, GoalStatus::InProgress);
            desc
        } else {
            // No more goals
            return Ok(None);
        };

        debug!("Working on goal: {}", next_goal);

        // Generate action for this goal
        let action_prompt = format!(
            r#"You are working on this goal: {}

Current progress:
{}

Determine the next concrete action to take. If you need to use a tool, use it.
If this goal is complete, say "[GOAL COMPLETE]".
If you're blocked and need user input, say "[BLOCKED]" followed by what you need.

What is your next action?"#,
            next_goal,
            driver.format_progress()
        );

        // Process through the normal flow which handles tool calling
        let context = self.context.clone();
        let response = self.process(&action_prompt, &context).await?;

        // Analyze the response
        let content = response.content.to_lowercase();

        if content.contains("[goal complete]")
            || content.contains("completed")
            || content.contains("[done]")
        {
            driver.complete_goal(&next_goal);
            info!("Goal completed: {}", next_goal);
            return Ok(None);
        }

        if content.contains("[blocked]")
            || content.contains("need your input")
            || content.contains("cannot proceed")
        {
            // Extract blocker reason from response
            let reason = self.extract_blocker_reason(&response.content);
            let blocker_type = self.classify_blocker_type(&response.content);
            let options = self.extract_options(&response.content);

            return Ok(Some(Blocker::with_options(reason, blocker_type, options)));
        }

        // Goal still in progress
        Ok(None)
    }
}
