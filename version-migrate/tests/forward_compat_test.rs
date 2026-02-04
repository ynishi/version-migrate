//! Integration tests for forward compatibility (Forwardable) support.

use serde::{Deserialize, Serialize};
use version_migrate::{migrate_path, Forwardable, IntoDomain, MigratesTo, Migrator, Versioned};

// V1.0.0 - Only known version
#[derive(Debug, Clone, Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0")]
struct TaskV1 {
    id: String,
    title: String,
}

// Domain model
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TaskEntity {
    id: String,
    title: String,
}

impl IntoDomain<TaskEntity> for TaskV1 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
        }
    }
}

fn create_migrator() -> Migrator {
    let mut migrator = Migrator::new();
    let path = migrate_path!("task", [TaskV1, TaskEntity]);
    migrator.register(path).unwrap();
    migrator
}

#[test]
fn test_load_forward_known_version() {
    let migrator = create_migrator();

    // V1.0.0 data (known version)
    let json = r#"{"version":"1.0.0","data":{"id":"1","title":"Task 1"}}"#;

    let task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

    assert_eq!(task.id, "1");
    assert_eq!(task.title, "Task 1");
    assert_eq!(task.original_version(), "1.0.0");
    assert!(!task.was_lossy());
    assert!(task.unknown_fields().is_empty());
}

#[test]
fn test_load_forward_unknown_version() {
    let migrator = create_migrator();

    // V2.0.0 data (unknown version) with extra field
    let json =
        r#"{"version":"2.0.0","data":{"id":"1","title":"Task 1","description":"New field"}}"#;

    let task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

    assert_eq!(task.id, "1");
    assert_eq!(task.title, "Task 1");
    assert_eq!(task.original_version(), "2.0.0");
    assert!(task.was_lossy());
    assert_eq!(task.unknown_fields().len(), 1);
    assert!(task.unknown_fields().contains_key("description"));
}

#[test]
fn test_save_forward_preserves_unknown_fields() {
    let migrator = create_migrator();

    // Load from unknown version
    let json =
        r#"{"version":"2.0.0","data":{"id":"1","title":"Original","description":"Preserved"}}"#;
    let mut task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

    // Modify known field
    task.title = "Updated".to_string();

    // Save should preserve unknown fields and original version
    let saved = migrator.save_forward(&task).unwrap();
    let saved_value: serde_json::Value = serde_json::from_str(&saved).unwrap();

    // Check version is preserved
    assert_eq!(saved_value["version"], "2.0.0");

    // Check known field is updated
    assert_eq!(saved_value["data"]["title"], "Updated");

    // Check unknown field is preserved
    assert_eq!(saved_value["data"]["description"], "Preserved");
}

#[test]
fn test_load_forward_flat_format() {
    let migrator = create_migrator();

    // Flat format with unknown version
    let json = r#"{"version":"2.0.0","id":"1","title":"Task","extra_field":"value"}"#;

    let task: Forwardable<TaskEntity> = migrator.load_forward_flat("task", json).unwrap();

    assert_eq!(task.id, "1");
    assert_eq!(task.original_version(), "2.0.0");
    assert!(task.was_lossy());
    assert!(task.unknown_fields().contains_key("extra_field"));
}

#[test]
fn test_save_forward_flat_format() {
    let migrator = create_migrator();

    // Load flat format
    let json = r#"{"version":"2.0.0","id":"1","title":"Original","extra":"preserved"}"#;
    let mut task: Forwardable<TaskEntity> = migrator.load_forward_flat("task", json).unwrap();

    task.title = "Updated".to_string();

    let saved = migrator.save_forward(&task).unwrap();
    let saved_value: serde_json::Value = serde_json::from_str(&saved).unwrap();

    // Flat format check
    assert_eq!(saved_value["version"], "2.0.0");
    assert_eq!(saved_value["title"], "Updated");
    assert_eq!(saved_value["extra"], "preserved");
    // Should NOT have nested "data" field
    assert!(saved_value.get("data").is_none());
}

#[test]
fn test_forwardable_deref() {
    let migrator = create_migrator();

    let json = r#"{"version":"1.0.0","data":{"id":"1","title":"Task"}}"#;
    let task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

    // Deref allows direct field access
    assert_eq!(task.id, "1");
    assert_eq!(task.title, "Task");
}

#[test]
fn test_forwardable_deref_mut() {
    let migrator = create_migrator();

    let json = r#"{"version":"1.0.0","data":{"id":"1","title":"Original"}}"#;
    let mut task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

    // DerefMut allows direct field modification
    task.title = "Modified".to_string();
    assert_eq!(task.title, "Modified");
}

#[test]
fn test_forwardable_into_inner() {
    let migrator = create_migrator();

    let json = r#"{"version":"1.0.0","data":{"id":"1","title":"Task"}}"#;
    let task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

    let inner: TaskEntity = task.into_inner();
    assert_eq!(inner.id, "1");
    assert_eq!(inner.title, "Task");
}

