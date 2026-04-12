//! Message poller for the MPM messaging store.
//!
//! Polls `~/.claude-mpm/messaging.db` for unread messages and dispatches
//! them to the appropriate project sessions via tmux.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, params};
use tracing::{debug, info, warn};

use crate::error::{DaemonError, Result};

/// Unread message from the MPM messaging store.
#[derive(Debug, Clone)]
pub struct UnreadMessage {
    /// Unique message ID.
    pub id: i64,
    /// Project that sent the message.
    pub from_project: String,
    /// Destination project path or name.
    pub to_project: String,
    /// Message subject line.
    pub subject: String,
    /// Message priority level (e.g., "HIGH", "NORMAL").
    pub priority: String,
    /// ISO-8601 creation timestamp.
    pub created_at: String,
}

/// Polls the MPM SQLite messaging database for unread messages.
pub struct MessagePoller {
    db_path: PathBuf,
    /// Track IDs already dispatched this run to avoid duplicate dispatch.
    dispatched: HashSet<i64>,
}

impl MessagePoller {
    /// Creates a new poller pointing at the default MPM messaging database.
    ///
    /// The default path is `~/.claude-mpm/messaging.db`.
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            db_path,
            dispatched: HashSet::new(),
        }
    }

    /// Creates a poller using the default `~/.claude-mpm/messaging.db` path.
    pub fn with_default_path() -> Self {
        let db_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join(".claude-mpm")
            .join("messaging.db");
        Self::new(db_path)
    }

    /// Opens the database read-only.
    fn open_readonly(&self) -> Result<Connection> {
        Connection::open_with_flags(
            &self.db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| DaemonError::Configuration(format!("Failed to open messaging db: {}", e)))
    }

    /// Polls for unread messages, excluding already-dispatched IDs.
    ///
    /// Messages are returned ordered by priority descending, then creation
    /// time ascending (oldest high-priority messages first).
    pub fn poll_unread(&self) -> Result<Vec<UnreadMessage>> {
        if !self.db_path.exists() {
            debug!(path = %self.db_path.display(), "Messaging db not found, skipping poll");
            return Ok(Vec::new());
        }

        let conn = self.open_readonly()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, from_project, to_project, subject, priority, created_at \
                 FROM messages \
                 WHERE status = 'unread' \
                 ORDER BY \
                   CASE priority \
                     WHEN 'CRITICAL' THEN 0 \
                     WHEN 'HIGH'     THEN 1 \
                     WHEN 'NORMAL'   THEN 2 \
                     WHEN 'LOW'      THEN 3 \
                     ELSE 4 \
                   END ASC, \
                   created_at ASC",
            )
            .map_err(|e| DaemonError::Configuration(format!("Failed to prepare query: {}", e)))?;

        let messages: std::result::Result<Vec<UnreadMessage>, _> = stmt
            .query_map([], |row| {
                Ok(UnreadMessage {
                    id: row.get(0)?,
                    from_project: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    to_project: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    subject: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    priority: row.get::<_, Option<String>>(4)?.unwrap_or_else(|| "NORMAL".to_string()),
                    created_at: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                })
            })
            .map_err(|e| DaemonError::Configuration(format!("Failed to query messages: {}", e)))?
            .collect();

        let all = messages
            .map_err(|e| DaemonError::Configuration(format!("Failed to read message row: {}", e)))?;

        // Filter out messages already dispatched in this run.
        let filtered: Vec<UnreadMessage> = all
            .into_iter()
            .filter(|m| !self.dispatched.contains(&m.id))
            .collect();

        debug!(count = filtered.len(), "Polled unread messages");
        Ok(filtered)
    }

    /// Marks a message ID as dispatched so it is not dispatched again.
    pub fn mark_dispatched(&mut self, id: i64) {
        self.dispatched.insert(id);
    }

    /// Returns the total number of unread messages in the store (including
    /// already-dispatched ones from the current run).
    pub fn get_unread_count(&self) -> Result<u64> {
        if !self.db_path.exists() {
            return Ok(0);
        }

        let conn = self.open_readonly()?;
        let count: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE status = 'unread'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| DaemonError::Configuration(format!("Failed to count messages: {}", e)))?;

        Ok(count)
    }

    /// Returns unread messages addressed to a specific project path or name.
    pub fn get_unread_for_project(&self, project_path: &str) -> Result<Vec<UnreadMessage>> {
        if !self.db_path.exists() {
            return Ok(Vec::new());
        }

        let conn = self.open_readonly()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, from_project, to_project, subject, priority, created_at \
                 FROM messages \
                 WHERE status = 'unread' AND to_project = ?1 \
                 ORDER BY \
                   CASE priority \
                     WHEN 'CRITICAL' THEN 0 \
                     WHEN 'HIGH'     THEN 1 \
                     WHEN 'NORMAL'   THEN 2 \
                     WHEN 'LOW'      THEN 3 \
                     ELSE 4 \
                   END ASC, \
                   created_at ASC",
            )
            .map_err(|e| DaemonError::Configuration(format!("Failed to prepare query: {}", e)))?;

        let messages: std::result::Result<Vec<UnreadMessage>, _> = stmt
            .query_map(params![project_path], |row| {
                Ok(UnreadMessage {
                    id: row.get(0)?,
                    from_project: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    to_project: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    subject: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    priority: row.get::<_, Option<String>>(4)?.unwrap_or_else(|| "NORMAL".to_string()),
                    created_at: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                })
            })
            .map_err(|e| DaemonError::Configuration(format!("Failed to query messages: {}", e)))?
            .collect();

        messages
            .map_err(|e| DaemonError::Configuration(format!("Failed to read message row: {}", e)))
    }
}

