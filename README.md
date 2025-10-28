# version-migrate

[![Crates.io](https://img.shields.io/crates/v/version-migrate.svg)](https://crates.io/crates/version-migrate)
[![Documentation](https://docs.rs/version-migrate/badge.svg)](https://docs.rs/version-migrate)
[![License](https://img.shields.io/crates/l/version-migrate.svg)](https://github.com/ynishi/version-migrate#license)

A Rust library for explicit, type-safe schema versioning and migration.

## Overview

Applications that persist data locally (e.g., session data, configuration) require a robust mechanism for managing changes to the data's schema over time. Ad-hoc solutions using `serde(default)` or `Option<T>` obscure migration logic, introduce technical debt, and lack reliability.

`version-migrate` provides an explicit, type-safe, and developer-friendly framework for schema versioning and migration, inspired by the design philosophy of `serde`.

## Features

- **Explicit**: All schema changes and migration logic must be explicitly coded and testable
- **Type-Safe**: Leverage Rust's type system to ensure migration paths are complete at compile time
- **Robust**: Provides a safe and reliable path to migrate data from any old version to the latest domain model
- **Separation of Concerns**: The core domain model remains completely unaware of persistence layer versioning details
- **Developer Experience**: `serde`-like derive macro (`#[derive(Versioned)]`) to minimize boilerplate
- **Direct Entity Saving**: Save domain entities directly using their latest version with `#[derive(VersionMigrate)]`
- **Format Flexibility**: Load from any serde-compatible format (JSON, TOML, YAML, etc.)
- **Flat Format Support**: Both wrapped (`{"version":"..","data":{..}}`) and flat (`{"version":"..","field":..}`) formats
- **Auto-Tag**: Direct serialization with `serde_json::to_string()` - no `Migrator` required for simple versioning
- **ConfigMigrator**: ORM-like interface for partial updates in complex JSON without version concerns
- **Vec Support**: Migrate collections of versioned entities with `save_vec` and `load_vec`
- **Hierarchical Structures**: Support for nested versioned entities with root-level versioning
- **Custom Serialization Keys**: Customize field names (`version_key`, `data_key`) with three-tier priority (Path > Migrator > Type)
- **Async Support**: Async traits for migrations requiring I/O operations (database, API calls)
- **File Storage with ACID**: Atomic file operations with retry logic, format conversion (TOML/JSON), and automatic cleanup.
- **Directory Storage with ACID**: A new storage engine (`DirStorage`) for managing entities as individual files, ideal for session or task management. Also provides a fully non-blocking `AsyncDirStorage` under an `async` feature flag.
- **Platform-Agnostic Paths**: Unified path management across Linux, macOS, Windows with customizable strategies (System/Xdg/CustomBase)

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
version-migrate = "0.9.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

## Quick Start

```rust
use version-migrate::{Versioned, MigratesTo, IntoDomain, Migrator};
use serde::{Serialize, Deserialize};

// Version 1.0.0
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0")]
struct Task_V1_0_0 {
    id: String,
    title: String,
}

// Version 1.1.0
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.1.0")]
struct Task_V1_1_0 {
    id: String,
    title: String,
    description: Option<String>,
}

// Domain model (clean, version-agnostic)
#[derive(Serialize, Deserialize)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
}

// Migration from V1.0.0 to V1.1.0
impl MigratesTo<Task_V1_1_0> for Task_V1_0_0 {
    fn migrate(self) -> Task_V1_1_0 {
        Task_V1_1_0 {
            id: self.id,
            title: self.title,
            description: None,
        }
    }
}

// Conversion to domain model
impl IntoDomain<TaskEntity> for Task_V1_1_0 {
    fn into_domain(self) -> TaskEntity {
        TaskEntity {
            id: self.id,
            title: self.title,
            description: self.description,
        }
    }
}

fn main() {
    // Setup migration path
    let task_path = Migrator::define("task")
        .from::<Task_V1_0_0>()
        .step::<Task_V1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path);

    // Save versioned data
    let old_task = Task_V1_0_0 {
        id: "task-1".to_string(),
        title: "Test".to_string(),
    };
    let json = migrator.save(old_task).unwrap();
    // Output: {"version":"1.0.0","data":{"id":"task-1","title":"Test"}}

    // Load and automatically migrate to latest version
    let task: TaskEntity = migrator.load("task", &json).unwrap();

    assert_eq!(task.title, "Test");
    assert_eq!(task.description, None); // Migrated from V1.0.0
}
```

## Key Features

### Save and Load

```rust
// Save versioned data to JSON
let task = TaskV1_0_0 { id: "1".into(), title: "My Task".into() };
let json = migrator.save(task)?;
// → {"version":"1.0.0","data":{"id":"1","title":"My Task"}}

// Load and automatically migrate to latest version
let task: TaskEntity = migrator.load("task", &json)?;
```

### Saving Domain Entities Directly

There are two ways to save domain entities directly using their latest version:

#### Option 1: Using `#[derive(VersionMigrate)]` macro

This approach associates the entity type with its latest version at compile time:

```rust
use version-migrate::{FromDomain, VersionMigrate};

// Latest versioned type
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.1.0")]
struct TaskV1_1_0 {
    id: String,
    title: String,
    description: Option<String>,
}

// Domain entity with macro
#[derive(Serialize, Deserialize, VersionMigrate)]
#[version_migrate(entity = "task", latest = TaskV1_1_0)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
}

// Define how to convert from Entity to latest version
impl FromDomain<TaskEntity> for TaskV1_1_0 {
    fn from_domain(entity: TaskEntity) -> Self {
        TaskV1_1_0 {
            id: entity.id,
            title: entity.title,
            description: entity.description,
        }
    }
}

// Now you can save entities directly!
let entity = TaskEntity {
    id: "1".to_string(),
    title: "My Task".to_string(),
    description: Some("Description".to_string()),
};

// Automatically saved with latest version (1.1.0)
let json = migrator.save_entity(entity)?;
// → {"version":"1.1.0","data":{"id":"1","title":"My Task","description":"Description"}}

// Also works with flat format
let json_flat = migrator.save_entity_flat(entity)?;
// → {"version":"1.1.0","id":"1","title":"My Task","description":"Description"}

// And with vectors
let entities = vec![entity1, entity2];
let json = migrator.save_entity_vec(entities)?;
```

#### Option 2: Using `into_with_save()` (No Macro Required)

This approach avoids circular dependencies between entity and DTO by registering save functions during migration path setup:

```rust
// Domain entity (no dependency on DTOs!)
#[derive(Serialize, Deserialize)]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
}

// Implement FromDomain on the DTO side
impl FromDomain<TaskEntity> for TaskV1_1_0 {
    fn from_domain(entity: TaskEntity) -> Self {
        TaskV1_1_0 {
            id: entity.id,
            title: entity.title,
            description: entity.description,
        }
    }
}

// Register with save support using into_with_save()
let path = Migrator::define("task")
    .from::<TaskV1_0_0>()
    .step::<TaskV1_1_0>()
    .into_with_save::<TaskEntity>();  // ← Enable domain saving

migrator.register(path)?;

// Save by entity name (no VersionMigrate macro needed!)
let entity = TaskEntity { ... };
let json = migrator.save_domain("task", entity)?;
// → {"version":"1.1.0","data":{"id":"1",...}}

// Also works with flat format
let json = migrator.save_domain_flat("task", entity)?;
// → {"version":"1.1.0","id":"1",...}
```

**Choose based on your needs:**
- **Option 1** (`VersionMigrate` macro): Better when entity and DTOs are in the same module
- **Option 2** (`into_with_save()`): Better for avoiding circular dependencies between domain and DTO layers

### Auto-Tag: Direct Serialization with Version

For cases where you want to use standard `serde_json::to_string()` directly without going through the `Migrator`, you can enable the `auto_tag` option:

```rust
#[derive(Versioned)]
#[versioned(version = "1.0.0", auto_tag = true)]
struct Task {
    id: String,
    title: String,
}

// Now you can use serde directly!
let task = Task { id: "1".into(), title: "My Task".into() };
let json = serde_json::to_string(&task)?;
// → {"version":"1.0.0","id":"1","title":"My Task"}

// Deserialization also works with version validation
let task: Task = serde_json::from_str(&json)?;
```

**Key features:**
- `auto_tag = true` generates custom `Serialize` and `Deserialize` implementations
- Version field is automatically inserted during serialization
- Version is validated during deserialization (returns error if mismatch)
- Works with custom version keys: `#[versioned(version = "1.0.0", version_key = "schema_version", auto_tag = true)]`
- No need for `Migrator` if you just want versioned serialization

**Note:** When `auto_tag = true`, you don't need `#[derive(Serialize, Deserialize)]` - the macro generates these implementations for you.

### ConfigMigrator: Partial Updates Made Easy

For complex configuration files with multiple versioned entities, `ConfigMigrator` provides an ORM-like interface for querying and updating specific parts of the JSON without dealing with migration logic.

```rust
use version-migrate::{ConfigMigrator, Migrator, DeriveQueryable as Queryable};

// Define your domain entity (version-agnostic) with queryable macro
#[derive(Serialize, Deserialize, Queryable)]
#[queryable(entity = "task")]
struct TaskEntity {
    id: String,
    title: String,
    description: Option<String>,
}

// That's it! The macro automatically implements:
// - Queryable trait with ENTITY_NAME = "task"
// - No version needed - domain entities are version-agnostic

// Setup migrator with migration paths (as usual)
let mut migrator = Migrator::new();
migrator.register(task_path)?;

// config.json:
// {
//   "app_name": "MyApp",
//   "version": "1.0.0",
//   "tasks": [
//     {"version": "1.0.0", "id": "1", "title": "Old Task"},
//     {"version": "2.0.0", "id": "2", "title": "New Task", "description": "Desc"}
//   ]
// }

let config_json = fs::read_to_string("config.json")?;
let mut config = ConfigMigrator::from(&config_json, migrator)?;

// Query tasks (automatically migrates all versions to TaskEntity)
let mut tasks: Vec<TaskEntity> = config.query("tasks")?;

// Work with domain entities (no version concerns!)
tasks[0].title = "Updated Task".to_string();
tasks.push(TaskEntity {
    id: "3".into(),
    title: "New Task".into(),
    description: None,
});

// Update config (version is automatically determined from migration path)
config.update("tasks", tasks)?;

// Save to file
fs::write("config.json", config.to_string()?)?;
// All tasks are now version 2.0.0!
```

**Benefits:**
- **No version awareness needed**: Work with domain entities (version-agnostic), not versioned DTOs
- **Separation of concerns**: Domain entities implement `Queryable`, versioned DTOs implement `Versioned`
- **Partial updates**: Only update specific keys in complex JSON structures
- **Preserves other fields**: Non-updated parts of the config remain unchanged
- **Automatic migration**: Old versions are transparently upgraded when queried
- **Type-safe**: `Queryable` trait ensures correct entity names at compile time
- **Zero boilerplate**: `#[derive(Queryable)]` macro eliminates manual trait implementation

**Perfect for:**
- Application configuration files with nested versioned data
- Session/state management with evolving schemas
- Multi-tenant systems where different tenants may have different data versions

**Standalone Queryable Macro:**
```rust
#[derive(Queryable)]
#[queryable(entity = "entity_name")]
struct DomainEntity { ... }

// Automatically implements:
impl Queryable for DomainEntity {
    const ENTITY_NAME: &'static str = "entity_name";
}
```

### Flat Format Support

In addition to the wrapped format, `version-migrate` supports flat format where the version field is at the same level as data fields. This is more common in general schema versioning scenarios.

```rust
// Save in flat format
let task = TaskV1_0_0 { id: "1".into(), title: "My Task".into() };
let json = migrator.save_flat(task)?;
// → {"version":"1.0.0","id":"1","title":"My Task"}

// Load from flat format
let task: TaskEntity = migrator.load_flat("task", &json)?;
```

**Format Comparison:**

```rust
// Wrapped format (for DB/storage systems)
save(data)  → {"version":"1.0.0","data":{"id":"1","title":"Task"}}
load()      → Extracts from "data" field

// Flat format (for general schema versioning)
save_flat(data) → {"version":"1.0.0","id":"1","title":"Task"}
load_flat()     → Version field at same level as data
```

**Vec Support:**

```rust
// Save and load collections in flat format
let tasks = vec![task1, task2, task3];
let json = migrator.save_vec_flat(tasks)?;
// → [{"version":"1.0.0","id":"1",...}, {"version":"1.0.0","id":"2",...}]

let tasks: Vec<TaskEntity> = migrator.load_vec_flat("task", &json)?;
```

**Runtime Override:**

Flat format also supports the same three-tier priority system for customizing version keys:

```rust
// Custom version key in flat format
let path = Migrator::define("task")
    .with_keys("schema_version", "ignored") // data_key not used in flat format
    .from::<TaskV1>()
    .into::<TaskDomain>();

let json = r#"{"schema_version":"1.0.0","id":"1","title":"Task"}"#;
let task: TaskEntity = migrator.load_flat("task", json)?;
```

### Multiple Format Support

The `load_from` method supports loading from any serde-compatible format (TOML, YAML, etc.):

```rust
// Load from TOML
let toml_str = r#"
version = "1.0.0"
[data]
id = "task-1"
title = "My Task"
"#;
let toml_value: toml::Value = toml::from_str(toml_str)?;
let task: TaskEntity = migrator.load_from("task", toml_value)?;

// Load from YAML
let yaml_str = r#"
version: "1.0.0"
data:
  id: "task-1"
  title: "My Task"
"#;
let yaml_value: serde_yaml::Value = serde_yaml::from_str(yaml_str)?;
let task: TaskEntity = migrator.load_from("task", yaml_value)?;

// JSON still works with the convenient load() method
let json = r#"{"version":"1.0.0","data":{"id":"task-1","title":"My Task"}}"#;
let task: TaskEntity = migrator.load("task", json)?;
```

### Automatic Migration

The migrator automatically applies all necessary migration steps:

```rust
// Even if data is V1.0.0, it will migrate through V1.1.0 → V1.2.0 → ... → Latest
let old_json = r#"{"version":"1.0.0","data":{...}}"#;
let latest: TaskEntity = migrator.load("task", old_json)?;
```

### Type-Safe Builder Pattern

The builder pattern ensures migration paths are complete at compile time:

```rust
Migrator::define("task")
    .from::<V1>()      // Starting version
    .step::<V2>()      // Must implement MigratesTo<V2> for V1
    .step::<V3>()      // Must implement MigratesTo<V3> for V2
    .into::<Domain>(); // Must implement IntoDomain<Domain> for V3
```

### Working with Collections (Vec)

Migrate multiple entities at once using `save_vec` and `load_vec`:

```rust
// Save multiple versioned entities
let tasks = vec![
    TaskV1_0_0 { id: "1".into(), title: "Task 1".into() },
    TaskV1_0_0 { id: "2".into(), title: "Task 2".into() },
    TaskV1_0_0 { id: "3".into(), title: "Task 3".into() },
];
let json = migrator.save_vec(tasks)?;
// → [{"version":"1.0.0","data":{"id":"1",...}}, ...]

// Load and migrate all entities
let domains: Vec<TaskEntity> = migrator.load_vec("task", &json)?;
```

The `load_vec_from` method also supports any serde-compatible format:

```rust
// Load from TOML array
let toml_array: Vec<toml::Value> = /* ... */;
let domains: Vec<TaskEntity> = migrator.load_vec_from("task", toml_array)?;

// Load from YAML array
let yaml_array: Vec<serde_yaml::Value> = /* ... */;
let domains: Vec<TaskEntity> = migrator.load_vec_from("task", yaml_array)?;
```

### Hierarchical Structures

For complex configurations with nested versioned entities, define migrations at the root level:

```rust
// Version 1.0.0 - Nested structure
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "1.0.0")]
struct ConfigV1 {
    setting: SettingV1,
    items: Vec<ItemV1>,
}

// Version 2.0.0 - All nested entities migrate together
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(version = "2.0.0")]
struct ConfigV2 {
    setting: SettingV2,
    items: Vec<ItemV2>,
}

// Migrate the entire hierarchy
impl MigratesTo<ConfigV2> for ConfigV1 {
    fn migrate(self) -> ConfigV2 {
        ConfigV2 {
            setting: self.setting.migrate(),  // Migrate nested entity
            items: self.items.into_iter()
                .map(|item| item.migrate())    // Migrate each item
                .collect(),
        }
    }
}
```

**Design Philosophy:**
- Root-level versioning ensures consistency across nested structures
- Each version has explicit types (ConfigV1, ConfigV2, etc.)
- All nested entities migrate together as a unit
- Migration logic is explicit and testable

This approach differs from ProtoBuf's "append-only" style but enables:
- Schema refactoring and cleanup
- Type-safe nested migrations
- Clear version history in code

### Custom Serialization Keys

For integrating with existing systems that use different field names (e.g., `schema_version` instead of `version`):

```rust
#[derive(Serialize, Deserialize, Versioned)]
#[versioned(
    version = "1.0.0",
    version_key = "schema_version",
    data_key = "payload"
)]
struct Task {
    id: String,
    title: String,
}

let migrator = Migrator::new();
let task = Task { id: "1".into(), title: "Task".into() };
let json = migrator.save(task)?;
// → {"schema_version":"1.0.0","payload":{"id":"1","title":"Task"}}
```

**Use cases:**
- Migrating existing data with custom field names
- Integrating with external APIs that use specific naming conventions
- Supporting multiple serialization formats with different requirements

**Default keys:**
- `version_key`: defaults to `"version"`
- `data_key`: defaults to `"data"`

### Runtime Key Override

Beyond compile-time customization, you can override serialization keys at runtime with a three-tier priority system:

**Priority (highest to lowest):**
1. **Path-level** (via `with_keys()`)
2. **Migrator-level** (via `builder()`)
3. **Type-level** (via `#[versioned]` macro)

#### Migrator-Level Defaults

Set default keys for all entities using `Migrator::builder()`:

```rust
let migrator = Migrator::builder()
    .default_version_key("schema_version")
    .default_data_key("payload")
    .build();

// All entities will use these keys unless overridden
let path = Migrator::define("task")
    .from::<TaskV1>()
    .into::<TaskDomain>();

migrator.register(path)?;

// Load with migrator-level keys
let json = r#"{"schema_version":"1.0.0","payload":{"id":"1","title":"Task"}}"#;
let task: TaskDomain = migrator.load("task", json)?;
```

#### Path-Level Override

Override keys for specific migration paths using `with_keys()`:

```rust
let path = Migrator::define("task")
    .with_keys("custom_ver", "custom_data")
    .from::<TaskV1>()
    .step::<TaskV2>()
    .into::<TaskDomain>();

let mut migrator = Migrator::builder()
    .default_version_key("default_ver")
    .default_data_key("default_data")
    .build();

migrator.register(path)?;

// Path-level keys take precedence over migrator defaults
let json = r#"{"custom_ver":"1.0.0","custom_data":{"id":"1","title":"Task"}}"#;
let task: TaskDomain = migrator.load("task", json)?;
```

#### Priority Example

```rust
// Type level: version_key = "type_version"
#[derive(Versioned)]
#[versioned(version = "1.0.0", version_key = "type_version")]
struct Task { ... }

// Migrator level overrides type level
let mut migrator = Migrator::builder()
    .default_version_key("migrator_version")  // Takes priority
    .build();

// Path level overrides migrator level
let path = Migrator::define("task")
    .with_keys("path_version", "data")  // Highest priority
    .from::<Task>()
    .into::<Domain>();
```

**Use cases:**
- Integrating multiple external systems with different naming conventions
- Supporting legacy data formats without changing type definitions
- Per-entity customization in multi-tenant systems

### Async Support

For migrations requiring I/O operations (database queries, API calls), use async traits:

```rust
use version-migrate::{async_trait, AsyncMigratesTo, AsyncIntoDomain};

#[async_trait]
impl AsyncMigratesTo<TaskV1_1_0> for TaskV1_0_0 {
    async fn migrate(self) -> Result<TaskV1_1_0, MigrationError> {
        // Fetch additional data from database
        let metadata = fetch_metadata(&self.id).await?;

        Ok(TaskV1_1_0 {
            id: self.id,
            title: self.title,
            metadata: Some(metadata),
        })
    }
}

#[async_trait]
impl AsyncIntoDomain<TaskEntity> for TaskV1_1_0 {
    async fn into_domain(self) -> Result<TaskEntity, MigrationError> {
        // Enrich data with external API call
        let enriched = enrich_task_data(&self).await?;
        Ok(enriched)
    }
}
```

### Migration Path Validation

Migration paths are automatically validated when registered:

```rust
let path = Migrator::define("task")
    .from::<TaskV1_0_0>()
    .step::<TaskV1_1_0>()
    .into::<TaskEntity>();

let mut migrator = Migrator::new();
migrator.register(path)?; // Validates before registering
```

Validation checks:
- **No circular paths**: Prevents version A → B → A loops
- **Semver ordering**: Ensures versions increase (1.0.0 → 1.1.0 → 2.0.0)

### Comprehensive Error Handling

All operations return `Result<T, MigrationError>`:

```rust
match migrator.load("task", json) {
    Ok(task) => println!("Loaded: {:?}", task),
    Err(MigrationError::EntityNotFound(e)) => eprintln!("Entity {} not registered", e),
    Err(MigrationError::DeserializationError(e)) => eprintln!("Invalid JSON: {}", e),
    Err(MigrationError::CircularMigrationPath { entity, path }) => {
        eprintln!("Circular path in {}: {}", entity, path)
    }
    Err(MigrationError::InvalidVersionOrder { entity, from, to }) => {
        eprintln!("Invalid version order in {}: {} -> {}", entity, from, to)
    }
    Err(e) => eprintln!("Migration failed: {}", e),
}
```

### File Storage with ACID Guarantees

`FileStorage` provides atomic file operations with ACID guarantees for persistent configuration:

```rust
use version-migrate::{FileStorage, FileStorageStrategy, FormatStrategy, LoadBehavior};

// Configure storage strategy
let strategy = FileStorageStrategy::default()
    .with_format(FormatStrategy::Toml)  // or Json
    .with_retry_count(3)
    .with_load_behavior(LoadBehavior::CreateIfMissing);

// Create storage (automatically loads from file if exists)
let mut storage = FileStorage::new(
    PathBuf::from("/path/to/config.toml"),
    migrator,
    strategy
)?;

// Query and update with automatic migration
let tasks: Vec<TaskEntity> = storage.query("tasks")?;
storage.update_and_save("tasks", updated_tasks)?;

// Get the file path for debugging or logging
let file_path = storage.path();
println!("Config stored at: {}", file_path.display());
```

**Features:**
- **Atomicity**: Temporary file + atomic rename ensures all-or-nothing updates
- **Retry Logic**: Configurable retry count for rename operations (default: 3)
- **Format Support**: TOML or JSON with automatic conversion
- **Load Strategies**: Create empty config if missing, or return error
- **Cleanup**: Automatic cleanup of temporary files (best effort)
- **Path Access**: `path()` method returns the storage file path for debugging or logging

### DirStorage: Multi-File Entity Storage

While `FileStorage` is ideal for single-file configurations, `DirStorage` is designed for managing a large number of entities where each entity is stored as a separate file. This is perfect for use cases like session data, user profiles, or task items.

It provides the same ACID guarantees as `FileStorage` but operates on a directory of files. It also supports flexible filename encoding to safely handle complex entity IDs.

#### Sync and Async Usage

`DirStorage` is available in both synchronous and asynchronous versions. The async version, `AsyncDirStorage`, is enabled via the `async` feature flag and uses `tokio::fs` for non-blocking I/O, providing significant performance benefits for I/O-heavy applications.

**`Cargo.toml` for async:**
```toml
[dependencies]
version-migrate = { version = "0.9.0", features = ["async"] }
tokio = { version = "1.0", features = ["full"] }
```

**Example:**

```rust
use version-migrate::{
    AppPaths, PathStrategy, Migrator, DirStorage, DirStorageStrategy, FilenameEncoding
};
// For async, also import AsyncDirStorage
use version-migrate::AsyncDirStorage;

// 1. Setup paths and migrator (same for both sync and async)
let paths = AppPaths::new("myapp");
let migrator = setup_migrator(); // Assuming a migrator is configured

// 2. Define a strategy
let strategy = DirStorageStrategy::default()
    .with_filename_encoding(FilenameEncoding::UrlEncode);

// =================================================
// 3a. Use the synchronous `DirStorage`
// =================================================
let storage = DirStorage::new(
    paths.clone(),
    "sessions",
    migrator.clone(),
    strategy.clone()
)?;

// Save, load, and list entities synchronously
storage.save("session", "user@example.com", session_entity.clone())?;
let loaded: SessionEntity = storage.load("session", "user@example.com")?;
let ids = storage.list_ids()?;
storage.delete("user@example.com")?;

// Get the base directory path for debugging or logging
let base_path = storage.base_path();
println!("Sessions stored in: {}", base_path.display());


// =================================================
// 3b. Use the asynchronous `AsyncDirStorage`
// =================================================
#[cfg(feature = "async")]
async fn run_async_storage() -> Result<(), MigrationError> {
    let paths = AppPaths::new("myapp");
    let migrator = setup_migrator();
    let strategy = DirStorageStrategy::default()
        .with_filename_encoding(FilenameEncoding::UrlEncode);

    let storage = AsyncDirStorage::new(
        paths,
        "sessions_async",
        migrator,
        strategy
    ).await?;

    // Save, load, and list entities asynchronously
    storage.save("session", "user@example.com", session_entity.clone()).await?;
    let loaded: SessionEntity = storage.load("session", "user@example.com").await?;
    let ids = storage.list_ids().await?;
    storage.delete("user@example.com").await?;

    // Get the base directory path for debugging or logging
    let base_path = storage.base_path();
    println!("Sessions stored in: {}", base_path.display());

    Ok(())
}
```

### Platform-Agnostic Path Management

`AppPaths` provides unified path resolution across platforms:

```rust
use version-migrate::{AppPaths, PathStrategy};

// Use OS-standard directories (default)
let paths = AppPaths::new("myapp");
// Linux:   ~/.config/myapp
// macOS:   ~/Library/Application Support/myapp
// Windows: %APPDATA%\myapp

// Force XDG on all platforms (for consistency)
let paths = AppPaths::new("myapp")
    .config_strategy(PathStrategy::Xdg)
    .data_strategy(PathStrategy::Xdg);
// All platforms: ~/.config/myapp, ~/.local/share/myapp

// Use custom base (useful for testing)
let paths = AppPaths::new("myapp")
    .config_strategy(PathStrategy::CustomBase("/opt/myapp".into()));
```

**Path Methods:**
```rust
// Get directory paths (creates if missing)
let config_dir = paths.config_dir()?;   // ~/.config/myapp
let data_dir = paths.data_dir()?;       // ~/.local/share/myapp

// Get file paths (creates parent directory)
let config_file = paths.config_file("config.toml")?;
let cache_file = paths.data_file("cache.db")?;
```

### Complete Example: Persistent Configuration

Combining `FileStorage` and `AppPaths` for production use:

```rust
use version-migrate::{
    AppPaths, PathStrategy, FileStorage, FileStorageStrategy,
    Migrator, Queryable
};

// Define your entity with Queryable
#[derive(Queryable)]
#[queryable(entity = "task")]
struct TaskEntity {
    id: String,
    title: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup path management (XDG for cross-platform consistency)
    let paths = AppPaths::new("myapp")
        .config_strategy(PathStrategy::Xdg);

    // 2. Setup migrator
    let task_path = Migrator::define("task")
        .from::<TaskV1_0_0>()
        .step::<TaskV1_1_0>()
        .into::<TaskEntity>();

    let mut migrator = Migrator::new();
    migrator.register(task_path)?;

    // 3. Create storage with automatic loading
    let config_path = paths.config_file("config.toml")?;
    let mut storage = FileStorage::new(
        config_path,
        migrator,
        FileStorageStrategy::default()
    )?;

    // 4. Use storage
    let tasks: Vec<TaskEntity> = storage.query("tasks")?;
    println!("Loaded {} tasks", tasks.len());

    // 5. Update and save atomically
    storage.update_and_save("tasks", updated_tasks)?;

    Ok(())
}
```

**Testing with Temporary Directories:**

```rust
use tempfile::TempDir;

#[test]
fn test_config_persistence() {
    let temp_dir = TempDir::new().unwrap();

    // Use CustomBase strategy to avoid touching real home directory
    let paths = AppPaths::new("myapp")
        .config_strategy(PathStrategy::CustomBase(temp_dir.path().into()));

    let config_path = paths.config_file("test.toml").unwrap();
    let mut storage = FileStorage::new(
        config_path,
        setup_migrator(),
        FileStorageStrategy::default()
    ).unwrap();

    // Test operations...
    storage.update_and_save("tasks", test_tasks).unwrap();

    // Verify persistence...
    let loaded: Vec<TaskEntity> = storage.query("tasks").unwrap();
    assert_eq!(loaded.len(), test_tasks.len());
}
```

**Important Notes:**
- **Production**: Use `PathStrategy::System` or `Xdg` for real user directories
- **Testing**: Use `PathStrategy::CustomBase` with `tempfile::TempDir` to avoid polluting home directory
- **TOML vs JSON**: TOML is more human-readable; JSON is more compact

## Architecture

The library is split into two crates:

- **`version-migrate`**: Core library with traits, `Migrator`, and error types
- **`version-migrate-macro`**: Procedural macro for deriving `Versioned` trait

This mirrors the structure of popular libraries like `serde`.

## Documentation

For detailed documentation, see:
- [API Documentation](https://docs.rs/version-migrate)
- [Architecture Design](./docs/design/architecture.md)

## Development

### Running Tests

```bash
make test
```

### Running Checks

```bash
make preflight
```

### Building Documentation

```bash
make doc
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Acknowledgments

This library is inspired by:
- `serde` - For its derive macro pattern and API design philosophy
- Database migration tools - For the concept of explicit, versioned migrations