use serde::{Deserialize, Serialize};
use version_migrate::{Queryable, Versioned};

// Test with explicit queryable_key
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0", queryable = true, queryable_key = "task")]
struct TaskEntity {
    id: String,
    title: String,
}

// Test with default queryable_key (should be "userentity")
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0", queryable = true)]
struct UserEntity {
    id: String,
    name: String,
}

// Test without queryable (should not implement Queryable)
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0")]
struct NonQueryableEntity {
    id: String,
}

#[test]
fn test_queryable_with_explicit_key() {
    assert_eq!(TaskEntity::ENTITY_NAME, "task");
}

#[test]
fn test_queryable_with_default_key() {
    // Default should be lowercased type name
    assert_eq!(UserEntity::ENTITY_NAME, "userentity");
}

#[test]
fn test_queryable_trait_implementation() {
    // This test verifies that TaskEntity implements Queryable
    fn requires_queryable<T: Queryable>(_: &T) {
        // If this compiles, T implements Queryable
    }

    let task = TaskEntity {
        id: "1".to_string(),
        title: "Test".to_string(),
    };

    requires_queryable(&task);
}

#[test]
fn test_multiple_entities_with_different_keys() {
    assert_eq!(TaskEntity::ENTITY_NAME, "task");
    assert_eq!(UserEntity::ENTITY_NAME, "userentity");
    assert_ne!(TaskEntity::ENTITY_NAME, UserEntity::ENTITY_NAME);
}

// Test combining auto_tag and queryable
#[derive(Versioned)]
#[versioned(
    version = "1.0.0",
    auto_tag = true,
    queryable = true,
    queryable_key = "combo"
)]
struct ComboEntity {
    id: String,
    value: String,
}

#[test]
fn test_auto_tag_and_queryable_combination() {
    // Should implement both Queryable and Serialize/Deserialize
    assert_eq!(ComboEntity::ENTITY_NAME, "combo");

    let entity = ComboEntity {
        id: "1".to_string(),
        value: "test".to_string(),
    };

    // Test auto_tag serialization
    let json = serde_json::to_string(&entity).unwrap();
    assert!(json.contains("\"version\":\"1.0.0\""));
    assert!(json.contains("\"id\":\"1\""));
    assert!(json.contains("\"value\":\"test\""));

    // Test deserialization
    let deserialized: ComboEntity = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, "1");
    assert_eq!(deserialized.value, "test");
}

#[test]
fn test_queryable_with_custom_version_key() {
    #[derive(Serialize, Deserialize, Versioned)]
    #[versioned(
        version = "1.0.0",
        version_key = "schema_version",
        queryable = true,
        queryable_key = "custom"
    )]
    struct CustomEntity {
        id: String,
    }

    assert_eq!(CustomEntity::ENTITY_NAME, "custom");
    assert_eq!(CustomEntity::VERSION_KEY, "schema_version");
}
