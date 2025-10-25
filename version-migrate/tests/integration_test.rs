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
    migrator.register(task_path);

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
    migrator.register(task_path);

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