// ---------------------------------------------------------------------------
// Async dispatch loop
// ---------------------------------------------------------------------------

/// Runs the message poller loop as a background daemon task.
///
/// For each unread message:
/// - Looks up the target project in the `StateStore`.
/// - If a tmux session exists and the session is idle, injects
///   `/mpm-message read` into the session pane.
/// - If no session exists, creates one and launches the project adapter.
/// - Marks the message as dispatched.
///
/// The loop runs every 30 seconds until the `shutdown` watch fires.
pub async fn run_message_poller(
    db_path: PathBuf,
    tmux: std::sync::Arc<commander_tmux::TmuxOrchestrator>,
    store: std::sync::Arc<commander_persistence::StateStore>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut poller = MessagePoller::new(db_path);
    let mut interval = tokio::time::interval(Duration::from_secs(30));

    info!("Message poller started");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                dispatch_messages(&mut poller, &tmux, &store).await;
            }
            _ = shutdown.changed() => {
                info!("Message poller shutting down");
                break;
            }
        }
    }
}

/// Inner dispatch function: polls and processes each unread message.
async fn dispatch_messages(
    poller: &mut MessagePoller,
    tmux: &commander_tmux::TmuxOrchestrator,
    store: &commander_persistence::StateStore,
) {
    let messages = match poller.poll_unread() {
        Ok(msgs) => msgs,
        Err(e) => {
            warn!(error = %e, "Failed to poll unread messages");
            return;
        }
    };

    if messages.is_empty() {
        return;
    }

    debug!(count = messages.len(), "Processing unread messages");

    for msg in messages {
        let to_project = msg.to_project.clone();
        let msg_id = msg.id;

        // Resolve the target project from the StateStore.
        // `to_project` may be a project name, alias, or filesystem path.
        let project = match find_project(store, &to_project) {
            Ok(Some(p)) => p,
            Ok(None) => {
                debug!(to_project = %to_project, "Target project not registered, skipping message");
                // Still mark dispatched to avoid infinite retry for unknown projects.
                poller.mark_dispatched(msg_id);
                continue;
            }
            Err(e) => {
                warn!(to_project = %to_project, error = %e, "StateStore lookup failed");
                continue;
            }
        };

        let session_name = project.name.replace([' ', '.', '/', ':'], "-");

        if tmux.session_exists(&session_name) {
            // Session is running — inject the mpm-message read command.
            info!(
                session = %session_name,
                msg_id = msg_id,
                "Injecting /mpm-message read into idle session"
            );
            if let Err(e) = tmux.send_line(&session_name, None, "/mpm-message read") {
                warn!(session = %session_name, error = %e, "Failed to inject /mpm-message read");
            }
        } else {
            // Session is not running — start it.
            info!(
                project = %project.name,
                msg_id = msg_id,
                "Starting project session for message delivery"
            );

            let tool_id = project
                .config
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("claude-code");

            match tmux.create_session_in_dir(&session_name, Some(&project.path)) {
                Ok(_) => {
                    // Launch the adapter and then inject the message read command.
                    let adapter_cmd = adapter_launch_cmd(tool_id, &project.path);
                    if let Err(e) = tmux.send_line(&session_name, None, &adapter_cmd) {
                        warn!(session = %session_name, error = %e, "Failed to launch adapter");
                    } else if let Err(e) = tmux.send_line(&session_name, None, "/mpm-message read") {
                        warn!(session = %session_name, error = %e, "Failed to inject /mpm-message read after start");
                    }
                }
                Err(e) => {
                    warn!(project = %project.name, error = %e, "Failed to create tmux session");
                    continue;
                }
            }
        }

        poller.mark_dispatched(msg_id);
    }
}

/// Looks up a project by name/alias or filesystem path.
///
/// First tries the StateStore's name/alias index.  If that misses, falls
/// back to scanning all projects by `project.path` in case `to_project`
/// is an absolute path stored directly in the messaging row.
fn find_project(
    store: &commander_persistence::StateStore,
    to_project: &str,
) -> commander_persistence::Result<Option<commander_models::Project>> {
    // Try name/alias match first (fast index scan).
    if let Some(p) = store.find_project_by_name_or_alias(to_project)? {
        return Ok(Some(p));
    }

    // Fall back to path match.
    let all = store.load_all_projects()?;
    Ok(all.into_values().find(|p| p.path == to_project))
}

