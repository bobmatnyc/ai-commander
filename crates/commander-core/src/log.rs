//! Persistent session summary log.
//!
//! Why: The summary/interpreted view in the GUI is ephemeral — when a session
//! is re-opened the user has no record of what happened before. We persist a
//! rolling JSONL log of summarized screen snapshots per session so history
//! survives across polling cycles, app restarts, and tmux detach/reattach.
//!
//! What: Append-only `~/.ai-commander/logs/<session>/YYYY-MM-DD.jsonl` files.
//! Each line is `{"ts", "text", "hash"}`. Writes are deduplicated by content
//! hash *and* by trimmed-text comparison with the last entry — so spurious
//! churn (ANSI redraws, progress bars) does not bloat the log.
//!
//! Test: Call `append_log_entry("sess", "hello", "h1")` twice with identical
//! args; second call returns `Ok(false)`. Then read with `read_entries` and
//! assert exactly one entry is present.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// A single summarized log entry for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Unix epoch seconds when this entry was written.
    pub ts: i64,
    /// The summarized/cleaned text chunk.
    pub text: String,
    /// u64 hash of the raw content this was derived from, hex-encoded.
    pub hash: String,
    /// Entry kind: "llm" (interpretation, default) or "user" (message sent by user).
    ///
    /// Why: User-typed messages need to be distinguished from LLM
    /// interpretations on replay so the GUI can render them with the correct
    /// `direction` (sent vs received). Older entries omit this field; absence
    /// is treated as "llm" for backwards compatibility.
    /// What: Optional string tag persisted in JSONL.
    /// Test: Round-trip a `LogEntry { kind: Some("user"), .. }` through serde
    /// and assert the JSON line contains `"kind":"user"`; round-trip a legacy
    /// entry without the field and assert deserialization succeeds with
    /// `kind == None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Returns the directory that holds log files for a given session.
///
/// Why: Centralizes the path convention so every caller produces the same
/// directory; required for archive + read paths to agree.
/// What: `~/.ai-commander/logs/<session>/`.
/// Test: Set `HOME=/tmp/fake`, call `log_dir_for("s1")`, assert the result
/// equals `/tmp/fake/.ai-commander/logs/s1`.
pub fn log_dir_for(session: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".ai-commander/logs")
        .join(sanitize(session))
}

/// Path to today's log file for a session.
fn today_file(session: &str) -> PathBuf {
    log_dir_for(session).join(format!("{}.jsonl", Local::now().format("%Y-%m-%d")))
}

/// Sanitize a session name for use as a directory component.
/// tmux session names are already constrained, but defensively strip `/` and
/// `..` so a crafted name cannot escape the logs directory.
fn sanitize(session: &str) -> String {
    session.replace(['/', '\\'], "_").replace("..", "_")
}

/// Append a summary entry for `session`. Returns `Ok(true)` if written,
/// `Ok(false)` if skipped as a duplicate.
///
/// Why: Stateless append is simpler than a per-session writer handle and log
/// writes are infrequent (every few seconds at most per session). Reading the
/// last line of today's file is cheap and keeps dedup correct across restarts.
/// What: Dedups vs the last line in today's file by hash AND by trimmed text.
/// Creates `~/.ai-commander/logs/<session>/` if missing. Appends a JSON line.
/// Test: Append twice with the same `(text, hash)`; second returns `Ok(false)`.
/// Append a different text with the same hash — second returns `Ok(true)`
/// only if trimmed text differs.
pub fn append_log_entry(session: &str, text: &str, hash: &str) -> std::io::Result<bool> {
    let text_trim = text.trim();
    if text_trim.is_empty() {
        return Ok(false);
    }

    let dir = log_dir_for(session);
    fs::create_dir_all(&dir)?;

    let path = today_file(session);

    // Dedup against the last entry in today's file.
    if let Some(last) = read_last_entry(&path) {
        if last.hash == hash || last.text.trim() == text_trim {
            return Ok(false);
        }
    }

    let entry = LogEntry {
        ts: chrono::Utc::now().timestamp(),
        text: text.to_string(),
        hash: hash.to_string(),
        kind: None,
    };
    let line = serde_json::to_string(&entry)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{}", line)?;
    Ok(true)
}

