use serde::{Deserialize, Serialize};
use version_migrate::{IntoDomain, MigratesTo, Migrator, Versioned};

// ===== Test entities =====

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "1.0.0")]
struct TaskV1 {
    id: String,
    title: String,
}

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "2.0.0")]
struct TaskV2 {
    id: String,
    title: String,
    description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct TaskDomain {
    id: String,
    title: String,
    description: Option<String>,
}

impl MigratesTo<TaskV2> for TaskV1 {
    fn migrate(self) -> TaskV2 {
        TaskV2 {
            id: self.id,
            title: self.title,
            description: None,
        }
    }
}

impl IntoDomain<TaskDomain> for TaskV2 {
    fn into_domain(self) -> TaskDomain {
        TaskDomain {
            id: self.id,
            title: self.title,
            description: self.description,
        }
    }
}

// ===== Tests =====

#[test]
fn test_save_flat_basic() {
    let migrator = Migrator::new();

    let task = TaskV1 {
        id: "task-1".to_string(),
        title: "My Task".to_string(),
    };

    let json = migrator.save_flat(task).unwrap();

    // Flat format: version field at same level as data fields
    assert!(json.contains("\"version\":\"1.0.0\""));
    assert!(json.contains("\"id\":\"task-1\""));
    assert!(json.contains("\"title\":\"My Task\""));
    // Should NOT have nested "data" field
    assert!(!json.contains("\"data\""));
}

#[test]
fn test_load_flat_no_migration() {
    let json = r#"{"version":"2.0.0","id":"task-1","title":"Test","description":"Desc"}"#;

    let path = Migrator::define("task")
        .from::<TaskV2>()
        .into::<TaskDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domain: TaskDomain = migrator.load_flat("task", json).unwrap();

    assert_eq!(domain.id, "task-1");
    assert_eq!(domain.title, "Test");
    assert_eq!(domain.description, Some("Desc".to_string()));
}

#[test]
fn test_load_flat_with_migration() {
    let json = r#"{"version":"1.0.0","id":"task-1","title":"Old Task"}"#;

    let path = Migrator::define("task")
        .from::<TaskV1>()
        .step::<TaskV2>()
        .into::<TaskDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domain: TaskDomain = migrator.load_flat("task", json).unwrap();

    assert_eq!(domain.id, "task-1");
    assert_eq!(domain.title, "Old Task");
    assert_eq!(domain.description, None); // Migrated from V1
}

#[test]
fn test_save_and_load_flat_roundtrip() {
    let migrator = Migrator::new();

    let task = TaskV1 {
        id: "roundtrip".to_string(),
        title: "Test".to_string(),
    };

    let json = migrator.save_flat(task).unwrap();

    let path = Migrator::define("task")
        .from::<TaskV1>()
        .step::<TaskV2>()
        .into::<TaskDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domain: TaskDomain = migrator.load_flat("task", &json).unwrap();

    assert_eq!(domain.id, "roundtrip");
    assert_eq!(domain.title, "Test");
    assert_eq!(domain.description, None);
}

#[test]
fn test_save_vec_flat() {
    let migrator = Migrator::new();

    let tasks = vec![
        TaskV1 {
            id: "task-1".to_string(),
            title: "Task 1".to_string(),
        },
        TaskV1 {
            id: "task-2".to_string(),
            title: "Task 2".to_string(),
        },
    ];

    let json = migrator.save_vec_flat(tasks).unwrap();

    assert!(json.contains("\"version\":\"1.0.0\""));
    assert!(json.contains("\"id\":\"task-1\""));
    assert!(json.contains("\"id\":\"task-2\""));
    assert!(!json.contains("\"data\""));
}

#[test]
fn test_load_vec_flat() {
    let json = r#"[
        {"version":"1.0.0","id":"task-1","title":"Task 1"},
        {"version":"2.0.0","id":"task-2","title":"Task 2","description":"Desc 2"}
    ]"#;

    let path = Migrator::define("task")
        .from::<TaskV1>()
        .step::<TaskV2>()
        .into::<TaskDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domains: Vec<TaskDomain> = migrator.load_vec_flat("task", json).unwrap();