/// Returns the shell command used to launch a given adapter.
fn adapter_launch_cmd(tool_id: &str, project_path: &str) -> String {
    match tool_id {
        "claude-code" | "cc" => format!("cd {} && claude", project_path),
        "claude-mpm" | "mpm" => format!("cd {} && claude-mpm", project_path),
        other => format!("cd {} && {}", project_path, other),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::NamedTempFile;

    /// Creates an in-memory-backed temp file with the messages table pre-populated.
    fn make_test_db(rows: &[(&str, &str, &str, &str, &str)]) -> NamedTempFile {
        let file = NamedTempFile::new().expect("tempfile");
        let conn = Connection::open(file.path()).expect("open");

        conn.execute_batch(
            "CREATE TABLE messages (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                from_project TEXT,
                to_project   TEXT,
                subject      TEXT,
                priority     TEXT DEFAULT 'NORMAL',
                status       TEXT NOT NULL DEFAULT 'unread',
                created_at   TEXT
            );",
        )
        .expect("create table");

        for (from, to, subject, priority, status) in rows {
            conn.execute(
                "INSERT INTO messages (from_project, to_project, subject, priority, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
                params![from, to, subject, priority, status],
            )
            .expect("insert");
        }

        file
    }

    #[test]
    fn poll_unread_returns_only_unread() {
        let db = make_test_db(&[
            ("proj-a", "proj-b", "Hello", "NORMAL", "unread"),
            ("proj-c", "proj-b", "Old msg", "NORMAL", "read"),
            ("proj-d", "proj-b", "Urgent", "HIGH", "unread"),
        ]);

        let poller = MessagePoller::new(db.path().to_path_buf());
        let msgs = poller.poll_unread().expect("poll");

        assert_eq!(msgs.len(), 2, "Expected 2 unread messages");
        // HIGH priority should come first.
        assert_eq!(msgs[0].priority, "HIGH");
        assert_eq!(msgs[1].priority, "NORMAL");
    }

    #[test]
    fn poll_unread_excludes_dispatched() {
        let db = make_test_db(&[
            ("proj-a", "proj-b", "Msg 1", "NORMAL", "unread"),
            ("proj-a", "proj-b", "Msg 2", "NORMAL", "unread"),
        ]);

        let mut poller = MessagePoller::new(db.path().to_path_buf());
        let first_poll = poller.poll_unread().expect("poll");
        assert_eq!(first_poll.len(), 2);

        // Mark the first message dispatched.
        let first_id = first_poll[0].id;
        poller.mark_dispatched(first_id);

        // Second poll should exclude the dispatched message.
        let second_poll = poller.poll_unread().expect("poll");
        assert_eq!(second_poll.len(), 1);
        assert_ne!(second_poll[0].id, first_id);
    }

    #[test]
    fn mark_dispatched_idempotent() {
        let db = make_test_db(&[("a", "b", "test", "NORMAL", "unread")]);
        let mut poller = MessagePoller::new(db.path().to_path_buf());

        poller.mark_dispatched(42);
        poller.mark_dispatched(42); // Should not panic.

        assert!(poller.dispatched.contains(&42));
    }

    #[test]
    fn get_unread_count_correct() {
        let db = make_test_db(&[
            ("a", "b", "m1", "NORMAL", "unread"),
            ("a", "b", "m2", "NORMAL", "unread"),
            ("a", "b", "m3", "NORMAL", "read"),
        ]);

        let poller = MessagePoller::new(db.path().to_path_buf());
        let count = poller.get_unread_count().expect("count");
        assert_eq!(count, 2);
    }

    #[test]
    fn get_unread_count_zero_when_db_missing() {
        let poller = MessagePoller::new(PathBuf::from("/tmp/nonexistent-messaging-db-test.db"));
        assert_eq!(poller.get_unread_count().unwrap(), 0);
    }

    #[test]
    fn get_unread_for_project_filters_correctly() {
        let db = make_test_db(&[
            ("a", "target-proj", "For target", "NORMAL", "unread"),
            ("a", "other-proj", "Not for target", "NORMAL", "unread"),
        ]);

        let poller = MessagePoller::new(db.path().to_path_buf());
        let msgs = poller.get_unread_for_project("target-proj").expect("query");

        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].to_project, "target-proj");
    }

    #[test]
    fn poll_unread_empty_when_db_missing() {
        let poller = MessagePoller::new(PathBuf::from("/tmp/nonexistent-messaging-db-test.db"));
        let msgs = poller.poll_unread().unwrap();
        assert!(msgs.is_empty());
    }
}
