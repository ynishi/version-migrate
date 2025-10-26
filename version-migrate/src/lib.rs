//! # version-migrate
//!
//! A library for explicit, type-safe schema versioning and migration.
//!
//! ## Features
//!
//! - **Type-safe migrations**: Define migrations between versions using traits
//! - **Validation**: Automatic validation of migration paths (circular path detection, version ordering)
//! - **Multi-format support**: Load from JSON, TOML, YAML, or any serde-compatible format
//! - **Vec support**: Migrate collections of versioned entities
//! - **Hierarchical structures**: Support for nested versioned entities
//! - **Async migrations**: Optional async support for I/O-heavy migrations
//!
//! ## Basic Example
//!
//! ```ignore
//! use version_migrate::{Versioned, MigratesTo, IntoDomain, Migrator};
//! use serde::{Serialize, Deserialize};
//!
//! // Version 1.0.0
//! #[derive(Serialize, Deserialize, Versioned)]
//! #[versioned(version = "1.0.0")]
//! struct TaskV1_0_0 {
//!     id: String,
//!     title: String,
//! }
//!
//! // Version 1.1.0
//! #[derive(Serialize, Deserialize, Versioned)]
//! #[versioned(version = "1.1.0")]
//! struct TaskV1_1_0 {
//!     id: String,
//!     title: String,
//!     description: Option<String>,
//! }
//!
//! // Domain model
//! struct TaskEntity {
//!     id: String,
//!     title: String,
//!     description: Option<String>,
//! }
//!
//! impl MigratesTo<TaskV1_1_0> for TaskV1_0_0 {
//!     fn migrate(self) -> TaskV1_1_0 {
//!         TaskV1_1_0 {
//!             id: self.id,
//!             title: self.title,
//!             description: None,
//!         }
//!     }
//! }
//!
//! impl IntoDomain<TaskEntity> for TaskV1_1_0 {
//!     fn into_domain(self) -> TaskEntity {
//!         TaskEntity {
//!             id: self.id,
//!             title: self.title,
//!             description: self.description,
//!         }
//!     }
//! }
//! ```
//!
//! ## Working with Collections (Vec)
//!
//! ```ignore
//! // Save multiple versioned entities
//! let tasks = vec![
//!     TaskV1_0_0 { id: "1".into(), title: "Task 1".into() },
//!     TaskV1_0_0 { id: "2".into(), title: "Task 2".into() },
//! ];
//! let json = migrator.save_vec(tasks)?;
//!
//! // Load and migrate multiple entities
//! let domains: Vec<TaskEntity> = migrator.load_vec("task", &json)?;
//! ```
//!
//! ## Hierarchical Structures
//!
//! For complex configurations with nested versioned entities:
//!
//! ```ignore
//! #[derive(Serialize, Deserialize, Versioned)]
//! #[versioned(version = "1.0.0")]
//! struct ConfigV1 {
//!     setting: SettingV1,
//!     items: Vec<ItemV1>,
//! }
//!
//! #[derive(Serialize, Deserialize, Versioned)]
//! #[versioned(version = "2.0.0")]
//! struct ConfigV2 {
//!     setting: SettingV2,
//!     items: Vec<ItemV2>,
//! }
//!
//! impl MigratesTo<ConfigV2> for ConfigV1 {
//!     fn migrate(self) -> ConfigV2 {
//!         ConfigV2 {
//!             // Migrate nested entities
//!             setting: self.setting.migrate(),
//!             items: self.items.into_iter()
//!                 .map(|item| item.migrate())
//!                 .collect(),
//!         }
//!     }
//! }
//! ```
//!
//! ## Design Philosophy
//!
//! This library follows the **explicit versioning** approach:
//!
//! - Each version has its own type (V1, V2, V3, etc.)
//! - Migration logic is explicit and testable
//! - Version changes are tracked in code
//! - Root-level versioning ensures consistency
//!
//! This differs from ProtoBuf's "append-only" approach but allows for:
//! - Schema refactoring and cleanup
//! - Type-safe migration paths
//! - Clear version history in code

use serde::{Deserialize, Serialize};

pub mod errors;
mod migrator;

// Re-export the derive macro
pub use version_migrate_macro::Versioned;

// Re-export error types
pub use errors::MigrationError;

// Re-export migrator types
pub use migrator::{MigrationPath, Migrator};

// Re-export async-trait for user convenience
pub use async_trait::async_trait;

/// A trait for versioned data schemas.
///
/// This trait marks a type as representing a specific version of a data schema.
/// It should be derived using `#[derive(Versioned)]` along with the `#[versioned(version = "x.y.z")]` attribute.
///
/// # Custom Keys
///
/// You can customize the serialization keys:
///
/// ```ignore
/// #[derive(Versioned)]
/// #[versioned(
///     version = "1.0.0",
///     version_key = "schema_version",
///     data_key = "payload"
/// )]
/// struct Task { ... }
/// // Serializes to: {"schema_version":"1.0.0","payload":{...}}
/// ```
pub trait Versioned {
    /// The semantic version of this schema.
    const VERSION: &'static str;