/// Append a user-message entry for `session`.
///
/// Why: User messages typed via the GUI/web input are part of the session's
/// conversation history but were previously only kept in volatile Svelte
/// state, vanishing when the user switched sessions. Persisting them to the
/// same JSONL log lets `read_entries` replay both LLM summaries and user
/// inputs in order, restoring the full chat on session re-open.
/// What: Writes a `LogEntry { kind: Some("user"), .. }` with a hash derived
/// from the text and a unique-enough timestamp suffix so legitimate repeats
/// (e.g. user sends "yes" twice) are NOT deduplicated. Returns Ok(()) on
/// success — this never silently drops the entry the way `append_log_entry`
/// does for summaries.
/// Test: Call twice with identical text; read today's entries and assert two
/// `kind == Some("user")` entries are present.
pub fn append_user_message(session: &str, text: &str) -> std::io::Result<()> {
    let text_trim = text.trim();
    if text_trim.is_empty() {
        return Ok(());
    }

    let dir = log_dir_for(session);
    fs::create_dir_all(&dir)?;
    let path = today_file(session);

    let ts = chrono::Utc::now().timestamp();
    // Hash text+timestamp so identical user messages are never deduplicated
    // against each other (unlike LLM summaries, where dedup is desirable).
    let hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        ts.hash(&mut h);
        format!("{:x}", h.finish())
    };

    let entry = LogEntry {
        ts,
        text: text.to_string(),
        hash,
        kind: Some("user".to_string()),
    };
    let line = serde_json::to_string(&entry)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{}", line)?;
    Ok(())
}

/// Read the last entry from a jsonl file, if any.
fn read_last_entry(path: &PathBuf) -> Option<LogEntry> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut last = None;
    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
            last = Some(entry);
        }
    }
    last
}

/// Read all log entries for a session on a given date (YYYY-MM-DD).
///
/// Why: The GUI needs to replay today's (or a past day's) summaries when a
/// session is opened so users see what happened before they arrived.
/// What: Parses `~/.ai-commander/logs/<session>/<date>.jsonl`. Returns empty
/// vec if the file doesn't exist or is malformed.
/// Test: Write two valid entries and one garbage line to the target file;
/// assert `read_entries(session, date).len() == 2`.
pub fn read_entries(session: &str, date: &str) -> Vec<LogEntry> {
    let path = log_dir_for(session).join(format!("{}.jsonl", date));
    let Ok(file) = fs::File::open(&path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<LogEntry>(&line).ok())
        .collect()
}

/// Read all log entries for a session across every recorded date.
///
/// Why: The GUI and web clients need the complete chat history on connect so
/// users see prior work immediately — not just today's entries. `read_entries`
/// is per-day, so without this helper each caller would re-enumerate dates.
/// What: Enumerates `list_dates(session)`, reads each file, concatenates the
/// entries, then sorts by timestamp ascending. Returns an empty vec if the
/// session has no log directory.
/// Test: Seed two date files with one entry each under a fake HOME, call this,
/// assert the returned vec has two entries sorted by `ts`.
pub fn read_all_entries(session: &str) -> std::io::Result<Vec<LogEntry>> {
    let dates = list_dates(session);
    let mut all = Vec::new();
    for date in dates {
        let entries = read_entries(session, &date);
        all.extend(entries);
    }
    all.sort_by_key(|e| e.ts);
    Ok(all)
}