    assert_eq!(domains.len(), 2);
    assert_eq!(domains[0].id, "task-1");
    assert_eq!(domains[0].description, None); // Migrated from V1
    assert_eq!(domains[1].id, "task-2");
    assert_eq!(domains[1].description, Some("Desc 2".to_string())); // V2
}

#[test]
fn test_save_vec_flat_and_load_vec_flat_roundtrip() {
    let migrator = Migrator::new();

    let tasks = vec![
        TaskV1 {
            id: "vec-1".to_string(),
            title: "Vec Task 1".to_string(),
        },
        TaskV1 {
            id: "vec-2".to_string(),
            title: "Vec Task 2".to_string(),
        },
    ];

    let json = migrator.save_vec_flat(tasks).unwrap();

    let path = Migrator::define("task")
        .from::<TaskV1>()
        .step::<TaskV2>()
        .into::<TaskDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domains: Vec<TaskDomain> = migrator.load_vec_flat("task", &json).unwrap();

    assert_eq!(domains.len(), 2);
    assert_eq!(domains[0].id, "vec-1");
    assert_eq!(domains[1].id, "vec-2");
}

// ===== Custom keys tests =====

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(
    version = "1.0.0",
    version_key = "schema_version",
    data_key = "payload"
)]
struct CustomV1 {
    name: String,
}

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(
    version = "2.0.0",
    version_key = "schema_version",
    data_key = "payload"
)]
struct CustomV2 {
    name: String,
    age: u32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct CustomDomain {
    name: String,
    age: u32,
}

impl MigratesTo<CustomV2> for CustomV1 {
    fn migrate(self) -> CustomV2 {
        CustomV2 {
            name: self.name,
            age: 0,
        }
    }
}

impl IntoDomain<CustomDomain> for CustomV2 {
    fn into_domain(self) -> CustomDomain {
        CustomDomain {
            name: self.name,
            age: self.age,
        }
    }
}

#[test]
fn test_flat_with_custom_keys() {
    let migrator = Migrator::new();

    let data = CustomV1 {
        name: "Alice".to_string(),
    };

    let json = migrator.save_flat(data).unwrap();

    // Should use custom version_key
    assert!(json.contains("\"schema_version\":\"1.0.0\""));
    assert!(json.contains("\"name\":\"Alice\""));
    assert!(!json.contains("\"version\""));
    assert!(!json.contains("\"payload\"")); // data_key not used in flat format
}

#[test]
fn test_load_flat_with_custom_keys() {
    let json = r#"{"schema_version":"1.0.0","name":"Bob"}"#;

    let path = Migrator::define("custom")
        .from::<CustomV1>()
        .step::<CustomV2>()
        .into::<CustomDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domain: CustomDomain = migrator.load_flat("custom", json).unwrap();

    assert_eq!(domain.name, "Bob");
    assert_eq!(domain.age, 0);
}

#[test]
fn test_flat_with_runtime_override() {
    // Path-level override
    let path = Migrator::define("task")
        .with_keys("custom_ver", "ignored") // data_key is ignored in flat format
        .from::<TaskV1>()
        .step::<TaskV2>()
        .into::<TaskDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let json = r#"{"custom_ver":"1.0.0","id":"override-test","title":"Override"}"#;
    let domain: TaskDomain = migrator.load_flat("task", json).unwrap();

    assert_eq!(domain.id, "override-test");
    assert_eq!(domain.title, "Override");
}

#[test]
fn test_flat_with_migrator_level_override() {
    let path = Migrator::define("task")
        .from::<TaskV1>()
        .step::<TaskV2>()
        .into::<TaskDomain>();

    let mut migrator = Migrator::builder()
        .default_version_key("app_version")
        .default_data_key("ignored")
        .build();

    migrator.register(path).unwrap();

    let json = r#"{"app_version":"1.0.0","id":"migrator-override","title":"Test"}"#;
    let domain: TaskDomain = migrator.load_flat("task", json).unwrap();

    assert_eq!(domain.id, "migrator-override");
    assert_eq!(domain.title, "Test");
}
