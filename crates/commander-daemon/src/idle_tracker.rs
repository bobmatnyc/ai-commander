//! Idle session tracker for auto-pausing MPM sessions.
//!
//! Monitors tmux pane output changes for each tracked session. When a session
//! has no output change for longer than `pause_threshold`, it is returned as a
//! candidate for pausing. The pause flag is set once so each session is only
//! paused a single time until activity resumes.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// Tracks idleness of tmux sessions.
pub struct IdleTracker {
    /// Per-session idle state, keyed by session name.
    sessions: HashMap<String, IdleInfo>,
    /// Time without output change before a session is considered idle.
    pause_threshold: Duration,
}

/// Per-session idle tracking state.
struct IdleInfo {
    /// When we last detected a change in output.
    last_activity: Instant,
    /// Hash of the last captured output — used to detect changes.
    last_output_hash: u64,
    /// True once we have sent `/mpm-session-pause` for this idle period.
    already_paused: bool,
}

impl IdleTracker {
    /// Create a new tracker with the given pause threshold.
    pub fn new(pause_threshold: Duration) -> Self {
        Self {
            sessions: HashMap::new(),
            pause_threshold,
        }
    }

    /// Register a session for idle tracking.
    ///
    /// The session starts with `last_activity = now` so it will not be
    /// considered idle until at least `pause_threshold` has elapsed.
    pub fn track_session(&mut self, session: &str) {
        self.sessions.entry(session.to_string()).or_insert_with(|| IdleInfo {
            last_activity: Instant::now(),
            last_output_hash: 0,
            already_paused: false,
        });
    }

    /// Remove a session from tracking.
    pub fn untrack_session(&mut self, session: &str) {
        self.sessions.remove(session);
    }

    /// Update activity state based on the latest captured output.
    ///
    /// Returns `true` if the output changed since the last call, `false`
    /// if it is the same (session is quiet).
    ///
    /// If the output *changed*, the `already_paused` flag is reset so the
    /// session can be paused again if it goes idle a second time.
    pub fn update_activity(&mut self, session: &str, output: &str) -> bool {
        let hash = compute_hash(output);

        let changed = if let Some(info) = self.sessions.get_mut(session) {
            if info.last_output_hash != hash {
                info.last_output_hash = hash;
                info.last_activity = Instant::now();
                // Output moved — allow a future pause if it goes idle again.
                self.reset_if_active(session);
                true
            } else {
                false
            }
        } else {
            // Session not tracked; insert it and treat this as the baseline.
            self.sessions.insert(
                session.to_string(),
                IdleInfo {
                    last_activity: Instant::now(),
                    last_output_hash: hash,
                    already_paused: false,
                },
            );
            true
        };

        changed
    }

    /// Return the names of sessions that are idle and have not been paused yet.
    ///
    /// Each returned session has its `already_paused` flag set so it will
    /// not be returned again until activity is detected.
    pub fn check_idle(&mut self) -> Vec<String> {
        let threshold = self.pause_threshold;
        let mut to_pause = Vec::new();

        for (name, info) in &mut self.sessions {
            if !info.already_paused && info.last_activity.elapsed() >= threshold {
                info.already_paused = true;
                to_pause.push(name.clone());
            }
        }

        to_pause
    }

    /// Reset the `already_paused` flag for a session when it becomes active.
    ///
    /// Called internally by `update_activity` when output changes.
    fn reset_if_active(&mut self, session: &str) {
        if let Some(info) = self.sessions.get_mut(session) {
            info.already_paused = false;
        }
    }
}

/// Compute a 64-bit hash of the given string using `DefaultHasher`.
fn compute_hash(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ============================================================
// Async monitor task
// ============================================================

use std::sync::Arc;

use tokio::sync::watch;
use tracing::{debug, info, warn};

use commander_persistence::StateStore;
use commander_tmux::TmuxOrchestrator;
use commander_models::project::AdapterType;

/// Spawnable task that monitors MPM sessions for idleness.
///
/// Every 60 seconds it:
/// 1. Loads all persisted projects.
/// 2. Keeps the tracker's session set in sync (track new, untrack removed).
/// 3. Captures tmux output for each `ClaudeMpm` session.
/// 4. Calls `check_idle()` and sends `/mpm-session-pause` to idle sessions.
///
/// The task exits when `shutdown` fires.
pub async fn run_idle_monitor(
    tmux: Arc<TmuxOrchestrator>,
    store: Arc<StateStore>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut tracker = IdleTracker::new(Duration::from_secs(900));
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    info!("Idle monitor started (threshold: 15 minutes)");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                tick_once(&tmux, &store, &mut tracker).await;
            }
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    info!("Idle monitor shutting down");
                    break;
                }
            }
        }
    }
}

