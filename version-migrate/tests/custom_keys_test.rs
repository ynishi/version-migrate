use serde::{Deserialize, Serialize};
use version_migrate::{IntoDomain, MigratesTo, Migrator, Versioned};

// ===== Custom Keys: schema_version and payload =====

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

// ===== Another custom keys: api_version and content =====

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "1.0.0", version_key = "api_version", data_key = "content")]
struct ApiV1 {
    id: String,
}

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "2.0.0", version_key = "api_version", data_key = "content")]
struct ApiV2 {
    id: String,
    timestamp: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct ApiDomain {
    id: String,
    timestamp: u64,
}

impl MigratesTo<ApiV2> for ApiV1 {
    fn migrate(self) -> ApiV2 {
        ApiV2 {
            id: self.id,
            timestamp: 0,
        }
    }
}

impl IntoDomain<ApiDomain> for ApiV2 {
    fn into_domain(self) -> ApiDomain {
        ApiDomain {
            id: self.id,
            timestamp: self.timestamp,
        }
    }
}

// ===== Tests =====

#[test]
fn test_custom_keys_save() {
    let migrator = Migrator::new();

    let data = CustomV1 {
        name: "Alice".to_string(),
    };

    let json = migrator.save(data).unwrap();

    // Should use custom keys: schema_version and payload
    assert!(json.contains("\"schema_version\""));
    assert!(json.contains("\"payload\""));
    assert!(!json.contains("\"version\""));
    assert!(!json.contains("\"data\""));
    assert!(json.contains("\"1.0.0\""));
    assert!(json.contains("Alice"));
}

#[test]
fn test_custom_keys_save_vec() {
    let migrator = Migrator::new();

    let data = vec![
        CustomV1 {
            name: "Alice".to_string(),
        },
        CustomV1 {
            name: "Bob".to_string(),
        },
    ];

    let json = migrator.save_vec(data).unwrap();

    assert!(json.contains("\"schema_version\""));
    assert!(json.contains("\"payload\""));
    assert!(json.contains("Alice"));
    assert!(json.contains("Bob"));
}

#[test]
fn test_custom_keys_load_and_migrate() {
    // JSON with custom keys
    let json = r#"{
        "schema_version": "1.0.0",
        "payload": {
            "name": "Charlie"
        }
    }"#;

    let path = Migrator::define("custom")
        .from::<CustomV1>()
        .step::<CustomV2>()
        .into::<CustomDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domain: CustomDomain = migrator.load("custom", json).unwrap();

    assert_eq!(domain.name, "Charlie");
    assert_eq!(domain.age, 0); // Default from migration
}

#[test]
fn test_custom_keys_load_vec() {
    let json = r#"[
        {"schema_version":"1.0.0","payload":{"name":"Alice"}},
        {"schema_version":"2.0.0","payload":{"name":"Bob","age":30}}
    ]"#;

    let path = Migrator::define("custom")
        .from::<CustomV1>()
        .step::<CustomV2>()
        .into::<CustomDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domains: Vec<CustomDomain> = migrator.load_vec("custom", json).unwrap();

    assert_eq!(domains.len(), 2);
    assert_eq!(domains[0].name, "Alice");
    assert_eq!(domains[0].age, 0);
    assert_eq!(domains[1].name, "Bob");
    assert_eq!(domains[1].age, 30);
}

#[test]
fn test_custom_keys_save_and_load_roundtrip() {
    let migrator = Migrator::new();

    let data = CustomV1 {
        name: "Dave".to_string(),
    };

    let json = migrator.save(data).unwrap();

    let path = Migrator::define("custom")
        .from::<CustomV1>()
        .step::<CustomV2>()
        .into::<CustomDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domain: CustomDomain = migrator.load("custom", &json).unwrap();

    assert_eq!(domain.name, "Dave");
    assert_eq!(domain.age, 0);
}

#[test]
fn test_different_custom_keys_per_entity() {
    // Test that different entities can use different custom keys
    let custom_path = Migrator::define("custom")
        .from::<CustomV1>()
        .step::<CustomV2>()
        .into::<CustomDomain>();

    let api_path = Migrator::define("api")
        .from::<ApiV1>()
        .step::<ApiV2>()
        .into::<ApiDomain>();

    let mut migrator = Migrator::new();
    migrator.register(custom_path).unwrap();
    migrator.register(api_path).unwrap();

    // Load custom entity with schema_version/payload
    let custom_json = r#"{"schema_version":"1.0.0","payload":{"name":"Alice"}}"#;
    let custom: CustomDomain = migrator.load("custom", custom_json).unwrap();
    assert_eq!(custom.name, "Alice");

    // Load API entity with api_version/content
    let api_json = r#"{"api_version":"1.0.0","content":{"id":"api-1"}}"#;
    let api: ApiDomain = migrator.load("api", api_json).unwrap();
    assert_eq!(api.id, "api-1");
}

#[test]
fn test_custom_keys_latest_version_no_migration() {
    let json = r#"{
        "schema_version": "2.0.0",
        "payload": {
            "name": "Eve",
            "age": 25
        }
    }"#;

    let path = Migrator::define("custom")
        .from::<CustomV2>()
        .into::<CustomDomain>();

    let mut migrator = Migrator::new();
    migrator.register(path).unwrap();

    let domain: CustomDomain = migrator.load("custom", json).unwrap();

    assert_eq!(domain.name, "Eve");
    assert_eq!(domain.age, 25);
}

#[test]
fn test_custom_keys_trait_constants() {
    // Verify that the trait constants are set correctly
    assert_eq!(CustomV1::VERSION, "1.0.0");
    assert_eq!(CustomV1::VERSION_KEY, "schema_version");
    assert_eq!(CustomV1::DATA_KEY, "payload");

    assert_eq!(CustomV2::VERSION, "2.0.0");
    assert_eq!(CustomV2::VERSION_KEY, "schema_version");
    assert_eq!(CustomV2::DATA_KEY, "payload");

    assert_eq!(ApiV1::VERSION_KEY, "api_version");
    assert_eq!(ApiV1::DATA_KEY, "content");
}
