# version-migrate

[![Crates.io](https://img.shields.io/crates/v/version-migrate.svg)](https://crates.io/crates/version-migrate)
[![Documentation](https://docs.rs/version-migrate/badge.svg)](https://docs.rs/version-migrate)
[![License](https://img.shields.io/crates/l/version-migrate.svg)](https://github.com/yourusername/version-migrate#license)

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

    // Load and migrate data
    let json = r#"{"version":"1.0.0","data":{"id":"task-1","title":"Test"}}"#;
    let task: TaskEntity = migrator.load("task", json).unwrap();

    assert_eq!(task.title, "Test");
    assert_eq!(task.description, None);
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