/// List the dates (YYYY-MM-DD) for which log files exist for a session.
///
/// Why: The archive UI and future date-picker UX need to know which dates
/// have content without enumerating every day.
/// What: Scans `~/.ai-commander/logs/<session>/*.jsonl`, strips the extension,
/// and returns the stems sorted ascending.
/// Test: Create files `2026-01-01.jsonl` and `2026-02-01.jsonl`; assert
/// `list_dates(session) == vec!["2026-01-01", "2026-02-01"]`.
pub fn list_dates(session: &str) -> Vec<String> {
    let dir = log_dir_for(session);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut dates: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();
    dates.sort();
    dates
}

/// Archive all logs for a session into a single zip.
///
/// Why: Users want to export/snapshot a session's entire summary history —
/// e.g. for sharing a ticket write-up or offloading before deleting the
/// session — in one file.
/// What: Writes to `~/.ai-commander/logs/archive/<session>-<timestamp>.zip`
/// using the system `zip` CLI (available on macOS by default; Linux needs
/// `zip` installed). Returns the archive's absolute path.
/// Test: Seed `~/.ai-commander/logs/sess/<today>.jsonl`, call
/// `archive_session_logs("sess")`, assert the returned path exists and has
/// non-zero size.
pub fn archive_session_logs(session: &str) -> std::io::Result<PathBuf> {
    let source = log_dir_for(session);
    if !source.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no logs for session {}", session),
        ));
    }

    let archive_dir = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".ai-commander/logs/archive");
    fs::create_dir_all(&archive_dir)?;

    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let archive_path = archive_dir.join(format!("{}-{}.zip", sanitize(session), ts));

    // Shell out to `zip -r <archive> <dir>`. `-j` would junk paths; we want
    // relative paths preserved so the archive self-describes the session.
    let parent = source.parent().unwrap_or(&source);
    let dir_name = source
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(session);

    let status = std::process::Command::new("zip")
        .arg("-r")
        .arg(&archive_path)
        .arg(dir_name)
        .current_dir(parent)
        .output()?;

    if !status.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "zip failed: {}",
                String::from_utf8_lossy(&status.stderr)
            ),
        ));
    }

    Ok(archive_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // HOME is a process-global env var; serialize tests that mutate it so
    // parallel test execution doesn't race and corrupt each other's dirs.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    fn with_tmp_home<F: FnOnce()>(f: F) {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var("HOME").ok();
        std::env::set_var("HOME", tmp.path());
        f();
        if let Some(p) = prev {
            std::env::set_var("HOME", p);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn dedup_by_hash_and_text() {
        with_tmp_home(|| {
            let written = append_log_entry("s1", "hello", "h1").unwrap();
            assert!(written);
            // Same hash → dedup
            let dup = append_log_entry("s1", "hello", "h1").unwrap();
            assert!(!dup);
            // Different hash, same trimmed text → dedup
            let dup2 = append_log_entry("s1", "hello", "h2").unwrap();
            assert!(!dup2);
            // Different text → written
            let new_entry = append_log_entry("s1", "hello 2", "h2").unwrap();
            assert!(new_entry);
        });
    }

    #[test]
    fn empty_text_skipped() {
        with_tmp_home(|| {
            let written = append_log_entry("s1", "   \n  ", "h1").unwrap();
            assert!(!written);
        });
    }

    #[test]
    fn read_entries_skips_garbage() {
        with_tmp_home(|| {
            append_log_entry("s1", "one", "h1").unwrap();
            append_log_entry("s1", "two", "h2").unwrap();
            // Append a junk line
            let path = today_file("s1");
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(f, "not json").unwrap();

            let date = Local::now().format("%Y-%m-%d").to_string();
            let entries = read_entries("s1", &date);
            assert_eq!(entries.len(), 2);
        });
    }

    #[test]
    fn list_dates_sorted() {
        with_tmp_home(|| {
            let dir = log_dir_for("s2");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("2026-02-01.jsonl"), "").unwrap();
            fs::write(dir.join("2026-01-01.jsonl"), "").unwrap();
            fs::write(dir.join("not-a-log.txt"), "").unwrap();
            let dates = list_dates("s2");
            assert_eq!(dates, vec!["2026-01-01", "2026-02-01"]);
        });
    }
}
