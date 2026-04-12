//! Usage tracking for Claude Max plan costs.
//!
//! Tracks cost per run, cumulative session/daily/weekly costs, and plan tier
//! by persisting `UsageRecord` entries to a JSON file under the Commander
//! state directory.

use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;

/// File name for persisted usage records.
const USAGE_FILE: &str = "usage_records.json";

/// Errors that can occur during usage tracking.
#[derive(Debug, Error)]
pub enum UsageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Subprocess error: {0}")]
    Subprocess(String),
    #[error("Failed to parse plan info: {0}")]
    ParsePlanInfo(String),
}

/// A single usage record from one session run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub project_name: String,
    pub session_id: String,
    pub cost_usd: f64,
    pub timestamp: DateTime<Utc>,
    pub adapter_type: String,
}

/// Aggregated usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageStats {
    pub session_total_usd: f64,
    pub today_total_usd: f64,
    pub week_total_usd: f64,
    pub all_time_total_usd: f64,
    pub run_count: u64,
    pub today_run_count: u64,
    pub week_run_count: u64,
}

/// Plan information from `claude auth status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanInfo {
    /// Subscription tier, e.g. "max", "pro", "free".
    pub subscription_type: String,
    pub email: Option<String>,
    pub checked_at: DateTime<Utc>,
}

/// Persisted state stored in `usage_records.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedState {
    records: Vec<UsageRecord>,
    plan_info: Option<PlanInfo>,
}

/// Usage tracker that persists records to a JSON file.
pub struct UsageTracker {
    records: Vec<UsageRecord>,
    plan_info: Option<PlanInfo>,
    storage_path: PathBuf,
}

impl UsageTracker {
    /// Load (or initialise) the tracker from `usage_records.json` inside `storage_dir`.
    ///
    /// If the file does not exist an empty tracker is returned.
    pub fn new(storage_dir: PathBuf) -> Self {
        let storage_path = storage_dir.join(USAGE_FILE);
        let state = Self::load_state(&storage_path).unwrap_or_default();
        Self {
            records: state.records,
            plan_info: state.plan_info,
            storage_path,
        }
    }

    fn load_state(path: &PathBuf) -> Result<PersistedState, UsageError> {
        if !path.exists() {
            return Ok(PersistedState::default());
        }
        let data = std::fs::read_to_string(path)?;
        let state = serde_json::from_str(&data)?;
        Ok(state)
    }

    fn save_state(&self) -> Result<(), UsageError> {
        // Ensure parent directory exists.
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let state = PersistedState {
            records: self.records.clone(),
            plan_info: self.plan_info.clone(),
        };
        let data = serde_json::to_string_pretty(&state)?;
        std::fs::write(&self.storage_path, data)?;
        Ok(())
    }

    /// Append a usage record and persist to disk.
    pub fn record_usage(&mut self, record: UsageRecord) -> Result<(), UsageError> {
        self.records.push(record);
        self.save_state()
    }

    /// Compute aggregated statistics.
    ///
    /// - `session_id`: when provided, restricts the session totals to records with
    ///   that session ID.
    /// - `today` = last 24 hours; `week` = last 7 days.
    pub fn get_stats(&self, session_id: Option<&str>) -> UsageStats {
        let now = Utc::now();
        let day_ago = now - Duration::hours(24);
        let week_ago = now - Duration::days(7);

        let mut stats = UsageStats::default();

        for record in &self.records {
            // All-time totals.
            stats.all_time_total_usd += record.cost_usd;
            stats.run_count += 1;

            // Today / this week.
            if record.timestamp >= day_ago {
                stats.today_total_usd += record.cost_usd;
                stats.today_run_count += 1;
            }
            if record.timestamp >= week_ago {
                stats.week_total_usd += record.cost_usd;
                stats.week_run_count += 1;
            }

            // Session totals (optional filter).
            if let Some(sid) = session_id {
                if record.session_id == sid {
                    stats.session_total_usd += record.cost_usd;
                }
            }
        }

        stats
    }

