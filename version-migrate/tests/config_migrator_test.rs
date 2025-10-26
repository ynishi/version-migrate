use serde::{Deserialize, Serialize};
use version_migrate::{
    ConfigMigrator, DeriveQueryable as Queryable, IntoDomain, MigratesTo, Migrator, Versioned,
};

// Task V1
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0")]
struct TaskV1 {
    id: String,
    title: String,
}

// Task V2
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "2.0.0")]
struct TaskV2 {
    id: String,
    title: String,
    description: Option<String>,
}

// Domain Entity (version-agnostic)
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Queryable)]
#[queryable(entity = "task")]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
}

// Migrations
impl MigratesTo<TaskV2> for TaskV1 {
    fn migrate(self) -> TaskV2 {
        TaskV2 {
            id: self.id,
            title: self.title,
            description: None,
        }
    }
}

impl IntoDomain<TaskEntity> for TaskV2 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: self.description,
        }
    }
}

fn setup_migrator() -> Migrator {
    let path = Migrator::define("task")
        .from::<TaskV1>()
        .step::<TaskV2>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();
    migrator
}

#[test]
fn test_config_migrator_query() {
    let migrator = setup_migrator();

    let config_json = r#"{
        "app_name": "MyApp",
        "tasks": [
            {"version": "1.0.0", "id": "1", "title": "Task 1"},
            {"version": "2.0.0", "id": "2", "title": "Task 2", "description": "Description 2"}
        ]
    }"#;

    let config = ConfigMigrator::from(config_json, migrator).unwrap();
    let tasks: Vec<TaskEntity> = config.query("tasks").unwrap();

    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, "1");
    assert_eq!(tasks[0].title, "Task 1");
    assert_eq!(tasks[0].description, None); // Migrated from V1

    assert_eq!(tasks[1].id, "2");
    assert_eq!(tasks[1].title, "Task 2");
    assert_eq!(tasks[1].description, Some("Description 2".to_string()));
}

#[test]
fn test_config_migrator_query_empty_array() {
    let migrator = setup_migrator();

    let config_json = r#"{
        "app_name": "MyApp",
        "tasks": []
    }"#;

    let config = ConfigMigrator::from(config_json, migrator).unwrap();
    let tasks: Vec<TaskEntity> = config.query("tasks").unwrap();

    assert_eq!(tasks.len(), 0);
}

#[test]
fn test_config_migrator_query_missing_key() {
    let migrator = setup_migrator();

    let config_json = r#"{
        "app_name": "MyApp"
    }"#;

    let config = ConfigMigrator::from(config_json, migrator).unwrap();
    let tasks: Vec<TaskEntity> = config.query("tasks").unwrap();

    assert_eq!(tasks.len(), 0);
}

#[test]
fn test_config_migrator_update() {
    let migrator = setup_migrator();

    let config_json = r#"{
        "app_name": "MyApp",
        "tasks": [
            {"version": "1.0.0", "id": "1", "title": "Task 1"}
        ]
    }"#;

    let mut config = ConfigMigrator::from(config_json, migrator).unwrap();
    let mut tasks: Vec<TaskEntity> = config.query("tasks").unwrap();

    // Update task
    tasks[0].title = "Updated Task 1".to_string();
    tasks[0].description = Some("New description".to_string());

    // Add new task
    tasks.push(TaskEntity {
        id: "2".to_string(),
        title: "New Task".to_string(),
        description: None,
    });

    config.update("tasks", tasks).unwrap();

    // Query again to verify
    let updated_tasks: Vec<TaskEntity> = config.query("tasks").unwrap();
    assert_eq!(updated_tasks.len(), 2);
    assert_eq!(updated_tasks[0].title, "Updated Task 1");
    assert_eq!(
        updated_tasks[0].description,
        Some("New description".to_string())
    );
    assert_eq!(updated_tasks[1].id, "2");
    assert_eq!(updated_tasks[1].title, "New Task");
}

#[test]
fn test_config_migrator_roundtrip() {
    let migrator = setup_migrator();

    let config_json = r#"{
        "app_name": "MyApp",
        "version": "1.0.0",
        "tasks": [
            {"version": "1.0.0", "id": "1", "title": "Old Task"},
            {"version": "2.0.0", "id": "2", "title": "New Task", "description": "Desc"}
        ]
    }"#;

    let mut config = ConfigMigrator::from(config_json, migrator).unwrap();

    // Query
    let mut tasks: Vec<TaskEntity> = config.query("tasks").unwrap();
    assert_eq!(tasks.len(), 2);

    // Modify
    tasks[0].title = "Modified Task".to_string();

    // Update
    config.update("tasks", tasks).unwrap();

    // Convert back to JSON
    let output_json = config.to_string().unwrap();

    // Verify JSON structure
    assert!(output_json.contains("\"app_name\": \"MyApp\""));
    assert!(output_json.contains("Modified Task"));

    // Parse to verify tasks have been updated to version 2.0.0
    let parsed: serde_json::Value = serde_json::from_str(&output_json).unwrap();
    let tasks_array = parsed["tasks"].as_array().unwrap();

    // All tasks should be version 2.0.0
    for task in tasks_array {
        assert_eq!(task["version"], "2.0.0");
    }
}

#[test]
fn test_config_migrator_preserves_other_fields() {
    let migrator = setup_migrator();

    let config_json = r#"{
        "app_name": "MyApp",
        "version": "1.0.0",
        "settings": {
            "theme": "dark",
            "language": "en"
        },
        "tasks": [
            {"version": "1.0.0", "id": "1", "title": "Task"}
        ]
    }"#;

    let mut config = ConfigMigrator::from(config_json, migrator).unwrap();

    // Update tasks
    let tasks: Vec<TaskEntity> = config.query("tasks").unwrap();
    config.update("tasks", tasks).unwrap();

    // Convert back to JSON
    let output_json = config.to_string().unwrap();

    // Verify other fields are preserved
    assert!(output_json.contains("\"app_name\": \"MyApp\""));
    assert!(output_json.contains("\"settings\""));
    assert!(output_json.contains("\"theme\": \"dark\""));
    assert!(output_json.contains("\"language\": \"en\""));
}

#[test]
fn test_config_migrator_to_string_compact() {
    let migrator = setup_migrator();

    let config_json = r#"{"app_name":"MyApp","tasks":[]}"#;
    let config = ConfigMigrator::from(config_json, migrator).unwrap();

    let compact = config.to_string_compact().unwrap();
    assert!(!compact.contains('\n'));
    assert!(compact.contains("\"app_name\":\"MyApp\""));
}

#[test]
fn test_config_migrator_as_value() {
    let migrator = setup_migrator();

    let config_json = r#"{"app_name":"MyApp","tasks":[]}"#;
    let config = ConfigMigrator::from(config_json, migrator).unwrap();

    let value = config.as_value();
    assert_eq!(value["app_name"], "MyApp");
    assert!(value["tasks"].is_array());
}

#[test]
fn test_config_migrator_query_non_array_error() {
    let migrator = setup_migrator();

    let config_json = r#"{
        "app_name": "MyApp",
        "tasks": "not an array"
    }"#;

    let config = ConfigMigrator::from(config_json, migrator).unwrap();
    let result: Result<Vec<TaskEntity>, _> = config.query("tasks");

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("does not contain an array"));
}

#[test]
fn test_config_migrator_invalid_json() {
    let migrator = setup_migrator();

    let invalid_json = r#"{"app_name": invalid}"#;
    let result = ConfigMigrator::from(invalid_json, migrator);

    assert!(result.is_err());
}
