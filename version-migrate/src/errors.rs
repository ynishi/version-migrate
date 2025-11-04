//! Error types for migration operations.

use std::fmt;
use thiserror::Error;

/// File I/O operation kind.
///
/// Identifies the specific type of I/O operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOperationKind {
    /// Reading from a file
    Read,
    /// Writing to a file
    Write,
    /// Creating a new file
    Create,
    /// Deleting a file
    Delete,
    /// Renaming/moving a file
    Rename,
    /// Creating a directory
    CreateDir,
    /// Reading directory contents
    ReadDir,
    /// Syncing file contents to disk
    Sync,
}

impl fmt::Display for IoOperationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Write => write!(f, "write"),
            Self::Create => write!(f, "create"),
            Self::Delete => write!(f, "delete"),
            Self::Rename => write!(f, "rename"),
            Self::CreateDir => write!(f, "create directory"),
            Self::ReadDir => write!(f, "read directory"),
            Self::Sync => write!(f, "sync"),
        }
    }
}

/// Format I/O error message with operation, path, context, and error details.
fn format_io_error(
    operation: &IoOperationKind,
    path: &str,
    context: &Option<String>,
    error: &str,
) -> String {
    if let Some(ctx) = context {
        format!("Failed to {} {} at '{}': {}", operation, ctx, path, error)
    } else {
        format!("Failed to {} file at '{}': {}", operation, path, error)
    }
}

