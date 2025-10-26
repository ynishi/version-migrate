use serde::{Deserialize, Serialize};
use version_migrate::{DeriveQueryable as Queryable, Queryable as QueryableTrait};

// Test standalone Queryable macro
#[derive(Serialize, Deserialize, Queryable)]
#[queryable(entity = "task")]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
}

#[derive(Serialize, Deserialize, Queryable)]
#[queryable(entity = "user")]
struct UserEntity {
    id: String,
    name: String,
}

#[test]
fn test_standalone_queryable_with_entity() {
    assert_eq!(TaskEntity::ENTITY_NAME, "task");
    assert_eq!(UserEntity::ENTITY_NAME, "user");
}

#[test]
fn test_queryable_trait_implementation() {
    // Verify that TaskEntity implements Queryable
    fn requires_queryable<T: QueryableTrait>(_: &T) {
        // If this compiles, T implements Queryable
    }

    let task = TaskEntity {
        id: "1".to_string(),
        title: "Test".to_string(),
        description: None,
    };

    requires_queryable(&task);
}

#[test]
fn test_multiple_entities_with_different_names() {
    assert_eq!(TaskEntity::ENTITY_NAME, "task");
    assert_eq!(UserEntity::ENTITY_NAME, "user");
    assert_ne!(TaskEntity::ENTITY_NAME, UserEntity::ENTITY_NAME);
}

// Test that entity names can be descriptive
#[derive(Serialize, Deserialize, Queryable)]
#[queryable(entity = "app_configuration")]
struct AppConfig {
    theme: String,
    language: String,
}

#[test]
fn test_descriptive_entity_names() {
    assert_eq!(AppConfig::ENTITY_NAME, "app_configuration");
}
