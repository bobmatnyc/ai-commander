//! Tab completion for TUI slash commands.

use super::App;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use std::time::{Duration, SystemTime};

/// Available slash commands for completion.
pub const COMMANDS: &[&str] = &[
    "/clear", "/connect", "/disconnect", "/help", "/inspect",
    "/list", "/quit", "/rename", "/send", "/sessions", "/status",
    "/stop", "/telegram",
];

impl App {
    /// Perform tab completion on the current input.
    ///
    /// Supports:
    /// - Command name completion (prefix and fuzzy matching)
    /// - Context-aware argument completion (project names, session names)
    /// - Flag/argument completion (-a cc|mpm)
    /// - Alias routing completion (@session_name)
    pub fn complete_command(&mut self) {
        // Build completions if not already built for this prefix
        if self.completions.is_empty() || self.completion_index.is_none() {
            self.completions = self.generate_completions();
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

    /// Generate completions based on current input.
    fn generate_completions(&mut self) -> Vec<String> {
        let input = self.input.clone();

        // Handle alias routing (@session_name)
        if input.contains('@') {
            return self.complete_alias_routing(&input);
        }

        // Handle slash commands
        if input.starts_with('/') {
            // Check if we're completing arguments after a command
            if input.contains(' ') {
                return self.complete_arguments(&input);
            } else {
                // Complete command name (fuzzy matching)
                return self.complete_command_name(&input);
            }
        }

        Vec::new()
    }

    /// Complete command names using fuzzy matching.
    fn complete_command_name(&self, input: &str) -> Vec<String> {
        let matcher = SkimMatcherV2::default();

        // Try fuzzy matching first
        let mut scored: Vec<(i64, &str)> = COMMANDS
            .iter()
            .filter_map(|cmd| {
                matcher.fuzzy_match(cmd, input)
                    .map(|score| (score, *cmd))
            })
            .collect();

        // Sort by score (highest first)
        scored.sort_by(|a, b| b.0.cmp(&a.0));

        if !scored.is_empty() {
            scored.into_iter().map(|(_, cmd)| cmd.to_string()).collect()
        } else {
            // Fallback to prefix matching if no fuzzy matches
            COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(input))
                .map(|s| s.to_string())
                .collect()
        }
    }

    /// Complete arguments after a command.
    fn complete_arguments(&mut self, input: &str) -> Vec<String> {
        let parts: Vec<&str> = input.split_whitespace().collect();

        if parts.is_empty() {
            return Vec::new();
        }

        let command = parts[0];

        // Check for flag completion
        if let Some(last) = parts.last() {
            // Complete -a flag values
            if parts.len() > 1 && parts[parts.len() - 2] == "-a" {
                return vec!["cc", "mpm"]
                    .into_iter()
                    .filter(|adapter| adapter.starts_with(last))
                    .map(|adapter| format!("{} {}", input.trim_end_matches(last), adapter))
                    .collect();
            }

            // Complete flags
            if last.starts_with("-") {
                if command == "/connect" || command == "/c" {
                    return vec!["-a", "-n"]
                        .into_iter()
                        .filter(|flag| flag.starts_with(last))
                        .map(|flag| format!("{} {}", input.trim_end_matches(last), flag))
                        .collect();
                }
            }
        }

        // Context-aware completion based on command
        match command {
            "/connect" | "/c" => {
                // Don't complete project names if we're likely adding flags
                if input.ends_with(' ') && (input.contains("-a") || input.contains("-n")) {
                    return Vec::new();
                }
                self.complete_project_names(input)
            }
            "/status" | "/s" => self.complete_project_names(input),
            "/stop" => self.complete_session_names(input),
            _ => Vec::new()
        }
    }

    /// Complete project names from state store.
    fn complete_project_names(&mut self, input: &str) -> Vec<String> {
        let projects = self.load_projects_cached();
        let parts: Vec<&str> = input.split_whitespace().collect();
        let command = parts[0];
        let prefix = parts.get(1).unwrap_or(&"");

        projects
            .into_iter()
            .filter(|name| name.starts_with(prefix))
            .map(|name| format!("{} {}", command, name))
            .collect()
    }

    /// Complete session names from tmux.
    fn complete_session_names(&mut self, input: &str) -> Vec<String> {
        let sessions = self.load_sessions_cached();
        let parts: Vec<&str> = input.split_whitespace().collect();
        let command = parts[0];
        let prefix = parts.get(1).unwrap_or(&"");

        sessions
            .into_iter()
            .filter(|name| name.starts_with(prefix))
            .map(|name| format!("{} {}", command, name))
            .collect()
    }

    /// Complete alias routing (@session_name).
    fn complete_alias_routing(&mut self, input: &str) -> Vec<String> {
        // Find the last @symbol position
        let at_pos = input.rfind('@');
        if at_pos.is_none() {
            return Vec::new();
        }

        let at_pos = at_pos.unwrap();
        let before_at = &input[..at_pos];
        let after_at = &input[at_pos + 1..];

        // Get session names
        let sessions = self.load_sessions_cached();

        sessions
            .into_iter()
            .filter(|name| name.starts_with(after_at))
            .map(|name| format!("{}@{}", before_at, name))
            .collect()
    }

    /// Load projects with caching (5 second TTL).
    fn load_projects_cached(&mut self) -> Vec<String> {
        // Check if we need to refresh the cache
        let needs_refresh = self.cached_projects.as_ref()
            .map(|(cached_time, _)| {
                cached_time.elapsed().unwrap_or(Duration::from_secs(10)) > Duration::from_secs(5)
            })
            .unwrap_or(true);

        if needs_refresh {
            let projects = self.store.load_all_projects()
                .ok()
                .map(|map| {
                    map.into_values()
                        .map(|p| p.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            self.cached_projects = Some((SystemTime::now(), projects.clone()));
            projects
        } else {
            self.cached_projects.as_ref()
                .map(|(_, projects)| projects.clone())
                .unwrap_or_default()
        }
    }

    /// Load tmux sessions with caching (5 second TTL).
    fn load_sessions_cached(&mut self) -> Vec<String> {
        // Check if we need to refresh the cache
        let needs_refresh = self.cached_sessions.as_ref()
            .map(|(cached_time, _)| {
                cached_time.elapsed().unwrap_or(Duration::from_secs(10)) > Duration::from_secs(5)
            })
            .unwrap_or(true);

        if needs_refresh {
            let sessions = if let Some(ref tmux) = self.tmux {
                tmux.list_sessions()
                    .ok()
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            let session_names: Vec<String> = sessions.into_iter()
                .map(|s| s.name)
                .collect();

            self.cached_sessions = Some((SystemTime::now(), session_names.clone()));
            session_names
        } else {
            self.cached_sessions.as_ref()
                .map(|(_, sessions)| sessions.clone())
                .unwrap_or_default()
        }
    }

    /// Get hint for current command.
    pub fn get_command_hint(&self) -> Option<String> {
        let input = self.input.as_str();

        if input == "/connect" || input == "/c" {
            Some("<path> -a <adapter> -n <name>  OR  <project-name>".to_string())
        } else if input.starts_with("/connect ") || input.starts_with("/c ") {
            if !input.contains("-a") && !input.contains("-n") {
                Some("[-a cc|mpm] [-n name]".to_string())
            } else {
                None
            }
        } else if input == "/status" || input == "/s" {
            Some("[project_name]".to_string())
        } else if input == "/stop" {
            Some("<session_name>".to_string())
        } else if input == "/send" {
            Some("<message>".to_string())
        } else if input == "/rename" {
            Some("<project> <new-name>".to_string())
        } else if input.starts_with("@") {
            Some("Route message to specific session(s)".to_string())
        } else {
            None
        }
    }

    /// Reset completion state (called when input changes).
    pub fn reset_completions(&mut self) {
        self.completions.clear();
        self.completion_index = None;
    }
}
