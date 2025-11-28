use serde::{Deserialize, Serialize};
use version_migrate::{migrator, IntoDomain, MigratesTo, Migrator, Versioned};

#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0")]
struct TaskV1_0_0 {
    id: String,
    title: String,
}

#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.1.0")]
struct TaskV1_1_0 {
    id: String,
    title: String,
    description: Option<String>,
}

#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.2.0")]
struct TaskV1_2_0 {
    id: String,
    title: String,
    description: Option<String>,
    priority: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
    priority: Option<u32>,
}

impl MigratesTo<TaskV1_1_0> for TaskV1_0_0 {
    fn migrate(self) -> TaskV1_1_0 {
        TaskV1_1_0 {
            id: self.id,
            title: self.title,
            description: None,
        }
    }
}

impl MigratesTo<TaskV1_2_0> for TaskV1_1_0 {
    fn migrate(self) -> TaskV1_2_0 {
        TaskV1_2_0 {
            id: self.id,
            title: self.title,
            description: self.description,
            priority: None,
        }
    }
}

// Implement IntoDomain for intermediate migrations
impl IntoDomain<TaskV1_1_0> for TaskV1_0_0 {
    fn into_domain(self) -> TaskV1_1_0 {
        self.migrate()
    }
}

impl IntoDomain<TaskV1_2_0> for TaskV1_1_0 {
    fn into_domain(self) -> TaskV1_2_0 {
        self.migrate()
    }
}

impl IntoDomain<TaskEntity> for TaskV1_2_0 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: self.description,
            priority: self.priority,
        }
    }
}

impl IntoDomain<CustomV2> for CustomV1 {
    fn into_domain(self) -> CustomV2 {
        self.migrate()
    }
}

#[test]
fn test_migrator_basic_syntax() {
    // Test basic two-version migration
    let path = migrator!("task", [TaskV1_0_0, TaskV1_1_0]);

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    // Test migration
    let v1_json = r#"{"version":"1.0.0","data":{"id":"1","title":"Test Task"}}"#;
    let result: TaskV1_1_0 = migrator.load("task", v1_json).unwrap();

    assert_eq!(result.id, "1");
    assert_eq!(result.title, "Test Task");
    assert_eq!(result.description, None);
}

#[test]
fn test_migrator_three_versions() {
    // Test three-version migration chain
    let path = migrator!("task", [TaskV1_0_0, TaskV1_1_0, TaskV1_2_0]);

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    // Test migration from v1.0.0 to v1.2.0
    let v1_json = r#"{"version":"1.0.0","data":{"id":"1","title":"Test Task"}}"#;
    let result: TaskV1_2_0 = migrator.load("task", v1_json).unwrap();

    assert_eq!(result.id, "1");
    assert_eq!(result.title, "Test Task");
    assert_eq!(result.description, None);
    assert_eq!(result.priority, None);
}

#[test]
fn test_migrator_with_domain() {
    // Test migration to domain entity
    let path = migrator!("task", [TaskV1_0_0, TaskV1_1_0, TaskV1_2_0]);

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    // Test migration to domain
    let v1_json = r#"{"version":"1.0.0","data":{"id":"1","title":"Test Task"}}"#;
    let result: TaskEntity = migrator.load("task", v1_json).unwrap();

    assert_eq!(result.id, "1");
    assert_eq!(result.title, "Test Task");
    assert_eq!(result.description, None);
    assert_eq!(result.priority, None);
}

// Custom key tests
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(
    version = "1.0.0",
    version_key = "schema_version",
    data_key = "payload"
)]
struct CustomV1 {
    value: String,
}

#[derive(Serialize, Deserialize, Versioned)]
#[versioned(
    version = "2.0.0",
    version_key = "schema_version",
    data_key = "payload"
)]
struct CustomV2 {
    value: String,
    new_field: Option<String>,
}

impl MigratesTo<CustomV2> for CustomV1 {
    fn migrate(self) -> CustomV2 {
        CustomV2 {
            value: self.value,
            new_field: None,
        }
    }
}

#[test]
fn test_migrator_custom_keys() {
    // Test with custom keys
    let path = migrator!(
        "custom",
        [CustomV1, CustomV2],
        version_key = "schema_version",
        data_key = "payload"
    );

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    // Test migration with custom keys
    let custom_json = r#"{"schema_version":"1.0.0","payload":{"value":"test"}}"#;
    let result: CustomV2 = migrator.load("custom", custom_json).unwrap();

    assert_eq!(result.value, "test");
    assert_eq!(result.new_field, None);
}

#[test]
fn test_migrator_compilation() {
    // This test ensures the macro patterns compile correctly with Vec notation
    let _path1 = migrator!("task1", [TaskV1_0_0, TaskV1_1_0]);
    let _path2 = migrator!("task2", [TaskV1_0_0, TaskV1_1_0, TaskV1_2_0]);
    let _path3 = migrator!(
        "task3",
        [TaskV1_0_0, TaskV1_1_0],
        version_key = "v",
        data_key = "d"
    );
    let _path4 = migrator!(
        "task4",
        [TaskV1_0_0, TaskV1_1_0, TaskV1_2_0],
        version_key = "v",
        data_key = "d"
    );
}

#[test]
fn test_macro_generates_correct_builder_pattern() {
    // Verify the macro generates the same result as manual builder pattern
    let macro_path = migrator!("task", [TaskV1_0_0, TaskV1_1_0, TaskV1_2_0]);

    let manual_path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskV1_2_0>();

    // Both should work the same way
    let mut migrator1 = Migrator::new();
    let mut migrator2 = Migrator::new();

    migrator1.register(macro_path).unwrap();
    migrator2.register(manual_path).unwrap();

    let test_json = r#"{"version":"1.0.0","data":{"id":"1","title":"Test"}}"#;

    let result1: TaskV1_2_0 = migrator1.load("task", test_json).unwrap();
    let result2: TaskV1_2_0 = migrator2.load("task", test_json).unwrap();

    assert_eq!(result1.id, result2.id);
    assert_eq!(result1.title, result2.title);
}