    /// The key name for the version field in serialized data.
    /// Defaults to "version".
    const VERSION_KEY: &'static str = "version";

    /// The key name for the data field in serialized data.
    /// Defaults to "data".
    const DATA_KEY: &'static str = "data";
}

/// Defines explicit migration logic from one version to another.
///
/// Implementing this trait establishes a migration path from `Self` (the source version)
/// to `T` (the target version).
pub trait MigratesTo<T: Versioned>: Versioned {
    /// Migrates from the current version to the target version.
    fn migrate(self) -> T;
}

/// Converts a versioned DTO into the application's domain model.
///
/// This trait should be implemented on the latest version of a DTO to convert
/// it into the clean, version-agnostic domain model.
pub trait IntoDomain<D>: Versioned {
    /// Converts this versioned data into the domain model.
    fn into_domain(self) -> D;
}

/// Async version of `MigratesTo` for migrations requiring I/O operations.
///
/// Use this trait when migrations need to perform asynchronous operations
/// such as database queries or API calls.
#[async_trait::async_trait]
pub trait AsyncMigratesTo<T: Versioned>: Versioned + Send {
    /// Asynchronously migrates from the current version to the target version.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if the migration fails.
    async fn migrate(self) -> Result<T, MigrationError>;
}

/// Async version of `IntoDomain` for domain conversions requiring I/O operations.
///
/// Use this trait when converting to the domain model requires asynchronous
/// operations such as fetching additional data from external sources.
#[async_trait::async_trait]
pub trait AsyncIntoDomain<D>: Versioned + Send {
    /// Asynchronously converts this versioned data into the domain model.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if the conversion fails.
    async fn into_domain(self) -> Result<D, MigrationError>;
}

/// A wrapper for serialized data that includes explicit version information.
///
/// This struct is used for persistence to ensure that the version of the data
/// is always stored alongside the data itself.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VersionedWrapper<T> {
    /// The semantic version of the data.
    pub version: String,
    /// The actual data.
    pub data: T,
}

impl<T> VersionedWrapper<T> {
    /// Creates a new versioned wrapper with the specified version and data.
    pub fn new(version: String, data: T) -> Self {
        Self { version, data }
    }
}

impl<T: Versioned> VersionedWrapper<T> {
    /// Creates a wrapper from a versioned value, automatically extracting its version.
    pub fn from_versioned(data: T) -> Self {
        Self {
            version: T::VERSION.to_string(),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    struct TestData {
        value: String,
    }

    impl Versioned for TestData {
        const VERSION: &'static str = "1.0.0";
    }

    #[test]
    fn test_versioned_wrapper_from_versioned() {
        let data = TestData {
            value: "test".to_string(),
        };
        let wrapper = VersionedWrapper::from_versioned(data);

        assert_eq!(wrapper.version, "1.0.0");
        assert_eq!(wrapper.data.value, "test");
    }

    #[test]
    fn test_versioned_wrapper_new() {
        let data = TestData {
            value: "manual".to_string(),
        };
        let wrapper = VersionedWrapper::new("2.0.0".to_string(), data);

        assert_eq!(wrapper.version, "2.0.0");
        assert_eq!(wrapper.data.value, "manual");
    }

    #[test]
    fn test_versioned_wrapper_serialization() {
        let data = TestData {
            value: "serialize_test".to_string(),
        };
        let wrapper = VersionedWrapper::from_versioned(data);

        // Serialize
        let json = serde_json::to_string(&wrapper).expect("Serialization failed");

        // Deserialize
        let deserialized: VersionedWrapper<TestData> =
            serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(deserialized.version, "1.0.0");
        assert_eq!(deserialized.data.value, "serialize_test");
    }

    #[test]
    fn test_versioned_wrapper_with_complex_data() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct ComplexData {
            id: u64,
            name: String,
            tags: Vec<String>,
            metadata: Option<String>,
        }

        impl Versioned for ComplexData {
            const VERSION: &'static str = "3.2.1";
        }

        let data = ComplexData {
            id: 42,
            name: "complex".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
            metadata: Some("meta".to_string()),
        };

        let wrapper = VersionedWrapper::from_versioned(data);
        assert_eq!(wrapper.version, "3.2.1");
        assert_eq!(wrapper.data.id, 42);
        assert_eq!(wrapper.data.tags.len(), 2);
    }

    #[test]
    fn test_versioned_wrapper_clone() {
        let data = TestData {
            value: "clone_test".to_string(),
        };
        let wrapper = VersionedWrapper::from_versioned(data);
        let cloned = wrapper.clone();

        assert_eq!(cloned.version, wrapper.version);
        assert_eq!(cloned.data.value, wrapper.data.value);
    }

    #[test]
    fn test_versioned_wrapper_debug() {
        let data = TestData {
            value: "debug".to_string(),
        };
        let wrapper = VersionedWrapper::from_versioned(data);
        let debug_str = format!("{:?}", wrapper);

        assert!(debug_str.contains("1.0.0"));
        assert!(debug_str.contains("debug"));
    }
}
