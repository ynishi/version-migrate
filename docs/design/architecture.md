# Architecture: `version-migrate`

## 1. Background

Applications that persist data locally (e.g., session data, configuration) require a robust mechanism for managing changes to the data's schema over time. Ad-hoc solutions using `serde(default)` or `Option<T>` obscure migration logic, introduce technical debt, and lack reliability.

This document outlines the architecture for `version-migrate`, a crate designed to provide an explicit, type-safe, and developer-friendly framework for schema versioning and migration.

## 2. Goals

- **Explicit**: All schema changes and migration logic must be explicitly coded and testable.
- **Robust**: Provide a safe and reliable path to migrate data from any old version to the latest domain model.
- **Separation of Concerns**: The core domain model should remain completely unaware of the persistence layer and its versioning details.
- **Developer Experience (DX)**: Offer a `serde`-like derive macro to minimize boilerplate and leverage the Rust compiler to ensure migration paths are complete.

## 3. Proposed Architecture

The architecture is split into two crates to provide a clean separation between the core library and the procedural macro, mirroring the structure of popular libraries like `serde`.

### 3.1. Crate Structure

1.  **`version-migrate` (Library Crate)**
    - Contains the core logic, public traits, and the migration manager (`Migrator`).
    - This will be the primary crate consumed by applications.

2.  **`version-migrate-macro` (Proc-Macro Crate)**
    - Provides the `#[derive(Versioned)]` procedural macro.
    - Depends on `version-migrate` and handles code generation.

### 3.2. Core Components (`version-migrate` crate)

#### a. Core Traits

-   **`pub trait Versioned`**
    -   A marker trait for any struct representing a versioned data schema.
    -   It defines a single associated constant: `const VERSION: &'static str;`.

-   **`pub trait MigratesTo<T: Versioned>: Versioned`**
    -   Defines the explicit migration logic from one version (`Self`) to the next (`T`).
    -   Contains one required method: `fn migrate(self) -> T;`.

-   **`pub trait IntoDomain<D>: Versioned`**
    -   Defines the conversion from the final versioned DTO into the application's clean domain model (`D`).
    -   Contains one required method: `fn into_domain(self) -> D;`.

#### b. Data Persistence Format

To unambiguously identify the version of serialized data, a wrapper struct will be used for persistence.

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct VersionedWrapper<T> {
    pub version: String,
    pub data: T,
}
```

When loading data, the system will first deserialize it into `VersionedWrapper<serde_json::Value>` to inspect the `version` field before proceeding with the typed migration.

#### c. Migration Manager(Migrator)

The `Migrator` is the central orchestrator for migrations. It uses a builder pattern to define and register type-safe migration paths.

```rust
// Builder Usage (ensures compile-time correctness)
let task_path = Migrator::define("task")
    .from::<TaskV1_0_0>()
    .step::<TaskV1_1_0>()
    .into::<TaskEntity>();

// Manager Registration
let mut migrator = Migrator::new();
migrator.register(task_path);

// Runtime Usage
let task: TaskEntity = migrator.load("task", &raw_json_string)?;
```

The `load` method will internally handle version detection, lookup of the correct migration path, and execution of the migration chain.

### 3.3. Procedural Macro (`version-migrate-macro` crate)

#### `#[derive(Versioned)]`

This derive macro implements the `Versioned` trait.

-   **Attribute**: It requires a helper attribute: `#[versioned(version = "x.y.z")]`.
-   **Validation**: The macro will use the `semver` crate at **compile time** to parse and validate the provided version string. If the version is not a valid semantic version, a `compile_error!` will be emitted.
-   **Code Generation**: On success, it generates the `impl Versioned` block:

    ```rust
    use version_migrate::Versioned;
    use serde::{Serialize, Deserialize};

    #[derive(Serialize, Deserialize, Versioned)]
    #[versioned(version = "1.0.0")] // Macro uses this
    pub struct TaskV1_0_0 {
        pub id: String,
        pub title: String,
    }

    // Generated code:
    // impl Versioned for TaskV1_0_0 {
    //     const VERSION: &'static str = "1.0.0";
    // }
    ```

## 4. Example Workflow

1.  **Define DTOs**: Create structs for each version, deriving `Serialize`, `Deserialize`, and `Versioned`.
2.  **Implement Migrations**: Implement `MigratesTo` for transitions between DTO versions and `IntoDomain` for the final conversion to the domain model.
3.  **Build Migrator**: In application setup, define the migration paths for each entity and register them with an `Migrator` instance.
4.  **Load Data**: In the repository layer, use the `migrator.load()` method to deserialize and migrate raw data into the final domain model in a single, safe operation.

## 5. Implementation Status

### 5.1. Comprehensive Error Handling ✅ **Implemented**

A dedicated `MigrationError` enum has been implemented to enable consumers of the crate to handle failures gracefully.

**Implementation:**

```rust
pub enum MigrationError {
    DeserializationError(String),
    EntityNotFound(String),
    MigrationPathNotDefined { entity: String, version: String },
    MigrationStepFailed {
        from: String,
        to: String,
        error: String,
    },
}
```

The error type implements `std::fmt::Display` and `std::error::Error` for proper error handling integration.

## 6. Areas for Future Consideration

### 6.1. Serialization Format Flexibility

The current architecture is tightly coupled to `serde_json`. To support other formats like `bincode`, `yaml`, or `cbor`, the `load` method could be made generic to accept any `impl serde::Deserializer`.

**Potential API:**

```rust
pub fn load_from<'de, D, T>(&self, entity: &str, deserializer: D) -> Result<T, MigrationError>
where
    D: serde::Deserializer<'de>,
    T: DeserializeOwned,
{
    // Implementation
}
```

### 6.2. Async Support

The current traits are synchronous. For migrations that require I/O (e.g., database queries, API calls), introducing async versions of the traits (e.g., using `#[async_trait]`) would be a valuable future addition.

**Potential API:**

```rust
#[async_trait]
pub trait AsyncMigratesTo<T: Versioned>: Versioned {
    async fn migrate(self) -> Result<T, MigrationError>;
}
```

### 6.3. Migration Validation

Add compile-time or runtime validation to ensure:
- No circular migration paths
- All versions in a migration chain are reachable
- Version ordering follows semantic versioning rules

### 6.4. Migration Rollback Support

Support for bidirectional migrations to enable rollback scenarios:

```rust
pub trait MigratesFrom<T: Versioned>: Versioned {
    fn rollback(self) -> T;
}
```
