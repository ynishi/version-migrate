//! Tests for saving domain entities using FromDomain and LatestVersioned traits.

use serde::{Deserialize, Serialize};
use version_migrate::{FromDomain, IntoDomain, LatestVersioned, MigratesTo, Migrator, Versioned};

// ===== Version 1.0.0 =====
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TaskV1_0_0 {
    id: String,
    title: String,
}

impl Versioned for TaskV1_0_0 {
    const VERSION: &'static str = "1.0.0";
}

// ===== Version 1.1.0 (Latest) =====
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TaskV1_1_0 {
    id: String,
    title: String,
    description: Option<String>,
}

impl Versioned for TaskV1_1_0 {
    const VERSION: &'static str = "1.1.0";
}

// ===== Domain Entity =====
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
}

// ===== Migrations =====
impl MigratesTo<TaskV1_1_0> for TaskV1_0_0 {
    fn migrate(self) -> TaskV1_1_0 {
        TaskV1_1_0 {
            id: self.id,
            title: self.title,
            description: None,
        }
    }
}

impl IntoDomain<TaskEntity> for TaskV1_1_0 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: self.description,
        }
    }
}

// ===== FromDomain Implementation =====
impl FromDomain<TaskEntity> for TaskV1_1_0 {
    fn from_domain(entity: TaskEntity) -> Self {
        TaskV1_1_0 {
            id: entity.id,
            title: entity.title,
            description: entity.description,
        }
    }
}

// ===== LatestVersioned Implementation =====
impl LatestVersioned for TaskEntity {
    type Latest = TaskV1_1_0;
    const ENTITY_NAME: &'static str = "task";
}

// ===== Tests =====

#[test]
fn test_save_entity_basic() {
    let migrator = Migrator::new();

    let entity = TaskEntity {
        id: "task-1".to_string(),
        title: "My Task".to_string(),
        description: Some("Task description".to_string()),
    };

    let json = migrator.save_entity(entity).unwrap();

    // Should be saved with latest version (1.1.0)
    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("\"task-1\""));
    assert!(json.contains("\"My Task\""));
    assert!(json.contains("\"Task description\""));
}

#[test]
fn test_save_entity_flat_basic() {
    let migrator = Migrator::new();

    let entity = TaskEntity {
        id: "task-2".to_string(),
        title: "Flat Task".to_string(),
        description: None,
    };

    let json = migrator.save_entity_flat(entity).unwrap();

    // Flat format: version at same level as data fields
    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("\"id\":\"task-2\""));
    assert!(json.contains("\"title\":\"Flat Task\""));

    // Should not have nested "data" key
    assert!(!json.contains("\"data\":{"));
}

#[test]
fn test_save_entity_and_load_roundtrip() {
    let migrator = Migrator::new();

    let entity = TaskEntity {
        id: "roundtrip".to_string(),
        title: "Roundtrip Task".to_string(),
        description: Some("Description".to_string()),
    };

    // Save entity (automatically uses latest version)
    let json = migrator.save_entity(entity.clone()).unwrap();

    // Register migration path
    let path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    // Load back
    let loaded: TaskEntity = migrator.load("task", &json).unwrap();

    assert_eq!(loaded.id, entity.id);
    assert_eq!(loaded.title, entity.title);
    assert_eq!(loaded.description, entity.description);
}

#[test]
fn test_save_entity_vec() {
    let migrator = Migrator::new();

    let entities = vec![
        TaskEntity {
            id: "1".to_string(),
            title: "Task 1".to_string(),
            description: None,
        },
        TaskEntity {
            id: "2".to_string(),
            title: "Task 2".to_string(),
            description: Some("Second task".to_string()),
        },
    ];

    let json = migrator.save_entity_vec(entities).unwrap();

    // Should be a JSON array
    assert!(json.starts_with('['));
    assert!(json.ends_with(']'));
    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("Task 1"));
    assert!(json.contains("Task 2"));
}

#[test]
fn test_save_entity_vec_flat() {
    let migrator = Migrator::new();

    let entities = vec![
        TaskEntity {
            id: "flat-1".to_string(),
            title: "Flat Task 1".to_string(),
            description: None,
        },
        TaskEntity {
            id: "flat-2".to_string(),
            title: "Flat Task 2".to_string(),
            description: Some("Flat description".to_string()),
        },
    ];

    let json = migrator.save_entity_vec_flat(entities).unwrap();

    // Should be a JSON array with flat format
    assert!(json.starts_with('['));
    assert!(json.ends_with(']'));
    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("\"id\":\"flat-1\""));
    assert!(json.contains("\"id\":\"flat-2\""));

    // Should not have nested "data" key
    assert!(!json.contains("\"data\":{"));
}

#[test]
fn test_save_entity_vec_and_load_roundtrip() {
    let migrator = Migrator::new();

    let entities = vec![
        TaskEntity {
            id: "vec-1".to_string(),
            title: "Vec Task 1".to_string(),
            description: None,
        },
        TaskEntity {
            id: "vec-2".to_string(),
            title: "Vec Task 2".to_string(),
            description: Some("Vec description".to_string()),
        },
    ];

    // Save entities
    let json = migrator.save_entity_vec(entities.clone()).unwrap();

    // Register migration path
    let path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    // Load back
    let loaded: Vec<TaskEntity> = migrator.load_vec("task", &json).unwrap();

    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].id, entities[0].id);
    assert_eq!(loaded[0].title, entities[0].title);
    assert_eq!(loaded[1].id, entities[1].id);
    assert_eq!(loaded[1].description, entities[1].description);
}

#[test]
fn test_from_domain_to_latest() {
    let entity = TaskEntity {
        id: "convert".to_string(),
        title: "Convert Test".to_string(),
        description: Some("Test conversion".to_string()),
    };

    let latest = entity.to_latest();

    assert_eq!(latest.id, "convert");
    assert_eq!(latest.title, "Convert Test");
    assert_eq!(latest.description, Some("Test conversion".to_string()));
}
