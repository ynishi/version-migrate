use serde::{Deserialize, Serialize};
use version_migrate::{IntoDomain, MigratesTo, Migrator, Versioned, VersionedWrapper};

// Version 1.0.0 of Task
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0")]
struct TaskV1_0_0 {
    id: String,
    title: String,
}

// Version 1.1.0 of Task (added description field)
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.1.0")]
struct TaskV1_1_0 {
    id: String,
    title: String,
    description: Option<String>,
}

// Domain model (clean, version-agnostic)
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
}

// Migration from V1.0.0 to V1.1.0
impl MigratesTo<TaskV1_1_0> for TaskV1_0_0 {
    fn migrate(self) -> TaskV1_1_0 {
        TaskV1_1_0 {
            id: self.id,
            title: self.title,
            description: None, // Default value for new field
        }
    }
}

// Conversion from latest version to domain model
impl IntoDomain<TaskEntity> for TaskV1_1_0 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: self.description,
        }
    }
}

#[test]
fn test_migration_from_v1_0_0_to_domain() {
    // Create old version data
    let old_task = TaskV1_0_0 {
        id: "task-1".to_string(),
        title: "Test Task".to_string(),
    };

    // Wrap it with version info
    let wrapped = VersionedWrapper::from_versioned(old_task);
    let json = serde_json::to_string(&wrapped).expect("Failed to serialize");

    // Setup migrator
    let task_path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path).unwrap();

    // Load and migrate
    let task: TaskEntity = migrator.load("task", &json).expect("Migration failed");

    // Verify the result
    assert_eq!(task.id, "task-1");
    assert_eq!(task.title, "Test Task");
    assert_eq!(task.description, None);
}

#[test]
fn test_load_latest_version() {
    // Create latest version data
    let latest_task = TaskV1_1_0 {
        id: "task-2".to_string(),
        title: "Latest Task".to_string(),
        description: Some("This is a description".to_string()),
    };

    // Wrap it with version info
    let wrapped = VersionedWrapper::from_versioned(latest_task);
    let json = serde_json::to_string(&wrapped).expect("Failed to serialize");

    // Setup migrator (no migration steps needed, just conversion to domain)
    let task_path = Migrator::define("task")
        .from::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path).unwrap();

    // Load
    let task: TaskEntity = migrator.load("task", &json).expect("Load failed");

    // Verify the result
    assert_eq!(task.id, "task-2");
    assert_eq!(task.title, "Latest Task");
    assert_eq!(task.description, Some("This is a description".to_string()));
}

#[test]
fn test_versioned_trait() {
    assert_eq!(TaskV1_0_0::VERSION, "1.0.0");
    assert_eq!(TaskV1_1_0::VERSION, "1.1.0");
}

#[test]
fn test_versioned_wrapper() {
    let task = TaskV1_0_0 {
        id: "test".to_string(),
        title: "Test".to_string(),
    };

    let wrapper = VersionedWrapper::from_versioned(task);
    assert_eq!(wrapper.version, "1.0.0");
    assert_eq!(wrapper.data.id, "test");
}

#[test]
fn test_save_and_load_with_migrator() {
    let migrator = Migrator::new();

    // Save V1.0.0 data
    let task_v1 = TaskV1_0_0 {
        id: "task-save".to_string(),
        title: "Saved Task".to_string(),
    };

    let json = migrator.save(task_v1).expect("Save failed");

    // Verify JSON format
    assert!(json.contains("\"version\":\"1.0.0\""));
    assert!(json.contains("\"task-save\""));
    assert!(json.contains("\"Saved Task\""));

    // Setup migration path
    let task_path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path).unwrap();

    // Load and migrate the saved data
    let loaded: TaskEntity = migrator.load("task", &json).expect("Load failed");

    assert_eq!(loaded.id, "task-save");
    assert_eq!(loaded.title, "Saved Task");
    assert_eq!(loaded.description, None); // Default from migration
}

#[test]
fn test_save_latest_and_load() {
    let migrator = Migrator::new();

    // Save V1.1.0 data (latest version)
    let task_v1_1 = TaskV1_1_0 {
        id: "task-latest".to_string(),
        title: "Latest Task".to_string(),
        description: Some("With description".to_string()),
    };

    let json = migrator.save(task_v1_1).expect("Save failed");

    // Setup migration path for latest version
    let task_path = Migrator::define("task")
        .from::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path).unwrap();

    // Load without migration needed
    let loaded: TaskEntity = migrator.load("task", &json).expect("Load failed");

    assert_eq!(loaded.id, "task-latest");
    assert_eq!(loaded.title, "Latest Task");
    assert_eq!(loaded.description, Some("With description".to_string()));
}

