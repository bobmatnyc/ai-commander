//! Runtime events.

use commander_models::{ProjectId, ProjectState};

/// Events emitted by the runtime.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// An instance was started.
    InstanceStarted {
        /// Project ID.
        project_id: ProjectId,
        /// Tmux session name.
        session: String,
    },
    /// An instance was stopped.
    InstanceStopped {
        /// Project ID.
        project_id: ProjectId,
    },
    /// Output was received from an instance.
    OutputReceived {
        /// Project ID.
        project_id: ProjectId,
        /// The output text.
        output: String,
    },
    /// The state of an instance changed.
    StateChanged {
        /// Project ID.
        project_id: ProjectId,
        /// New state.
        state: ProjectState,
    },
    /// An error occurred.
    Error {
        /// Project ID.
        project_id: ProjectId,
        /// Error message.
        error: String,
    },
}

impl RuntimeEvent {
    /// Returns the project ID associated with this event.
    pub fn project_id(&self) -> &ProjectId {
        match self {
            RuntimeEvent::InstanceStarted { project_id, .. } => project_id,
            RuntimeEvent::InstanceStopped { project_id } => project_id,
            RuntimeEvent::OutputReceived { project_id, .. } => project_id,
            RuntimeEvent::StateChanged { project_id, .. } => project_id,
            RuntimeEvent::Error { project_id, .. } => project_id,
        }
    }

    /// Returns true if this is an error event.
    pub fn is_error(&self) -> bool {
        matches!(self, RuntimeEvent::Error { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_project_id() {
        let project_id = ProjectId::from_string("test-project");

        let event = RuntimeEvent::InstanceStarted {
            project_id: project_id.clone(),
            session: "test-session".to_string(),
        };
        assert_eq!(event.project_id(), &project_id);

        let event = RuntimeEvent::InstanceStopped {
            project_id: project_id.clone(),
        };
        assert_eq!(event.project_id(), &project_id);

        let event = RuntimeEvent::OutputReceived {
            project_id: project_id.clone(),
            output: "test output".to_string(),
        };
        assert_eq!(event.project_id(), &project_id);

        let event = RuntimeEvent::StateChanged {
            project_id: project_id.clone(),
            state: ProjectState::Working,
        };
        assert_eq!(event.project_id(), &project_id);

        let event = RuntimeEvent::Error {
            project_id: project_id.clone(),
            error: "test error".to_string(),
        };
        assert_eq!(event.project_id(), &project_id);
    }

    #[test]
    fn test_event_is_error() {
        let project_id = ProjectId::from_string("test-project");

        let event = RuntimeEvent::InstanceStarted {
            project_id: project_id.clone(),
            session: "test-session".to_string(),
        };
        assert!(!event.is_error());

        let event = RuntimeEvent::Error {
            project_id,
            error: "test error".to_string(),
        };
        assert!(event.is_error());
    }
}