    /// Run `claude auth status --output-format json` and parse the response.
    ///
    /// The result is cached in memory and on disk.
    pub fn refresh_plan_info(&mut self) -> Result<PlanInfo, UsageError> {
        let output = std::process::Command::new("claude")
            .args(["auth", "status", "--output-format", "json"])
            .output()
            .map_err(|e| UsageError::Subprocess(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UsageError::Subprocess(format!(
                "claude auth status failed: {}",
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(stdout.trim())
            .map_err(|e| UsageError::ParsePlanInfo(format!("JSON parse: {e}")))?;

        let subscription_type = json
            .get("subscriptionType")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_lowercase();

        let email = json
            .get("email")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let plan = PlanInfo {
            subscription_type,
            email,
            checked_at: Utc::now(),
        };

        self.plan_info = Some(plan.clone());
        if let Err(e) = self.save_state() {
            warn!("Failed to persist plan info: {}", e);
        }

        Ok(plan)
    }

    /// Return the cached plan info without hitting the network.
    pub fn cached_plan_info(&self) -> Option<&PlanInfo> {
        self.plan_info.as_ref()
    }

    /// Render a human-readable usage report.
    pub fn format_report(&self) -> String {
        let stats = self.get_stats(None);

        // Plan line.
        let plan_line = match &self.plan_info {
            Some(info) => {
                let tier = capitalise(&info.subscription_type);
                match &info.email {
                    Some(email) => format!("Plan: {} ({})", tier, email),
                    None => format!("Plan: {}", tier),
                }
            }
            None => "Plan: unknown (run /usage to refresh)".to_string(),
        };

        // Last-checked timestamp.
        let checked_line = match &self.plan_info {
            Some(info) => format!(
                "Last checked: {}",
                info.checked_at.format("%Y-%m-%d %H:%M UTC")
            ),
            None => "Last checked: never".to_string(),
        };

        format!(
            "Usage Report\n\
             {plan_line}\n\
             {sep}\n\
             {hdr}\n\
             {sep}\n\
             Session:    {s_cost:>8.2}   {s_runs:>5}\n\
             Today:      {t_cost:>8.2}   {t_runs:>5}\n\
             This Week:  {w_cost:>8.2}   {w_runs:>5}\n\
             All Time:   {a_cost:>8.2}   {a_runs:>5}\n\
             {sep}\n\
             {checked_line}",
            sep = "─────────────────────────────────",
            hdr = "              Cost ($)    Runs",
            s_cost = stats.session_total_usd,
            s_runs = stats.run_count, // session filter not applied here (no session_id at report time)
            t_cost = stats.today_total_usd,
            t_runs = stats.today_run_count,
            w_cost = stats.week_total_usd,
            w_runs = stats.week_run_count,
            a_cost = stats.all_time_total_usd,
            a_runs = stats.run_count,
        )
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_record(session_id: &str, cost: f64, hours_ago: i64) -> UsageRecord {
        UsageRecord {
            project_name: "test-project".to_string(),
            session_id: session_id.to_string(),
            cost_usd: cost,
            timestamp: Utc::now() - Duration::hours(hours_ago),
            adapter_type: "claude-code".to_string(),
        }
    }

    #[test]
    fn test_empty_tracker_zero_stats() {
        let dir = TempDir::new().unwrap();
        let tracker = UsageTracker::new(dir.path().to_path_buf());
        let stats = tracker.get_stats(None);
        assert_eq!(stats.all_time_total_usd, 0.0);
        assert_eq!(stats.run_count, 0);
    }

    #[test]
    fn test_record_usage_persists() {
        let dir = TempDir::new().unwrap();
        let mut tracker = UsageTracker::new(dir.path().to_path_buf());
        tracker.record_usage(make_record("s1", 1.50, 0)).unwrap();
        tracker.record_usage(make_record("s1", 0.50, 0)).unwrap();

        // Reload from disk.
        let tracker2 = UsageTracker::new(dir.path().to_path_buf());
        let stats = tracker2.get_stats(None);
        assert!((stats.all_time_total_usd - 2.0).abs() < 1e-9);
        assert_eq!(stats.run_count, 2);
    }

    #[test]
    fn test_today_and_week_windows() {
        let dir = TempDir::new().unwrap();
        let mut tracker = UsageTracker::new(dir.path().to_path_buf());
        // Within today (1 hour ago).
        tracker.record_usage(make_record("s1", 1.0, 1)).unwrap();
        // Within week but not today (48 hours ago).
        tracker.record_usage(make_record("s1", 2.0, 48)).unwrap();
        // Older than a week (200 hours ago).
        tracker.record_usage(make_record("s1", 4.0, 200)).unwrap();

        let stats = tracker.get_stats(None);
        assert!((stats.today_total_usd - 1.0).abs() < 1e-9);
        assert!((stats.week_total_usd - 3.0).abs() < 1e-9);
        assert!((stats.all_time_total_usd - 7.0).abs() < 1e-9);
        assert_eq!(stats.today_run_count, 1);
        assert_eq!(stats.week_run_count, 2);
    }

    #[test]
    fn test_session_filter() {
        let dir = TempDir::new().unwrap();
        let mut tracker = UsageTracker::new(dir.path().to_path_buf());
        tracker.record_usage(make_record("session-a", 1.0, 0)).unwrap();
        tracker.record_usage(make_record("session-b", 5.0, 0)).unwrap();

        let stats = tracker.get_stats(Some("session-a"));
        assert!((stats.session_total_usd - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_format_report_contains_headers() {
        let dir = TempDir::new().unwrap();
        let tracker = UsageTracker::new(dir.path().to_path_buf());
        let report = tracker.format_report();
        assert!(report.contains("Usage Report"));
        assert!(report.contains("Session:"));
        assert!(report.contains("Today:"));
        assert!(report.contains("This Week:"));
        assert!(report.contains("All Time:"));
    }

    #[test]
    fn test_capitalise() {
        assert_eq!(capitalise("max"), "Max");
        assert_eq!(capitalise("pro"), "Pro");
        assert_eq!(capitalise(""), "");
    }
}