#[test]
fn test_roundtrip_preserves_data() {
    let migrator = Migrator::new();

    // Original data
    let original = TaskV1_1_0 {
        id: "roundtrip-1".to_string(),
        title: "Roundtrip Test".to_string(),
        description: Some("Testing roundtrip".to_string()),
    };

    // Save
    let json = migrator.save(original).expect("Save failed");

    // Load back
    let wrapper: VersionedWrapper<TaskV1_1_0> = serde_json::from_str(&json).expect("Parse failed");

    // Verify all fields preserved
    assert_eq!(wrapper.version, "1.1.0");
    assert_eq!(wrapper.data.id, "roundtrip-1");
    assert_eq!(wrapper.data.title, "Roundtrip Test");
    assert_eq!(
        wrapper.data.description,
        Some("Testing roundtrip".to_string())
    );
}

#[test]
fn test_load_from_toml() {
    // Create TOML representation of versioned data
    let toml_str = r#"
version = "1.0.0"

[data]
id = "task-toml"
title = "Task from TOML"
"#;

    // Parse TOML
    let toml_value: toml::Value = toml::from_str(toml_str).expect("Failed to parse TOML");

    // Setup migrator
    let task_path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path).unwrap();

    // Load from TOML using load_from
    let task: TaskEntity = migrator
        .load_from("task", toml_value)
        .expect("Failed to load from TOML");

    // Verify the result
    assert_eq!(task.id, "task-toml");
    assert_eq!(task.title, "Task from TOML");
    assert_eq!(task.description, None); // Default from migration
}

#[test]
fn test_load_from_toml_latest_version() {
    // Create TOML representation with latest version
    let toml_str = r#"
version = "1.1.0"

[data]
id = "task-toml-latest"
title = "Latest Task from TOML"
description = "TOML description"
"#;

    // Parse TOML
    let toml_value: toml::Value = toml::from_str(toml_str).expect("Failed to parse TOML");

    // Setup migrator
    let task_path = Migrator::define("task")
        .from::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path).unwrap();

    // Load from TOML
    let task: TaskEntity = migrator
        .load_from("task", toml_value)
        .expect("Failed to load from TOML");

    // Verify the result
    assert_eq!(task.id, "task-toml-latest");
    assert_eq!(task.title, "Latest Task from TOML");
    assert_eq!(task.description, Some("TOML description".to_string()));
}

#[test]
fn test_load_from_yaml() {
    // Create YAML representation of versioned data
    let yaml_str = r#"
version: "1.0.0"
data:
  id: "task-yaml"
  title: "Task from YAML"
"#;

    // Parse YAML
    let yaml_value: serde_yaml::Value =
        serde_yaml::from_str(yaml_str).expect("Failed to parse YAML");

    // Setup migrator
    let task_path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path).unwrap();

    // Load from YAML using load_from
    let task: TaskEntity = migrator
        .load_from("task", yaml_value)
        .expect("Failed to load from YAML");

    // Verify the result
    assert_eq!(task.id, "task-yaml");
    assert_eq!(task.title, "Task from YAML");
    assert_eq!(task.description, None); // Default from migration
}

#[test]
fn test_load_from_yaml_latest_version() {
    // Create YAML representation with latest version
    let yaml_str = r#"
version: "1.1.0"
data:
  id: "task-yaml-latest"
  title: "Latest Task from YAML"
  description: "YAML description"
"#;

    // Parse YAML
    let yaml_value: serde_yaml::Value =
        serde_yaml::from_str(yaml_str).expect("Failed to parse YAML");

    // Setup migrator
    let task_path = Migrator::define("task")
        .from::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path).unwrap();

    // Load from YAML
    let task: TaskEntity = migrator
        .load_from("task", yaml_value)
        .expect("Failed to load from YAML");

    // Verify the result
    assert_eq!(task.id, "task-yaml-latest");
    assert_eq!(task.title, "Latest Task from YAML");
    assert_eq!(task.description, Some("YAML description".to_string()));
}

#[test]
fn test_load_from_multi_format_consistency() {
    // Setup migrator once
    let task_path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path).unwrap();

    // Same data in different formats
    let json_str =
        r#"{"version":"1.0.0","data":{"id":"multi-format","title":"Multi Format Test"}}"#;

    let toml_str = r#"
version = "1.0.0"
[data]
id = "multi-format"
title = "Multi Format Test"
"#;

    let yaml_str = r#"
version: "1.0.0"
data:
  id: "multi-format"
  title: "Multi Format Test"
"#;

    // Load from JSON
    let from_json: TaskEntity = migrator.load("task", json_str).expect("JSON load failed");

    // Load from TOML
    let toml_value: toml::Value = toml::from_str(toml_str).expect("TOML parse failed");
    let from_toml: TaskEntity = migrator
        .load_from("task", toml_value)
        .expect("TOML load failed");

    // Load from YAML
    let yaml_value: serde_yaml::Value = serde_yaml::from_str(yaml_str).expect("YAML parse failed");
    let from_yaml: TaskEntity = migrator
        .load_from("task", yaml_value)
        .expect("YAML load failed");

    // All should produce the same result
    assert_eq!(from_json, from_toml);
    assert_eq!(from_json, from_yaml);
    assert_eq!(from_json.id, "multi-format");
    assert_eq!(from_json.title, "Multi Format Test");
}