/// Error types that can occur during migration operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum MigrationError {
    /// Failed to deserialize the data.
    #[error("Failed to deserialize: {0}")]
    DeserializationError(String),

    /// Failed to serialize the data.
    #[error("Failed to serialize: {0}")]
    SerializationError(String),

    /// The requested entity type was not found in the migrator.
    #[error("Entity '{0}' not found")]
    EntityNotFound(String),

    /// No migration path is defined for the given entity and version.
    #[error("No migration path defined for entity '{entity}' version '{version}'")]
    MigrationPathNotDefined {
        /// The entity name.
        entity: String,
        /// The version that has no migration path.
        version: String,
    },

    /// A migration step failed during execution.
    #[error("Migration failed from '{from}' to '{to}': {error}")]
    MigrationStepFailed {
        /// The source version.
        from: String,
        /// The target version.
        to: String,
        /// The error message.
        error: String,
    },

    /// A circular migration path was detected.
    #[error("Circular migration path detected in entity '{entity}': {path}")]
    CircularMigrationPath {
        /// The entity name.
        entity: String,
        /// The path that forms a cycle.
        path: String,
    },

    /// Version ordering is invalid (not following semver rules).
    #[error("Invalid version order in entity '{entity}': '{from}' -> '{to}' (versions must increase according to semver)")]
    InvalidVersionOrder {
        /// The entity name.
        entity: String,
        /// The source version.
        from: String,
        /// The target version.
        to: String,
    },

    /// File I/O error with detailed operation context.
    ///
    /// Provides specific information about which I/O operation failed,
    /// along with optional context (e.g., "temporary file", "after 3 retries").
    #[error("{}", format_io_error(.operation, .path, .context, .error))]
    IoError {
        /// The I/O operation that failed.
        operation: IoOperationKind,
        /// The file path where the error occurred.
        path: String,
        /// Additional context (e.g., "temporary file", "after 3 retries").
        context: Option<String>,
        /// The underlying I/O error message.
        error: String,
    },

    /// File locking error.
    #[error("Failed to acquire file lock for '{path}': {error}")]
    LockError {
        /// The file path.
        path: String,
        /// The error message.
        error: String,
    },

    /// TOML parsing error.
    #[error("Failed to parse TOML: {0}")]
    TomlParseError(String),

    /// TOML serialization error.
    #[error("Failed to serialize to TOML: {0}")]
    TomlSerializeError(String),

    /// Failed to find home directory.
    #[error("Cannot determine home directory")]
    HomeDirNotFound,

    /// Failed to resolve path.
    #[error("Failed to resolve path: {0}")]
    PathResolution(String),

    /// Failed to encode filename.
    #[error("Failed to encode filename for ID '{id}': {reason}")]
    FilenameEncoding {
        /// The entity ID that failed to encode.
        id: String,
        /// The reason for the encoding failure.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_deserialization() {
        let err = MigrationError::DeserializationError("invalid JSON".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Failed to deserialize"));
        assert!(display.contains("invalid JSON"));
    }

    #[test]
    fn test_error_display_serialization() {
        let err = MigrationError::SerializationError("invalid data".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Failed to serialize"));
        assert!(display.contains("invalid data"));
    }

    #[test]
    fn test_error_display_entity_not_found() {
        let err = MigrationError::EntityNotFound("user".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Entity 'user' not found"));
    }

    #[test]
    fn test_error_display_migration_path_not_defined() {
        let err = MigrationError::MigrationPathNotDefined {
            entity: "task".to_string(),
            version: "2.0.0".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("No migration path defined"));
        assert!(display.contains("task"));
        assert!(display.contains("2.0.0"));
    }

    #[test]
    fn test_error_display_migration_step_failed() {
        let err = MigrationError::MigrationStepFailed {
            from: "1.0.0".to_string(),
            to: "2.0.0".to_string(),
            error: "field missing".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("Migration failed"));
        assert!(display.contains("1.0.0"));
        assert!(display.contains("2.0.0"));
        assert!(display.contains("field missing"));
    }

    #[test]
    fn test_error_debug() {
        let err = MigrationError::EntityNotFound("test".to_string());
        let debug = format!("{:?}", err);
        assert!(debug.contains("EntityNotFound"));
    }

    #[test]
    fn test_error_is_std_error() {
        let err = MigrationError::DeserializationError("test".to_string());
        // MigrationError should implement std::error::Error
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_error_display_circular_migration_path() {
        let err = MigrationError::CircularMigrationPath {
            entity: "task".to_string(),
            path: "1.0.0 -> 2.0.0 -> 1.0.0".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("Circular migration path"));
        assert!(display.contains("task"));
        assert!(display.contains("1.0.0 -> 2.0.0 -> 1.0.0"));
    }

    #[test]
    fn test_error_display_invalid_version_order() {
        let err = MigrationError::InvalidVersionOrder {
            entity: "task".to_string(),
            from: "2.0.0".to_string(),
            to: "1.0.0".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("Invalid version order"));
        assert!(display.contains("task"));
        assert!(display.contains("2.0.0"));
        assert!(display.contains("1.0.0"));
        assert!(display.contains("must increase"));
    }

    #[test]
    fn test_error_display_io_error_without_context() {
        let err = MigrationError::IoError {
            operation: IoOperationKind::Read,
            path: "/path/to/file.toml".to_string(),
            context: None,
            error: "Permission denied".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("Failed to read"));
        assert!(display.contains("/path/to/file.toml"));
        assert!(display.contains("Permission denied"));
    }

    #[test]
    fn test_error_display_io_error_with_context() {
        let err = MigrationError::IoError {
            operation: IoOperationKind::Write,
            path: "/path/to/tmp.toml".to_string(),
            context: Some("temporary file".to_string()),
            error: "Disk full".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("Failed to write"));
        assert!(display.contains("temporary file"));
        assert!(display.contains("/path/to/tmp.toml"));
        assert!(display.contains("Disk full"));
    }

    #[test]
    fn test_error_display_io_error_rename_with_retries() {
        let err = MigrationError::IoError {
            operation: IoOperationKind::Rename,
            path: "/path/to/file.toml".to_string(),
            context: Some("after 3 retries".to_string()),
            error: "Resource temporarily unavailable".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("Failed to rename"));
        assert!(display.contains("after 3 retries"));
        assert!(display.contains("/path/to/file.toml"));
        assert!(display.contains("Resource temporarily unavailable"));
    }

    #[test]
    fn test_io_operation_kind_display() {
        assert_eq!(IoOperationKind::Read.to_string(), "read");
        assert_eq!(IoOperationKind::Write.to_string(), "write");
        assert_eq!(IoOperationKind::Create.to_string(), "create");
        assert_eq!(IoOperationKind::Delete.to_string(), "delete");
        assert_eq!(IoOperationKind::Rename.to_string(), "rename");
        assert_eq!(IoOperationKind::CreateDir.to_string(), "create directory");
        assert_eq!(IoOperationKind::ReadDir.to_string(), "read directory");
        assert_eq!(IoOperationKind::Sync.to_string(), "sync");
    }
}
