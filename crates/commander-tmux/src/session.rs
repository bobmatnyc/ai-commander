//! Tmux session and pane data structures.

use chrono::{DateTime, TimeZone, Utc};

use crate::{Result, TmuxError};

/// Represents a tmux session.
#[derive(Debug, Clone)]
pub struct TmuxSession {
    /// Session name.
    pub name: String,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// Panes in this session.
    pub panes: Vec<TmuxPane>,
}

impl TmuxSession {
    /// Create a new TmuxSession.
    pub fn new(name: impl Into<String>, created_at: DateTime<Utc>) -> Self {
        Self {
            name: name.into(),
            created_at,
            panes: Vec::new(),
        }
    }

    /// Parse session from tmux list-sessions output line.
    ///
    /// Expected format: `session_name:created_timestamp`
    pub fn parse(line: &str) -> Result<Self> {
        let parts: Vec<&str> = line.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(TmuxError::ParseError(format!(
                "invalid session format: {}",
                line
            )));
        }

        let name = parts[0].to_string();
        let timestamp: i64 = parts[1].trim().parse().map_err(|_| {
            TmuxError::ParseError(format!("invalid timestamp: {}", parts[1]))
        })?;

        let created_at = Utc
            .timestamp_opt(timestamp, 0)
            .single()
            .ok_or_else(|| TmuxError::ParseError(format!("invalid timestamp: {}", timestamp)))?;

        Ok(Self {
            name,
            created_at,
            panes: Vec::new(),
        })
    }
}

/// Represents a pane within a tmux session.
#[derive(Debug, Clone)]
pub struct TmuxPane {
    /// Pane ID (e.g., "%0", "%1").
    pub id: String,
    /// Pane index within window.
    pub index: u32,
    /// Whether this pane is active.
    pub active: bool,
    /// Pane width in characters.
    pub width: u32,
    /// Pane height in characters.
    pub height: u32,
}

impl TmuxPane {
    /// Create a new TmuxPane.
    pub fn new(id: impl Into<String>, index: u32, active: bool, width: u32, height: u32) -> Self {
        Self {
            id: id.into(),
            index,
            active,
            width,
            height,
        }
    }

    /// Parse pane from tmux list-panes output line.
    ///
    /// Expected format: `pane_id:pane_index:pane_active:pane_width:pane_height`
    pub fn parse(line: &str) -> Result<Self> {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() != 5 {
            return Err(TmuxError::ParseError(format!(
                "invalid pane format: {}",
                line
            )));
        }

        let id = parts[0].to_string();
        let index: u32 = parts[1]
            .parse()
            .map_err(|_| TmuxError::ParseError(format!("invalid pane index: {}", parts[1])))?;
        let active = parts[2] == "1";
        let width: u32 = parts[3]
            .parse()
            .map_err(|_| TmuxError::ParseError(format!("invalid pane width: {}", parts[3])))?;
        let height: u32 = parts[4]
            .parse()
            .map_err(|_| TmuxError::ParseError(format!("invalid pane height: {}", parts[4])))?;

        Ok(Self {
            id,
            index,
            active,
            width,
            height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_valid() {
        let line = "mysession:1706000000";
        let session = TmuxSession::parse(line).unwrap();
        assert_eq!(session.name, "mysession");
        assert_eq!(session.created_at.timestamp(), 1706000000);
        assert!(session.panes.is_empty());
    }

    #[test]
    fn test_parse_session_with_colons_in_name() {
        // Session names can't have colons in tmux, so this should fail
        // because "session:1706000000" is not a valid timestamp
        let line = "my:session:1706000000";
        let result = TmuxSession::parse(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_session_invalid_format() {
        let line = "noseparator";
        let result = TmuxSession::parse(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_session_invalid_timestamp() {
        let line = "mysession:notanumber";
        let result = TmuxSession::parse(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pane_valid() {
        let line = "%0:0:1:120:40";
        let pane = TmuxPane::parse(line).unwrap();
        assert_eq!(pane.id, "%0");
        assert_eq!(pane.index, 0);
        assert!(pane.active);
        assert_eq!(pane.width, 120);
        assert_eq!(pane.height, 40);
    }

    #[test]
    fn test_parse_pane_inactive() {
        let line = "%1:1:0:80:24";
        let pane = TmuxPane::parse(line).unwrap();
        assert_eq!(pane.id, "%1");
        assert_eq!(pane.index, 1);
        assert!(!pane.active);
        assert_eq!(pane.width, 80);
        assert_eq!(pane.height, 24);
    }

    #[test]
    fn test_parse_pane_invalid_format() {
        let line = "%0:0:1:120"; // Missing height
        let result = TmuxPane::parse(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pane_invalid_index() {
        let line = "%0:abc:1:120:40";
        let result = TmuxPane::parse(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_session_new() {
        let session = TmuxSession::new("test", Utc::now());
        assert_eq!(session.name, "test");
        assert!(session.panes.is_empty());
    }

    #[test]
    fn test_pane_new() {
        let pane = TmuxPane::new("%0", 0, true, 120, 40);
        assert_eq!(pane.id, "%0");
        assert_eq!(pane.index, 0);
        assert!(pane.active);
        assert_eq!(pane.width, 120);
        assert_eq!(pane.height, 40);
    }

    #[test]
    fn test_parse_multiple_sessions() {
        let output = "session1:1706000000\nsession2:1706000001\nsession3:1706000002";
        let sessions: Vec<TmuxSession> = output
            .lines()
            .filter(|l| !l.is_empty())
            .map(TmuxSession::parse)
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].name, "session1");
        assert_eq!(sessions[1].name, "session2");
        assert_eq!(sessions[2].name, "session3");
    }

    #[test]
    fn test_parse_multiple_panes() {
        let output = "%0:0:1:120:40\n%1:1:0:120:40\n%2:2:0:120:40";
        let panes: Vec<TmuxPane> = output
            .lines()
            .filter(|l| !l.is_empty())
            .map(TmuxPane::parse)
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(panes.len(), 3);
        assert!(panes[0].active);
        assert!(!panes[1].active);
        assert!(!panes[2].active);
    }
}