/// Execute a single monitoring tick.
async fn tick_once(
    tmux: &TmuxOrchestrator,
    store: &StateStore,
    tracker: &mut IdleTracker,
) {
    // Load all persisted projects.
    let projects = match store.load_all_projects() {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "idle_monitor: failed to load projects");
            return;
        }
    };

    // Build the set of MPM session names currently configured.
    let mpm_sessions: Vec<String> = projects
        .values()
        .filter(|p| p.adapter_type == Some(AdapterType::ClaudeMpm))
        .map(|p| p.session_name())
        .collect();

    // Sync tracker: untrack sessions that are no longer MPM projects.
    let tracked: Vec<String> = {
        // We need a snapshot because we borrow tracker mutably below.
        tracker.sessions.keys().cloned().collect()
    };
    for name in &tracked {
        if !mpm_sessions.contains(name) {
            tracker.untrack_session(name);
            debug!(session = %name, "idle_monitor: untracked removed session");
        }
    }

    // Ensure new MPM sessions are tracked.
    for name in &mpm_sessions {
        tracker.track_session(name);
    }

    // Capture output and update activity for each MPM session.
    for name in &mpm_sessions {
        if !tmux.session_exists(name) {
            debug!(session = %name, "idle_monitor: tmux session does not exist, skipping");
            continue;
        }

        match tmux.capture_output(name, None, Some(50)) {
            Ok(output) => {
                let changed = tracker.update_activity(name, &output);
                debug!(session = %name, changed = changed, "idle_monitor: activity update");
            }
            Err(e) => {
                warn!(session = %name, error = %e, "idle_monitor: failed to capture output");
            }
        }
    }

    // Check which sessions have gone idle and need pausing.
    let to_pause = tracker.check_idle();
    for name in to_pause {
        info!(session = %name, "idle_monitor: session idle for 15 min, sending /mpm-session-pause");
        if let Err(e) = tmux.send_line(&name, None, "/mpm-session-pause") {
            warn!(session = %name, error = %e, "idle_monitor: failed to send pause command");
        }
    }
}

// ============================================================
// Unit tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tracker(threshold_ms: u64) -> IdleTracker {
        IdleTracker::new(Duration::from_millis(threshold_ms))
    }

    #[test]
    fn track_then_check_immediately_returns_empty() {
        let mut tracker = make_tracker(1000);
        tracker.track_session("alpha");
        // Not enough time has passed.
        assert!(tracker.check_idle().is_empty());
    }

    #[test]
    fn idle_after_threshold_returns_session() {
        // Use a 1 ms threshold so we can cross it without sleeping.
        let mut tracker = make_tracker(1);
        tracker.track_session("beta");

        // Manually back-date last_activity so the threshold is exceeded.
        if let Some(info) = tracker.sessions.get_mut("beta") {
            info.last_activity = Instant::now() - Duration::from_millis(10);
        }

        let idle = tracker.check_idle();
        assert_eq!(idle, vec!["beta"]);
    }

    #[test]
    fn already_paused_not_returned_again() {
        let mut tracker = make_tracker(1);
        tracker.track_session("gamma");

        if let Some(info) = tracker.sessions.get_mut("gamma") {
            info.last_activity = Instant::now() - Duration::from_millis(10);
        }

        // First check: should return gamma.
        let first = tracker.check_idle();
        assert_eq!(first, vec!["gamma"]);

        // Second check: already_paused is set, should be empty.
        let second = tracker.check_idle();
        assert!(second.is_empty());
    }

    #[test]
    fn update_activity_resets_timer_and_pause_flag() {
        let mut tracker = make_tracker(1);
        tracker.track_session("delta");

        // Back-date so the session is idle.
        if let Some(info) = tracker.sessions.get_mut("delta") {
            info.last_activity = Instant::now() - Duration::from_millis(10);
        }

        // Pause it.
        let first = tracker.check_idle();
        assert_eq!(first, vec!["delta"]);

        // New output arrives — should reset the timer and pause flag.
        tracker.update_activity("delta", "new output");

        // Threshold not exceeded again yet.
        assert!(tracker.check_idle().is_empty());

        // Back-date again to simulate another idle period.
        if let Some(info) = tracker.sessions.get_mut("delta") {
            info.last_activity = Instant::now() - Duration::from_millis(10);
        }

        // Should be paused a second time.
        let second = tracker.check_idle();
        assert_eq!(second, vec!["delta"]);
    }

    #[test]
    fn untrack_removes_session() {
        let mut tracker = make_tracker(1);
        tracker.track_session("epsilon");
        tracker.untrack_session("epsilon");

        // Back-dating is irrelevant — session should not appear.
        assert!(tracker.check_idle().is_empty());
        assert!(!tracker.sessions.contains_key("epsilon"));
    }

    #[test]
    fn update_activity_returns_true_on_change() {
        let mut tracker = make_tracker(1000);
        tracker.track_session("zeta");
        // First call with new hash — treated as change.
        tracker.update_activity("zeta", "initial output");
        // Same output — no change.
        assert!(!tracker.update_activity("zeta", "initial output"));
        // Different output — change detected.
        assert!(tracker.update_activity("zeta", "different output"));
    }
}
