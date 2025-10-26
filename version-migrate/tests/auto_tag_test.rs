use version_migrate::Versioned;

#[derive(Debug, PartialEq, Versioned)]
#[versioned(version = "1.0.0", auto_tag = true)]
struct TaskV1 {
    id: String,
    title: String,
}

#[derive(Debug, PartialEq, Versioned)]
#[versioned(version = "2.0.0", auto_tag = true)]
struct TaskV2 {
    id: String,
    title: String,
    description: Option<String>,
}

#[derive(Debug, PartialEq, Versioned)]
#[versioned(version = "1.0.0", version_key = "schema_version", auto_tag = true)]
struct CustomKeyTask {
    id: String,
    name: String,
}

#[test]
fn test_auto_tag_serialize() {
    let task = TaskV1 {
        id: "task-1".to_string(),
        title: "Test Task".to_string(),
    };

    let json = serde_json::to_string(&task).unwrap();
    assert!(json.contains("\"version\":\"1.0.0\""));
    assert!(json.contains("\"id\":\"task-1\""));
    assert!(json.contains("\"title\":\"Test Task\""));

    // Parse to verify structure
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["version"], "1.0.0");
    assert_eq!(parsed["id"], "task-1");
    assert_eq!(parsed["title"], "Test Task");
}

#[test]
fn test_auto_tag_deserialize() {
    let json = r#"{"version":"1.0.0","id":"task-1","title":"Test Task"}"#;
    let task: TaskV1 = serde_json::from_str(json).unwrap();

    assert_eq!(task.id, "task-1");
    assert_eq!(task.title, "Test Task");
}

#[test]
fn test_auto_tag_roundtrip() {
    let original = TaskV1 {
        id: "task-1".to_string(),
        title: "Test Task".to_string(),
    };

    let json = serde_json::to_string(&original).unwrap();
    let deserialized: TaskV1 = serde_json::from_str(&json).unwrap();

    assert_eq!(original, deserialized);
}

#[test]
fn test_auto_tag_with_optional_fields() {
    let task = TaskV2 {
        id: "task-2".to_string(),
        title: "Task with description".to_string(),
        description: Some("This is a description".to_string()),
    };

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: TaskV2 = serde_json::from_str(&json).unwrap();

    assert_eq!(task, deserialized);
    assert_eq!(
        deserialized.description,
        Some("This is a description".to_string())
    );
}

#[test]
fn test_auto_tag_custom_version_key() {
    let task = CustomKeyTask {
        id: "custom-1".to_string(),
        name: "Custom Task".to_string(),
    };

    let json = serde_json::to_string(&task).unwrap();
    assert!(json.contains("\"schema_version\":\"1.0.0\""));
    assert!(!json.contains("\"version\""));

    // Parse to verify structure
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["schema_version"], "1.0.0");
    assert_eq!(parsed["id"], "custom-1");
    assert_eq!(parsed["name"], "Custom Task");
}

#[test]
fn test_auto_tag_custom_key_roundtrip() {
    let original = CustomKeyTask {
        id: "custom-1".to_string(),
        name: "Custom Task".to_string(),
    };

    let json = serde_json::to_string(&original).unwrap();
    let deserialized: CustomKeyTask = serde_json::from_str(&json).unwrap();

    assert_eq!(original, deserialized);
}

#[test]
fn test_auto_tag_version_mismatch_error() {
    let json = r#"{"version":"2.0.0","id":"task-1","title":"Test Task"}"#;
    let result: Result<TaskV1, _> = serde_json::from_str(json);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("version mismatch"));
}

#[test]
fn test_auto_tag_missing_version_error() {
    let json = r#"{"id":"task-1","title":"Test Task"}"#;
    let result: Result<TaskV1, _> = serde_json::from_str(json);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("missing field"));
}

#[test]
fn test_auto_tag_missing_field_error() {
    let json = r#"{"version":"1.0.0","id":"task-1"}"#;
    let result: Result<TaskV1, _> = serde_json::from_str(json);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("missing field"));
}

#[test]
fn test_auto_tag_pretty_print() {
    let task = TaskV1 {
        id: "task-1".to_string(),
        title: "Test Task".to_string(),
    };

    let json = serde_json::to_string_pretty(&task).unwrap();
    assert!(json.contains("\"version\": \"1.0.0\""));
    assert!(json.contains("\"id\": \"task-1\""));
    assert!(json.contains("\"title\": \"Test Task\""));
}
