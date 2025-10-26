use serde::{Deserialize, Serialize};
use version_migrate::{IntoDomain, MigratesTo, Migrator, Versioned};

// ===== Test entities with default keys =====

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "1.0.0")]
struct DefaultV1 {
    value: String,
}

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "2.0.0")]
struct DefaultV2 {
    value: String,
    count: u32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct DefaultDomain {
    value: String,
    count: u32,
}

impl MigratesTo<DefaultV2> for DefaultV1 {
    fn migrate(self) -> DefaultV2 {
        DefaultV2 {
            value: self.value,
            count: 0,
        }
    }
}

impl IntoDomain<DefaultDomain> for DefaultV2 {
    fn into_domain(self) -> DefaultDomain {
        DefaultDomain {
            value: self.value,
            count: self.count,
        }
    }
}

// ===== Tests =====

#[test]
fn test_migrator_builder_default_keys() {
    // Build migrator with global defaults
    let migrator = Migrator::builder()
        .default_version_key("global_version")
        .default_data_key("global_data")
        .build();

    // Save with entity (uses trait defaults internally, but will be overridden on load)
    let data = DefaultV1 {
        value: "test".to_string(),
    };

    // Note: save() uses the TYPE's constants, not the Migrator's defaults
    // The Migrator defaults are applied during register() for load operations
    let _json = migrator.save(data).unwrap();

    // Create path (without custom keys, so Migrator defaults should apply)
    let path = Migrator::define("default")
        .from::<DefaultV1>()
        .step::<DefaultV2>()
        .into::<DefaultDomain>();

    let mut migrator = Migrator::builder()
        .default_version_key("global_version")
        .default_data_key("global_data")
        .build();

    migrator.register(path).unwrap();

    // Load with Migrator's default keys
    let json = r#"{"global_version":"1.0.0","global_data":{"value":"override"}}"#;
    let domain: DefaultDomain = migrator.load("default", json).unwrap();

    assert_eq!(domain.value, "override");
    assert_eq!(domain.count, 0);
}

#[test]
fn test_migration_path_with_keys() {
    // Path-level override (highest priority)
    let path = Migrator::define("task")
        .with_keys("path_version", "path_data")
        .from::<DefaultV1>()
        .step::<DefaultV2>()
        .into::<DefaultDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    // Load with path-specific keys
    let json = r#"{"path_version":"1.0.0","path_data":{"value":"path"}}"#;
    let domain: DefaultDomain = migrator.load("task", json).unwrap();

    assert_eq!(domain.value, "path");
    assert_eq!(domain.count, 0);
}

#[test]
fn test_priority_path_over_migrator() {
    // Path with_keys() should override Migrator defaults
    let path = Migrator::define("entity")
        .with_keys("path_ver", "path_dat")
        .from::<DefaultV1>()
        .step::<DefaultV2>()
        .into::<DefaultDomain>();

    let mut migrator = Migrator::builder()
        .default_version_key("migrator_ver")
        .default_data_key("migrator_dat")
        .build();

    migrator.register(path).unwrap();

    // Should use path keys, not migrator defaults
    let json = r#"{"path_ver":"2.0.0","path_dat":{"value":"priority","count":42}}"#;
    let domain: DefaultDomain = migrator.load("entity", json).unwrap();

    assert_eq!(domain.value, "priority");
    assert_eq!(domain.count, 42);
}

#[test]
fn test_priority_migrator_over_trait() {
    // Migrator defaults should override trait constants
    let path = Migrator::define("entity")
        .from::<DefaultV1>()
        .step::<DefaultV2>()
        .into::<DefaultDomain>();

    let mut migrator = Migrator::builder()
        .default_version_key("mig_version")
        .default_data_key("mig_data")
        .build();

    migrator.register(path).unwrap();

    // Should use migrator defaults, not trait constants ("version"/"data")
    let json = r#"{"mig_version":"1.0.0","mig_data":{"value":"migrator"}}"#;
    let domain: DefaultDomain = migrator.load("entity", json).unwrap();

    assert_eq!(domain.value, "migrator");
    assert_eq!(domain.count, 0);
}

#[test]
fn test_multiple_entities_different_overrides() {
    // Entity 1: Path-level override
    let path1 = Migrator::define("entity1")
        .with_keys("e1_ver", "e1_data")
        .from::<DefaultV1>()
        .step::<DefaultV2>()
        .into::<DefaultDomain>();

    // Entity 2: No override (uses Migrator defaults)
    let path2 = Migrator::define("entity2")
        .from::<DefaultV1>()
        .step::<DefaultV2>()
        .into::<DefaultDomain>();

    let mut migrator = Migrator::builder()
        .default_version_key("default_ver")
        .default_data_key("default_data")
        .build();

    migrator.register(path1).unwrap();
    migrator.register(path2).unwrap();

    // Entity 1 uses path keys
    let json1 = r#"{"e1_ver":"2.0.0","e1_data":{"value":"ent1","count":10}}"#;
    let domain1: DefaultDomain = migrator.load("entity1", json1).unwrap();
    assert_eq!(domain1.value, "ent1");
    assert_eq!(domain1.count, 10);

    // Entity 2 uses migrator defaults
    let json2 = r#"{"default_ver":"1.0.0","default_data":{"value":"ent2"}}"#;
    let domain2: DefaultDomain = migrator.load("entity2", json2).unwrap();
    assert_eq!(domain2.value, "ent2");
    assert_eq!(domain2.count, 0);
}

#[test]
fn test_migrator_builder_partial_override() {
    // Only override version_key, data_key uses trait default
    let path = Migrator::define("entity")
        .from::<DefaultV1>()
        .step::<DefaultV2>()
        .into::<DefaultDomain>();

    let mut migrator = Migrator::builder()
        .default_version_key("custom_ver")
        // No default_data_key
        .build();

    migrator.register(path).unwrap();

    // custom_ver from Migrator, data from trait
    let json = r#"{"custom_ver":"2.0.0","data":{"value":"partial","count":5}}"#;
    let domain: DefaultDomain = migrator.load("entity", json).unwrap();
    assert_eq!(domain.value, "partial");
    assert_eq!(domain.count, 5);
}

#[test]
fn test_with_keys_partial_override() {
    // Only override version_key at path level
    let path = Migrator::define("entity")
        .with_keys("path_version", "data") // "data" is trait default
        .from::<DefaultV1>()
        .step::<DefaultV2>()
        .into::<DefaultDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let json = r#"{"path_version":"2.0.0","data":{"value":"partial_path","count":7}}"#;
    let domain: DefaultDomain = migrator.load("entity", json).unwrap();
    assert_eq!(domain.value, "partial_path");
    assert_eq!(domain.count, 7);
}

#[test]
fn test_save_vec_and_load_vec_with_runtime_override() {
    let data = vec![
        DefaultV1 {
            value: "item1".to_string(),
        },
        DefaultV1 {
            value: "item2".to_string(),
        },
    ];

    let migrator = Migrator::new();
    let _json = migrator.save_vec(data).unwrap();

    // Load with runtime override
    let path = Migrator::define("entity")
        .with_keys("rt_version", "rt_data")
        .from::<DefaultV1>()
        .step::<DefaultV2>()
        .into::<DefaultDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let json = r#"[
        {"rt_version":"1.0.0","rt_data":{"value":"vec1"}},
        {"rt_version":"2.0.0","rt_data":{"value":"vec2","count":99}}
    ]"#;

    let domains: Vec<DefaultDomain> = migrator.load_vec("entity", json).unwrap();
    assert_eq!(domains.len(), 2);
    assert_eq!(domains[0].value, "vec1");
    assert_eq!(domains[0].count, 0);
    assert_eq!(domains[1].value, "vec2");
    assert_eq!(domains[1].count, 99);
}