#[test]
fn test_roundtrip_known_version() {
    let migrator = create_migrator();

    let original_json = r#"{"version":"1.0.0","data":{"id":"1","title":"Task"}}"#;
    let task: Forwardable<TaskEntity> = migrator.load_forward("task", original_json).unwrap();
    let saved = migrator.save_forward(&task).unwrap();

    // Load again and verify
    let reloaded: Forwardable<TaskEntity> = migrator.load_forward("task", &saved).unwrap();
    assert_eq!(reloaded.id, "1");
    assert_eq!(reloaded.title, "Task");
    assert_eq!(reloaded.original_version(), "1.0.0");
}

#[test]
fn test_roundtrip_unknown_version_with_modifications() {
    let migrator = create_migrator();

    // Load unknown version
    let json = r#"{"version":"3.0.0","data":{"id":"1","title":"Original","new_field":"keep_me"}}"#;
    let mut task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

    // Modify
    task.title = "Modified".to_string();

    // Save
    let saved = migrator.save_forward(&task).unwrap();

    // Load again
    let reloaded: Forwardable<TaskEntity> = migrator.load_forward("task", &saved).unwrap();

    assert_eq!(reloaded.title, "Modified");
    assert_eq!(reloaded.original_version(), "3.0.0");
    assert!(reloaded.unknown_fields().contains_key("new_field"));
    assert_eq!(reloaded.unknown_fields()["new_field"], "keep_me");
}

// V1 -> V2 migration path test
mod with_migration {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, Versioned)]
    #[versioned(version = "1.0.0")]
    struct ConfigV1 {
        name: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, Versioned)]
    #[versioned(version = "2.0.0")]
    struct ConfigV2 {
        name: String,
        enabled: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct ConfigEntity {
        name: String,
        enabled: bool,
    }

    impl MigratesTo<ConfigV2> for ConfigV1 {
        fn migrate(self) -> ConfigV2 {
            ConfigV2 {
                name: self.name,
                enabled: true, // default
            }
        }
    }

    impl IntoDomain<ConfigEntity> for ConfigV2 {
        fn into_domain(self) -> ConfigEntity {
            ConfigEntity {
                name: self.name,
                enabled: self.enabled,
            }
        }
    }

    fn create_config_migrator() -> Migrator {
        let mut migrator = Migrator::new();
        let path = migrate_path!("config", [ConfigV1, ConfigV2, ConfigEntity]);
        migrator.register(path).unwrap();
        migrator
    }

    #[test]
    fn test_load_forward_with_migration() {
        let migrator = create_config_migrator();

        // V1 data should be migrated to V2, then to domain
        let json = r#"{"version":"1.0.0","data":{"name":"Test"}}"#;
        let config: Forwardable<ConfigEntity> = migrator.load_forward("config", json).unwrap();

        assert_eq!(config.name, "Test");
        assert!(config.enabled); // default from migration
        assert_eq!(config.original_version(), "1.0.0");
        assert!(!config.was_lossy());
    }

    #[test]
    fn test_load_forward_unknown_future_version() {
        let migrator = create_config_migrator();

        // V3 doesn't exist, should load as V2 (latest known)
        let json =
            r#"{"version":"3.0.0","data":{"name":"Future","enabled":false,"new_setting":123}}"#;
        let config: Forwardable<ConfigEntity> = migrator.load_forward("config", json).unwrap();

        assert_eq!(config.name, "Future");
        assert!(!config.enabled);
        assert_eq!(config.original_version(), "3.0.0");
        assert!(config.was_lossy());
        assert!(config.unknown_fields().contains_key("new_setting"));
    }
}

// Error case tests
mod error_cases {
    use super::*;
    use version_migrate::MigrationError;

