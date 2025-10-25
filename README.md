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
- **Format Flexibility**: Load from any serde-compatible format (JSON, TOML, YAML, etc.)
- **Async Support**: Async traits for migrations requiring I/O operations (database, API calls)

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
version-migrate = "0.1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

## Quick Start

```rust
use version_migrate::{Versioned, MigratesTo, IntoDomain, Migrator};
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

### Async Support

For migrations requiring I/O operations (database queries, API calls), use async traits:

```rust
use version_migrate::{async_trait, AsyncMigratesTo, AsyncIntoDomain};

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
