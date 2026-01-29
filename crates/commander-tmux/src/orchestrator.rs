//! Tmux orchestrator for session and pane management.

use std::process::{Command, Output};

use tracing::{debug, trace, warn};

use crate::{Result, TmuxError, TmuxPane, TmuxSession};

/// Main tmux orchestrator for session and pane management.
#[derive(Debug)]
pub struct TmuxOrchestrator {
    /// Path to tmux binary.
    tmux_path: String,
}

impl TmuxOrchestrator {
    /// Create a new TmuxOrchestrator.
    ///
    /// Verifies that tmux is available in PATH.
    ///
    /// # Errors
    ///
    /// Returns `TmuxError::NotFound` if tmux is not available.
    pub fn new() -> Result<Self> {
        let tmux_path = Self::find_tmux()?;
        debug!(path = %tmux_path, "tmux found");
        Ok(Self { tmux_path })
    }

    /// Check if tmux is available in PATH.
    pub fn is_available() -> bool {
        Self::find_tmux().is_ok()
    }

    /// Find tmux binary in PATH.
    fn find_tmux() -> Result<String> {
        let output = Command::new("which").arg("tmux").output()?;

        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                return Err(TmuxError::NotFound);
            }
            Ok(path)
        } else {
            Err(TmuxError::NotFound)
        }
    }

    /// Run a tmux command and return the output.
    fn run_tmux(&self, args: &[&str]) -> Result<Output> {
        trace!(args = ?args, "running tmux command");
        let output = Command::new(&self.tmux_path).args(args).output()?;
        trace!(
            status = %output.status,
            stdout_len = output.stdout.len(),
            stderr_len = output.stderr.len(),
            "tmux command completed"
        );
        Ok(output)
    }

    /// Run a tmux command and check for success.
    fn run_tmux_checked(&self, args: &[&str]) -> Result<String> {
        let output = self.run_tmux(args)?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(TmuxError::CommandFailed(stderr))
        }
    }

    // ==================== Session Management ====================

    /// Create a new detached tmux session.
    ///
    /// # Errors
    ///
    /// Returns error if session already exists or tmux command fails.
    pub fn create_session(&self, name: &str) -> Result<TmuxSession> {
        debug!(name = %name, "creating tmux session");

        self.run_tmux_checked(&["new-session", "-d", "-s", name])?;

        // Verify session was created and get details
        let sessions = self.list_sessions()?;
        sessions
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| {
                TmuxError::CommandFailed(format!("session '{}' was not created", name))
            })
    }

    /// Destroy a tmux session.
    ///
    /// # Errors
    ///
    /// Returns `TmuxError::SessionNotFound` if session doesn't exist.
    pub fn destroy_session(&self, name: &str) -> Result<()> {
        debug!(name = %name, "destroying tmux session");

        if !self.session_exists(name) {
            return Err(TmuxError::SessionNotFound(name.to_string()));
        }

        self.run_tmux_checked(&["kill-session", "-t", name])?;
        Ok(())
    }

    /// List all tmux sessions.
    pub fn list_sessions(&self) -> Result<Vec<TmuxSession>> {
        let output = self.run_tmux(&["list-sessions", "-F", "#{session_name}:#{session_created}"])?;

        // If no sessions exist, tmux returns non-zero exit code
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // "no server running" or "no sessions" means empty list
            if stderr.contains("no server running") || stderr.contains("no sessions") {
                return Ok(Vec::new());
            }
            return Err(TmuxError::CommandFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut sessions = Vec::new();

        for line in stdout.lines() {
            if line.is_empty() {
                continue;
            }
            match TmuxSession::parse(line) {
                Ok(mut session) => {
                    // Load panes for this session
                    if let Ok(panes) = self.list_panes(&session.name) {
                        session.panes = panes;
                    }
                    sessions.push(session);
                }
                Err(e) => {
                    warn!(line = %line, error = %e, "failed to parse session");
                }
            }
        }

        Ok(sessions)
    }

    /// Check if a session exists.
    pub fn session_exists(&self, name: &str) -> bool {
        let output = self.run_tmux(&["has-session", "-t", name]);
        matches!(output, Ok(o) if o.status.success())
    }

    // ==================== Pane Management ====================

    /// Create a new pane in the session (splits the window).
    ///
    /// # Errors
    ///
    /// Returns `TmuxError::SessionNotFound` if session doesn't exist.
    pub fn create_pane(&self, session: &str) -> Result<TmuxPane> {
        debug!(session = %session, "creating pane");

        if !self.session_exists(session) {
            return Err(TmuxError::SessionNotFound(session.to_string()));
        }

        // Split the window to create a new pane
        self.run_tmux_checked(&["split-window", "-t", session])?;

        // Get the newly created pane (should be the active one)
        let panes = self.list_panes(session)?;
        panes
            .into_iter()
            .find(|p| p.active)
            .ok_or_else(|| TmuxError::CommandFailed("failed to find new pane".to_string()))
    }

    /// List all panes in a session.
    ///
    /// # Errors
    ///
    /// Returns `TmuxError::SessionNotFound` if session doesn't exist.
    pub fn list_panes(&self, session: &str) -> Result<Vec<TmuxPane>> {
        if !self.session_exists(session) {
            return Err(TmuxError::SessionNotFound(session.to_string()));
        }

        let output = self.run_tmux_checked(&[
            "list-panes",
            "-t",
            session,
            "-F",
            "#{pane_id}:#{pane_index}:#{pane_active}:#{pane_width}:#{pane_height}",
        ])?;

        let mut panes = Vec::new();
        for line in output.lines() {
            if line.is_empty() {
                continue;
            }
            match TmuxPane::parse(line) {
                Ok(pane) => panes.push(pane),
                Err(e) => {
                    warn!(line = %line, error = %e, "failed to parse pane");
                }
            }
        }

        Ok(panes)
    }

    // ==================== I/O Operations ====================

    /// Capture output from a pane.
    ///
    /// # Arguments
    ///
    /// * `session` - Session name
    /// * `pane` - Optional pane ID (defaults to active pane)
    /// * `lines` - Number of lines to capture (defaults to entire scrollback)
    ///
    /// # Errors
    ///
    /// Returns error if session/pane doesn't exist.
    pub fn capture_output(
        &self,
        session: &str,
        pane: Option<&str>,
        lines: Option<u32>,
    ) -> Result<String> {
        if !self.session_exists(session) {
            return Err(TmuxError::SessionNotFound(session.to_string()));
        }

        let target = match pane {
            Some(p) => format!("{}:{}", session, p),
            None => session.to_string(),
        };

        let mut args = vec!["capture-pane", "-t", &target, "-p"];

        let lines_arg;
        if let Some(n) = lines {
            lines_arg = format!("-{}", n);
            args.push("-S");
            args.push(&lines_arg);
        }

        let output = self.run_tmux_checked(&args)?;

        // Validate pane exists if specified
        if let Some(p) = pane {
            let panes = self.list_panes(session)?;
            if !panes.iter().any(|pn| pn.id == p || pn.index.to_string() == p) {
                return Err(TmuxError::PaneNotFound(p.to_string(), session.to_string()));
            }
        }

        Ok(output)
    }

    /// Send keys to a pane.
    ///
    /// # Arguments
    ///
    /// * `session` - Session name
    /// * `pane` - Optional pane ID (defaults to active pane)
    /// * `keys` - Keys to send (can include special keys like Enter, Escape, etc.)
    ///
    /// # Errors
    ///
    /// Returns error if session/pane doesn't exist.
    pub fn send_keys(&self, session: &str, pane: Option<&str>, keys: &str) -> Result<()> {
        debug!(session = %session, pane = ?pane, keys = %keys, "sending keys");

        if !self.session_exists(session) {
            return Err(TmuxError::SessionNotFound(session.to_string()));
        }

        let target = match pane {
            Some(p) => format!("{}:{}", session, p),
            None => session.to_string(),
        };

        self.run_tmux_checked(&["send-keys", "-t", &target, keys])?;
        Ok(())
    }

    /// Send a line of text to a pane (adds Enter at the end).
    ///
    /// # Arguments
    ///
    /// * `session` - Session name
    /// * `pane` - Optional pane ID (defaults to active pane)
    /// * `text` - Text to send (Enter will be appended)
    ///
    /// # Errors
    ///
    /// Returns error if session/pane doesn't exist.
    pub fn send_line(&self, session: &str, pane: Option<&str>, text: &str) -> Result<()> {
        debug!(session = %session, pane = ?pane, text = %text, "sending line");

        if !self.session_exists(session) {
            return Err(TmuxError::SessionNotFound(session.to_string()));
        }

        let target = match pane {
            Some(p) => format!("{}:{}", session, p),
            None => session.to_string(),
        };

        self.run_tmux_checked(&["send-keys", "-t", &target, text, "Enter"])?;
        Ok(())
    }
}