    #[test]
    fn test_load_forward_entity_not_found() {
        let migrator = create_migrator();

        let json = r#"{"version":"1.0.0","data":{"id":"1","title":"Task"}}"#;
        let result: Result<Forwardable<TaskEntity>, MigrationError> =
            migrator.load_forward("nonexistent_entity", json);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, MigrationError::EntityNotFound(_)),
            "Expected EntityNotFound, got: {:?}",
            err
        );
    }

    #[test]
    fn test_load_forward_invalid_json() {
        let migrator = create_migrator();

        let invalid_json = "{ invalid json }";
        let result: Result<Forwardable<TaskEntity>, MigrationError> =
            migrator.load_forward("task", invalid_json);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, MigrationError::DeserializationError(_)),
            "Expected DeserializationError, got: {:?}",
            err
        );
    }

    #[test]
    fn test_load_forward_missing_version_field() {
        let migrator = create_migrator();

        // Missing "version" field
        let json = r#"{"data":{"id":"1","title":"Task"}}"#;
        let result: Result<Forwardable<TaskEntity>, MigrationError> =
            migrator.load_forward("task", json);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, MigrationError::DeserializationError(_)),
            "Expected DeserializationError, got: {:?}",
            err
        );
    }

    #[test]
    fn test_load_forward_missing_data_field() {
        let migrator = create_migrator();

        // Missing "data" field (wrapped format)
        let json = r#"{"version":"1.0.0"}"#;
        let result: Result<Forwardable<TaskEntity>, MigrationError> =
            migrator.load_forward("task", json);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, MigrationError::DeserializationError(_)),
            "Expected DeserializationError, got: {:?}",
            err
        );
    }

    #[test]
    fn test_load_forward_domain_deserialization_failure() {
        let migrator = create_migrator();

        // Data missing required field "title"
        let json = r#"{"version":"1.0.0","data":{"id":"1"}}"#;
        let result: Result<Forwardable<TaskEntity>, MigrationError> =
            migrator.load_forward("task", json);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, MigrationError::DeserializationError(_)),
            "Expected DeserializationError, got: {:?}",
            err
        );
    }

    #[test]
    fn test_load_forward_flat_missing_version() {
        let migrator = create_migrator();

        // Flat format without version
        let json = r#"{"id":"1","title":"Task"}"#;
        let result: Result<Forwardable<TaskEntity>, MigrationError> =
            migrator.load_forward_flat("task", json);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, MigrationError::DeserializationError(_)),
            "Expected DeserializationError, got: {:?}",
            err
        );
    }

    #[test]
    fn test_load_forward_non_object_json() {
        let migrator = create_migrator();

        // JSON array instead of object
        let json = r#"[1, 2, 3]"#;
        let result: Result<Forwardable<TaskEntity>, MigrationError> =
            migrator.load_forward("task", json);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, MigrationError::DeserializationError(_)),
            "Expected DeserializationError, got: {:?}",
            err
        );
    }

    #[test]
    fn test_load_forward_version_not_string() {
        let migrator = create_migrator();

        // version is a number, not string
        let json = r#"{"version":100,"data":{"id":"1","title":"Task"}}"#;
        let result: Result<Forwardable<TaskEntity>, MigrationError> =
            migrator.load_forward("task", json);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, MigrationError::DeserializationError(_)),
            "Expected DeserializationError, got: {:?}",
            err
        );
    }
}

// Edge case tests
mod edge_cases {
    use super::*;

    #[test]
    fn test_load_forward_empty_unknown_fields() {
        let migrator = create_migrator();

        // Known version with exact schema match (no unknown fields)
        let json = r#"{"version":"1.0.0","data":{"id":"1","title":"Task"}}"#;
        let task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

        assert!(task.unknown_fields().is_empty());
        assert!(!task.was_lossy());
    }

    #[test]
    fn test_load_forward_nested_unknown_fields() {
        let migrator = create_migrator();

        // Unknown version with nested object in unknown field
        let json = r#"{"version":"2.0.0","data":{"id":"1","title":"Task","metadata":{"key":"value","nested":{"deep":true}}}}"#;
        let task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

        assert!(task.was_lossy());
        assert!(task.unknown_fields().contains_key("metadata"));

        // Verify nested structure is preserved
        let metadata = &task.unknown_fields()["metadata"];
        assert!(metadata.is_object());
        assert_eq!(metadata["key"], "value");
        assert_eq!(metadata["nested"]["deep"], true);
    }

    #[test]
    fn test_load_forward_array_unknown_field() {
        let migrator = create_migrator();

        // Unknown version with array in unknown field
        let json = r#"{"version":"2.0.0","data":{"id":"1","title":"Task","tags":["rust","test","array"]}}"#;
        let task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

        assert!(task.was_lossy());
        assert!(task.unknown_fields().contains_key("tags"));

        let tags = &task.unknown_fields()["tags"];
        assert!(tags.is_array());
        assert_eq!(tags.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_save_forward_preserves_nested_unknown_fields() {
        let migrator = create_migrator();

        let json = r#"{"version":"2.0.0","data":{"id":"1","title":"Original","complex":{"a":1,"b":[1,2,3]}}}"#;
        let mut task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

        task.title = "Updated".to_string();

        let saved = migrator.save_forward(&task).unwrap();
        let saved_value: serde_json::Value = serde_json::from_str(&saved).unwrap();

        // Verify nested unknown field is preserved
        assert_eq!(saved_value["data"]["complex"]["a"], 1);
        assert_eq!(saved_value["data"]["complex"]["b"][0], 1);
        assert_eq!(saved_value["data"]["complex"]["b"][2], 3);
    }

    #[test]
    fn test_load_forward_multiple_unknown_fields() {
        let migrator = create_migrator();

        let json = r#"{"version":"2.0.0","data":{"id":"1","title":"Task","field1":"a","field2":123,"field3":true,"field4":null}}"#;
        let task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

        assert_eq!(task.unknown_fields().len(), 4);
        assert_eq!(task.unknown_fields()["field1"], "a");
        assert_eq!(task.unknown_fields()["field2"], 123);
        assert_eq!(task.unknown_fields()["field3"], true);
        assert!(task.unknown_fields()["field4"].is_null());
    }

    #[test]
    fn test_context_accessors() {
        let migrator = create_migrator();

        let json = r#"{"version":"2.0.0","data":{"id":"1","title":"Task","extra":"field"}}"#;
        let task: Forwardable<TaskEntity> = migrator.load_forward("task", json).unwrap();

        let ctx = task.context();
        assert_eq!(ctx.original_version(), "2.0.0");
        assert!(ctx.was_lossy());
        assert_eq!(ctx.unknown_fields().len(), 1);
    }
}
