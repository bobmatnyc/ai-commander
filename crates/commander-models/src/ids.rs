//! Type-safe ID wrappers for Commander.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Macro to generate ID newtypes with common functionality.
macro_rules! define_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a new random ID.
            pub fn new() -> Self {
                Self(format!("{}-{}", $prefix, Uuid::new_v4()))
            }

            /// Creates an ID from an existing string (for deserialization/testing).
            pub fn from_string(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            /// Returns the inner string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

define_id!(ProjectId, "proj");
define_id!(EventId, "evt");
define_id!(WorkId, "work");
define_id!(SessionId, "sess");
define_id!(MessageId, "msg");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_id_prefix() {
        let id = ProjectId::new();
        assert!(id.as_str().starts_with("proj-"));
    }

    #[test]
    fn test_event_id_prefix() {
        let id = EventId::new();
        assert!(id.as_str().starts_with("evt-"));
    }

    #[test]
    fn test_id_from_string() {
        let id = ProjectId::from_string("proj-custom-123");
        assert_eq!(id.as_str(), "proj-custom-123");
    }

    #[test]
    fn test_id_serialization() {
        let id = ProjectId::from_string("proj-test");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"proj-test\"");

        let parsed: ProjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_id_display() {
        let id = EventId::from_string("evt-123");
        assert_eq!(format!("{}", id), "evt-123");
    }

    #[test]
    fn test_ids_are_unique_types() {
        // This test documents that you can't accidentally assign wrong ID type
        // let project_id: ProjectId = ProjectId::new();
        // let event_id: EventId = project_id; // This would fail to compile!

        // They serialize the same way but are different types
        let p = ProjectId::from_string("test");
        let e = EventId::from_string("test");

        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            serde_json::to_string(&e).unwrap()
        );
        // But p != e won't even compile because they're different types
    }
}