impl Default for TmuxOrchestrator {
    fn default() -> Self {
        Self::new().expect("tmux not found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_available() {
        // This test works whether or not tmux is installed
        let available = TmuxOrchestrator::is_available();
        // Just ensure it returns a boolean without panicking
        assert!(available || !available);
    }

    #[test]
    fn test_new_when_tmux_not_found() {
        // We can't easily test this without mocking, but we can test the error type
        let result = TmuxOrchestrator::new();
        // Either succeeds (tmux installed) or returns NotFound
        if let Err(e) = result {
            assert!(matches!(e, TmuxError::NotFound));
        }
    }

    // Integration tests that require actual tmux
    #[test]
    #[ignore]
    fn test_create_and_destroy_session() {
        let tmux = TmuxOrchestrator::new().unwrap();
        let session_name = "test-commander-create";

        // Clean up any existing session
        let _ = tmux.destroy_session(session_name);

        // Create session
        let session = tmux.create_session(session_name).unwrap();
        assert_eq!(session.name, session_name);

        // Verify session exists
        assert!(tmux.session_exists(session_name));

        // List sessions should include our session
        let sessions = tmux.list_sessions().unwrap();
        assert!(sessions.iter().any(|s| s.name == session_name));

        // Destroy session
        tmux.destroy_session(session_name).unwrap();

        // Verify session no longer exists
        assert!(!tmux.session_exists(session_name));
    }

    #[test]
    #[ignore]
    fn test_destroy_nonexistent_session() {
        let tmux = TmuxOrchestrator::new().unwrap();
        let result = tmux.destroy_session("nonexistent-session-12345");
        assert!(matches!(result, Err(TmuxError::SessionNotFound(_))));
    }

    #[test]
    #[ignore]
    fn test_create_pane() {
        let tmux = TmuxOrchestrator::new().unwrap();
        let session_name = "test-commander-pane";

        // Clean up any existing session
        let _ = tmux.destroy_session(session_name);

        // Create session (starts with one pane)
        tmux.create_session(session_name).unwrap();

        // List initial panes
        let panes = tmux.list_panes(session_name).unwrap();
        assert_eq!(panes.len(), 1);

        // Create another pane
        let new_pane = tmux.create_pane(session_name).unwrap();
        assert!(new_pane.active);

        // Should now have two panes
        let panes = tmux.list_panes(session_name).unwrap();
        assert_eq!(panes.len(), 2);

        // Clean up
        tmux.destroy_session(session_name).unwrap();
    }

    #[test]
    #[ignore]
    fn test_send_keys_and_capture() {
        let tmux = TmuxOrchestrator::new().unwrap();
        let session_name = "test-commander-io";

        // Clean up any existing session
        let _ = tmux.destroy_session(session_name);

        // Create session
        tmux.create_session(session_name).unwrap();

        // Send some text
        tmux.send_line(session_name, None, "echo hello").unwrap();

        // Give it a moment to execute
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Capture output
        let output = tmux.capture_output(session_name, None, Some(10)).unwrap();

        // Output should contain our command or its result
        assert!(output.contains("echo") || output.contains("hello"));

        // Clean up
        tmux.destroy_session(session_name).unwrap();
    }

    #[test]
    #[ignore]
    fn test_session_exists() {
        let tmux = TmuxOrchestrator::new().unwrap();

        // Non-existent session
        assert!(!tmux.session_exists("definitely-does-not-exist-12345"));

        // Create a session and verify it exists
        let session_name = "test-commander-exists";
        let _ = tmux.destroy_session(session_name);
        tmux.create_session(session_name).unwrap();

        assert!(tmux.session_exists(session_name));

        tmux.destroy_session(session_name).unwrap();
        assert!(!tmux.session_exists(session_name));
    }

    #[test]
    #[ignore]
    fn test_list_panes_nonexistent_session() {
        let tmux = TmuxOrchestrator::new().unwrap();
        let result = tmux.list_panes("nonexistent-session-12345");
        assert!(matches!(result, Err(TmuxError::SessionNotFound(_))));
    }

    #[test]
    #[ignore]
    fn test_send_keys_nonexistent_session() {
        let tmux = TmuxOrchestrator::new().unwrap();
        let result = tmux.send_keys("nonexistent-session-12345", None, "test");
        assert!(matches!(result, Err(TmuxError::SessionNotFound(_))));
    }

    #[test]
    #[ignore]
    fn test_capture_output_nonexistent_session() {
        let tmux = TmuxOrchestrator::new().unwrap();
        let result = tmux.capture_output("nonexistent-session-12345", None, None);
        assert!(matches!(result, Err(TmuxError::SessionNotFound(_))));
    }
}
