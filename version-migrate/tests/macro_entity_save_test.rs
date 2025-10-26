//! Tests for saving domain entities using the VersionMigrate macro.

use serde::{Deserialize, Serialize};
use version_migrate::{FromDomain, IntoDomain, MigratesTo, Migrator, VersionMigrate, Versioned};

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

// ===== Domain Entity with Macro =====
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, VersionMigrate)]
#[version_migrate(entity = "task", latest = TaskV1_1_0)]
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

// ===== FromDomain Implementation (Still required) =====
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
fn test_macro_generated_latest_versioned() {
    // The VersionMigrate macro should generate LatestVersioned implementation
    use version_migrate::LatestVersioned;

    assert_eq!(TaskEntity::ENTITY_NAME, "task");

    let entity = TaskEntity {
        id: "macro-test".to_string(),
        title: "Macro Test".to_string(),
        description: Some("Testing macro".to_string()),
    };

    let latest = entity.to_latest();

    assert_eq!(latest.id, "macro-test");
    assert_eq!(latest.title, "Macro Test");
    assert_eq!(latest.description, Some("Testing macro".to_string()));
}

#[test]
fn test_save_entity_with_macro() {
    let migrator = Migrator::new();

    let entity = TaskEntity {
        id: "macro-save".to_string(),
        title: "Macro Save Test".to_string(),
        description: Some("Saved via macro".to_string()),
    };

    let json = migrator.save_entity(entity).unwrap();

    // Should be saved with latest version (1.1.0)
    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("\"macro-save\""));
    assert!(json.contains("\"Macro Save Test\""));
    assert!(json.contains("\"Saved via macro\""));
}

#[test]
fn test_save_entity_flat_with_macro() {
    let migrator = Migrator::new();

    let entity = TaskEntity {
        id: "flat-macro".to_string(),
        title: "Flat Macro Test".to_string(),
        description: None,
    };

    let json = migrator.save_entity_flat(entity).unwrap();

    // Flat format: version at same level as data fields
    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("\"id\":\"flat-macro\""));
    assert!(json.contains("\"title\":\"Flat Macro Test\""));

    // Should not have nested "data" key
    assert!(!json.contains("\"data\":{"));
}

#[test]
fn test_full_roundtrip_with_macro() {
    let migrator = Migrator::new();

    let entity = TaskEntity {
        id: "roundtrip-macro".to_string(),
        title: "Roundtrip Macro Test".to_string(),
        description: Some("Full cycle".to_string()),
    };

    // Save entity (uses macro-generated LatestVersioned)
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
fn test_save_entity_vec_with_macro() {
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

    let json = migrator.save_entity_vec(entities).unwrap();

    // Should be a JSON array with correct version
    assert!(json.starts_with('['));
    assert!(json.ends_with(']'));
    assert!(json.contains("\"version\":\"1.1.0\""));
    assert!(json.contains("Vec Task 1"));
    assert!(json.contains("Vec Task 2"));
}
