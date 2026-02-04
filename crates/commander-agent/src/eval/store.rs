//! Persistent storage for feedback entries.

use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

use crate::error::{AgentError, Result};

use super::types::{Feedback, FeedbackType};

/// Persistent storage for feedback entries.
pub struct FeedbackStore {
    /// Directory for storing feedback data.
    path: PathBuf,
    /// In-memory cache of feedback entries.
    entries: Vec<Feedback>,
}

impl FeedbackStore {
    /// Create a new feedback store at the specified path.
    pub fn new(path: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&path).map_err(|e| {
            AgentError::Configuration(format!(
                "Failed to create feedback directory {}: {}",
                path.display(),
                e
            ))
        })?;

        let mut store = Self {
            path,
            entries: Vec::new(),
        };

        store.load()?;
        Ok(store)
    }

    /// Add a feedback entry.
    pub async fn add(&mut self, feedback: Feedback) -> Result<()> {
        info!(
            id = %feedback.id,
            agent_id = %feedback.agent_id,
            feedback_type = %feedback.feedback_type,
            "Recording feedback"
        );

        self.entries.push(feedback);
        self.save()
    }

    /// Get recent feedback for a specific agent.
    pub async fn get_recent(&self, agent_id: &str, limit: usize) -> Vec<&Feedback> {
        self.entries
            .iter()
            .filter(|f| f.agent_id == agent_id)
            .rev() // Most recent first
            .take(limit)
            .collect()
    }

    /// Get feedback by type.
    pub async fn get_by_type(&self, feedback_type: FeedbackType, limit: usize) -> Vec<&Feedback> {
        self.entries
            .iter()
            .filter(|f| f.feedback_type == feedback_type)
            .rev()
            .take(limit)
            .collect()
    }

    /// Get all feedback for an agent.
    pub fn get_all(&self, agent_id: &str) -> Vec<&Feedback> {
        self.entries
            .iter()
            .filter(|f| f.agent_id == agent_id)
            .collect()
    }

    /// Count feedback by type for an agent.
    pub fn count_by_type(&self, agent_id: &str) -> HashMap<FeedbackType, usize> {
        let mut counts = HashMap::new();

        for feedback in self.entries.iter().filter(|f| f.agent_id == agent_id) {
            *counts.entry(feedback.feedback_type.clone()).or_insert(0) += 1;
        }

        counts
    }

    /// Save feedback to disk.
    pub fn save(&self) -> Result<()> {
        let file = self.data_file();
        let json = serde_json::to_string_pretty(&self.entries)?;

        // Atomic write via temp file
        let temp_file = file.with_extension("json.tmp");
        std::fs::write(&temp_file, &json).map_err(|e| {
            AgentError::Configuration(format!("Failed to write feedback: {}", e))
        })?;
        std::fs::rename(&temp_file, &file).map_err(|e| {
            AgentError::Configuration(format!("Failed to save feedback: {}", e))
        })?;

        debug!(count = self.entries.len(), "Saved feedback to disk");
        Ok(())
    }

    /// Load feedback from disk.
    pub fn load(&mut self) -> Result<()> {
        let file = self.data_file();
        if !file.exists() {
            debug!(path = %file.display(), "No existing feedback file");
            return Ok(());
        }

        let data = std::fs::read_to_string(&file).map_err(|e| {
            AgentError::Configuration(format!("Failed to read feedback: {}", e))
        })?;

        self.entries = serde_json::from_str(&data)?;
        info!(count = self.entries.len(), "Loaded feedback from disk");
        Ok(())
    }

    fn data_file(&self) -> PathBuf {
        self.path.join("feedback.json")
    }
}
