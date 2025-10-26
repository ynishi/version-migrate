//! Tests for saving domain entities by entity name using into_with_save().

use serde::{Deserialize, Serialize};
use version_migrate::{FromDomain, IntoDomain, MigratesTo, Migrator, Versioned};

// ===== Version 1.0.0 =====
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0")]
struct TaskV1_0_0 {
    id: String,
    title: String,
}

// ===== Version 1.1.0 (Latest) =====
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Versioned)]
#[versioned(version = "1.1.0")]
struct TaskV1_1_0 {
    id: String,
    title: String,
    description: Option<String>,
}

// ===== Domain Entity (No VersionMigrate macro needed) =====
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

// ===== Tests =====

#[test]
fn test_save_domain_by_name() {
    let path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into_with_save::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let entity = TaskEntity {
        id: "task-1".to_string(),
        title: "My Task".to_string(),
        description: Some("Task description".to_string()),
    };

    let json = migrator.save_domain("task", entity).unwrap();

    // Should be saved with latest version (1.1.0)
    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("\"task-1\""));
    assert!(json.contains("\"My Task\""));
    assert!(json.contains("\"Task description\""));
}

#[test]
fn test_save_domain_flat_by_name() {
    let path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into_with_save::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let entity = TaskEntity {
        id: "task-2".to_string(),
        title: "Flat Task".to_string(),
        description: None,
    };

    let json = migrator.save_domain_flat("task", entity).unwrap();

    // Flat format: version at same level as data fields
    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("\"id\":\"task-2\""));
    assert!(json.contains("\"title\":\"Flat Task\""));

    // Should not have nested "data" key
    assert!(!json.contains("\"data\":{"));
}

#[test]
fn test_save_domain_and_load_roundtrip() {
    let path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into_with_save::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let entity = TaskEntity {
        id: "roundtrip".to_string(),
        title: "Roundtrip Task".to_string(),
        description: Some("Description".to_string()),
    };

    // Save by entity name
    let json = migrator.save_domain("task", entity.clone()).unwrap();

    // Load back
    let loaded: TaskEntity = migrator.load("task", &json).unwrap();

    assert_eq!(loaded.id, entity.id);
    assert_eq!(loaded.title, entity.title);
    assert_eq!(loaded.description, entity.description);
}

#[test]
fn test_save_domain_without_save_support_error() {
    // Register without into_with_save
    let path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let entity = TaskEntity {
        id: "error".to_string(),
        title: "Should Error".to_string(),
        description: None,
    };

    // Should fail because into_with_save was not used
    let result = migrator.save_domain("task", entity);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("not registered with domain save support"));
}

#[test]
fn test_save_domain_unregistered_entity() {
    let migrator = Migrator::new();

    let entity = TaskEntity {
        id: "error".to_string(),
        title: "Unregistered".to_string(),
        description: None,
    };

    // Should fail because entity is not registered
    let result = migrator.save_domain("task", entity);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("not registered with domain save support"));
}

#[test]
fn test_save_domain_from_single_step() {
    // Test with single version (no migration steps)
    let path = Migrator::define("task")
        .from::<TaskV1_1_0>()
        .into_with_save::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let entity = TaskEntity {
        id: "single".to_string(),
        title: "Single Step".to_string(),
        description: Some("No migration needed".to_string()),
    };

    let json = migrator.save_domain("task", entity).unwrap();

    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("\"single\""));
    assert!(json.contains("\"Single Step\""));
}

#[test]
fn test_save_domain_flat_roundtrip() {
    let path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into_with_save::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let entity = TaskEntity {
        id: "flat-roundtrip".to_string(),
        title: "Flat Roundtrip".to_string(),
        description: None,
    };

    // Save flat
    let json = migrator.save_domain_flat("task", entity.clone()).unwrap();

    // Load flat
    let loaded: TaskEntity = migrator.load_flat("task", &json).unwrap();

    assert_eq!(loaded.id, entity.id);
    assert_eq!(loaded.title, entity.title);
    assert_eq!(loaded.description, entity.description);
}
