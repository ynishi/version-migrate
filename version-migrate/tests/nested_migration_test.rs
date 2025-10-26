use serde::{Deserialize, Serialize};
use version_migrate::{IntoDomain, MigratesTo, Migrator, Versioned, VersionedWrapper};

// ===== Setting entity =====

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "1.0.0")]
struct SettingV1 {
    name: String,
}

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "2.0.0")]
struct SettingV2 {
    name: String,
    description: String,
}

impl MigratesTo<SettingV2> for SettingV1 {
    fn migrate(self) -> SettingV2 {
        SettingV2 {
            name: self.name,
            description: "No description".to_string(),
        }
    }
}

// ===== Item entity =====

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "1.0.0")]
struct ItemV1 {
    id: String,
}

#[derive(Serialize, Deserialize, Versioned, Debug, Clone)]
#[versioned(version = "2.0.0")]
struct ItemV2 {
    id: String,
    label: String,
}

impl MigratesTo<ItemV2> for ItemV1 {
    fn migrate(self) -> ItemV2 {
        ItemV2 {
            id: self.id.clone(),
            label: format!("Item {}", self.id),
        }
    }
}

// ===== Config entity (hierarchical root) =====

#[derive(Serialize, Deserialize, Versioned, Debug)]
#[versioned(version = "1.0.0")]
struct ConfigV1 {
    setting: SettingV1,
    items: Vec<ItemV1>,
}

#[derive(Serialize, Deserialize, Versioned, Debug)]
#[versioned(version = "2.0.0")]
struct ConfigV2 {
    setting: SettingV2,
    items: Vec<ItemV2>,
}

impl MigratesTo<ConfigV2> for ConfigV1 {
    fn migrate(self) -> ConfigV2 {
        ConfigV2 {
            setting: self.setting.migrate(),
            items: self.items.into_iter().map(|item| item.migrate()).collect(),
        }
    }
}

