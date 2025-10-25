//! Error types for migration operations.

use thiserror::Error;

/// Error types that can occur during migration operations.
#[derive(Error, Debug)]
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
}