// ===== Domain model =====

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct ConfigDomain {
    setting_name: String,
    setting_description: String,
    items: Vec<ItemDomain>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct ItemDomain {
    id: String,
    label: String,
}

impl IntoDomain<ConfigDomain> for ConfigV2 {
    fn into_domain(self) -> ConfigDomain {
        ConfigDomain {
            setting_name: self.setting.name,
            setting_description: self.setting.description,
            items: self
                .items
                .into_iter()
                .map(|item| ItemDomain {
                    id: item.id,
                    label: item.label,
                })
                .collect(),
        }
    }
}

// ===== Tests =====

#[test]
fn test_nested_migration_v1_to_domain() {
    // Create V1 config with nested structures
    let config_v1 = ConfigV1 {
        setting: SettingV1 {
            name: "MyApp".to_string(),
        },
        items: vec![
            ItemV1 {
                id: "item-1".to_string(),
            },
            ItemV1 {
                id: "item-2".to_string(),
            },
            ItemV1 {
                id: "item-3".to_string(),
            },
        ],
    };

    // Save as JSON
    let wrapper = VersionedWrapper::from_versioned(config_v1);
    let json = serde_json::to_string(&wrapper).expect("Failed to serialize");

    // Setup migrator
    let config_path = Migrator::define("config")
        .from::<ConfigV1>()
        .step::<ConfigV2>()
        .into::<ConfigDomain>();

    let mut migrator = Migrator::new();
    migrator.register(config_path).unwrap();

    // Load and migrate
    let domain: ConfigDomain = migrator.load("config", &json).expect("Migration failed");

    // Verify setting migration
    assert_eq!(domain.setting_name, "MyApp");
    assert_eq!(domain.setting_description, "No description");

    // Verify items migration
    assert_eq!(domain.items.len(), 3);
    assert_eq!(domain.items[0].id, "item-1");
    assert_eq!(domain.items[0].label, "Item item-1");
    assert_eq!(domain.items[1].id, "item-2");
    assert_eq!(domain.items[1].label, "Item item-2");
    assert_eq!(domain.items[2].id, "item-3");
    assert_eq!(domain.items[2].label, "Item item-3");
}

#[test]
fn test_nested_no_migration_needed() {
    // Create V2 config (latest version)
    let config_v2 = ConfigV2 {
        setting: SettingV2 {
            name: "LatestApp".to_string(),
            description: "A great app".to_string(),
        },
        items: vec![ItemV2 {
            id: "item-x".to_string(),
            label: "Custom Label".to_string(),
        }],
    };

    // Save as JSON
    let wrapper = VersionedWrapper::from_versioned(config_v2);
    let json = serde_json::to_string(&wrapper).expect("Failed to serialize");

    // Setup migrator
    let config_path = Migrator::define("config")
        .from::<ConfigV2>()
        .into::<ConfigDomain>();

    let mut migrator = Migrator::new();
    migrator.register(config_path).unwrap();

    // Load without migration
    let domain: ConfigDomain = migrator.load("config", &json).expect("Load failed");

    // Verify data preserved
    assert_eq!(domain.setting_name, "LatestApp");
    assert_eq!(domain.setting_description, "A great app");
    assert_eq!(domain.items.len(), 1);
    assert_eq!(domain.items[0].id, "item-x");
    assert_eq!(domain.items[0].label, "Custom Label");
}

#[test]
fn test_nested_save_and_load_roundtrip() {
    let migrator = Migrator::new();

    // Create and save V1 config
    let config_v1 = ConfigV1 {
        setting: SettingV1 {
            name: "Roundtrip".to_string(),
        },
        items: vec![
            ItemV1 {
                id: "rt-1".to_string(),
            },
            ItemV1 {
                id: "rt-2".to_string(),
            },
        ],
    };

    let json = migrator.save(config_v1).expect("Save failed");

    // Verify JSON structure
    assert!(json.contains("\"version\":\"1.0.0\""));
    assert!(json.contains("Roundtrip"));
    assert!(json.contains("rt-1"));
    assert!(json.contains("rt-2"));

    // Setup migration and load
    let config_path = Migrator::define("config")
        .from::<ConfigV1>()
        .step::<ConfigV2>()
        .into::<ConfigDomain>();

    let mut migrator = Migrator::new();
    migrator.register(config_path).unwrap();

    let domain: ConfigDomain = migrator.load("config", &json).expect("Load failed");

    // Verify migrated data
    assert_eq!(domain.setting_name, "Roundtrip");
    assert_eq!(domain.items.len(), 2);
    assert_eq!(domain.items[0].id, "rt-1");
    assert_eq!(domain.items[1].id, "rt-2");
}

#[test]
fn test_nested_empty_items() {
    // Config with empty items array
    let config_v1 = ConfigV1 {
        setting: SettingV1 {
            name: "Empty".to_string(),
        },
        items: vec![],
    };

    let wrapper = VersionedWrapper::from_versioned(config_v1);
    let json = serde_json::to_string(&wrapper).expect("Failed to serialize");

    let config_path = Migrator::define("config")
        .from::<ConfigV1>()
        .step::<ConfigV2>()
        .into::<ConfigDomain>();

    let mut migrator = Migrator::new();
    migrator.register(config_path).unwrap();

    let domain: ConfigDomain = migrator.load("config", &json).expect("Migration failed");

    assert_eq!(domain.setting_name, "Empty");
    assert_eq!(domain.items.len(), 0);
}

#[test]
fn test_nested_large_items_array() {
    // Config with many items
    let items: Vec<ItemV1> = (0..100)
        .map(|i| ItemV1 {
            id: format!("item-{}", i),
        })
        .collect();

    let config_v1 = ConfigV1 {
        setting: SettingV1 {
            name: "Large".to_string(),
        },
        items,
    };

    let wrapper = VersionedWrapper::from_versioned(config_v1);
    let json = serde_json::to_string(&wrapper).expect("Failed to serialize");

    let config_path = Migrator::define("config")
        .from::<ConfigV1>()
        .step::<ConfigV2>()
        .into::<ConfigDomain>();

    let mut migrator = Migrator::new();
    migrator.register(config_path).unwrap();

    let domain: ConfigDomain = migrator.load("config", &json).expect("Migration failed");

    assert_eq!(domain.items.len(), 100);
    assert_eq!(domain.items[0].id, "item-0");
    assert_eq!(domain.items[99].id, "item-99");
    assert_eq!(domain.items[50].label, "Item item-50");
}
