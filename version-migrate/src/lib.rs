//! # version-migrate
//!
//! A library for explicit, type-safe schema versioning and migration.
//!
//! ## Features
//!
//! - **Type-safe migrations**: Define migrations between versions using traits
//! - **Validation**: Automatic validation of migration paths (circular path detection, version ordering)
//! - **Multi-format support**: Load from JSON, TOML, YAML, or any serde-compatible format
//! - **Legacy data support**: Automatic fallback for data without version information
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
//! ## Legacy Data Support
//!
//! Handle data that was created before versioning was introduced:
//!
//! ```ignore
//! // Legacy data without version information
//! let legacy_json = r#"{"id": "task-1", "title": "Legacy Task"}"#;
//!
//! // Automatically treats legacy data as the first version and migrates
//! let domain: TaskEntity = migrator.load_with_fallback("task", legacy_json)?;
//!
//! // Also works with properly versioned data
//! let versioned_json = r#"{"version":"1.0.0","data":{"id":"task-1","title":"My Task"}}"#;
//! let domain: TaskEntity = migrator.load_with_fallback("task", versioned_json)?;
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

pub mod dir_storage;
pub mod errors;
mod migrator;
pub mod paths;
pub mod storage;

// Re-export the derive macros
pub use version_migrate_macro::Versioned;

// Re-export Queryable derive macro (same name as trait is OK in Rust)
#[doc(inline)]
pub use version_migrate_macro::Queryable as DeriveQueryable;

// Re-export VersionMigrate derive macro
#[doc(inline)]
pub use version_migrate_macro::VersionMigrate;
/// Creates a migration path with simplified syntax.
///
/// This macro provides a concise way to define migration paths between versioned types.
///
/// # Syntax
///
/// Basic usage:
/// ```ignore
/// migrator!("entity", [V1, V2, V3])
/// ```
///
/// With custom version/data keys:
/// ```ignore
/// migrator!("entity", [V1, V2, V3], version_key = "v", data_key = "d")
/// ```
///
/// # Arguments
///
/// * `entity` - The entity name as a string literal (e.g., `"user"`, `"task"`)
/// * `versions` - A list of version types in migration order (e.g., `[V1, V2, V3]`)
/// * `version_key` - (Optional) Custom key for the version field (default: `"version"`)
/// * `data_key` - (Optional) Custom key for the data field (default: `"data"`)
///
/// # Examples
///
/// ```ignore
/// use version_migrate::{migrator, Migrator};
///
/// // Simple two-step migration
/// let path = migrator!("task", [TaskV1, TaskV2]);
///
/// // Multi-step migration
/// let path = migrator!("task", [TaskV1, TaskV2, TaskV3]);
///
/// // Many versions (arbitrary length supported)
/// let path = migrator!("task", [TaskV1, TaskV2, TaskV3, TaskV4, TaskV5, TaskV6]);
///
/// // With custom keys
/// let path = migrator!("task", [TaskV1, TaskV2], version_key = "v", data_key = "d");
///
/// // Register with migrator
/// let mut migrator = Migrator::new();
/// migrator.register(path).unwrap();
/// ```
///
/// # Generated Code
///
/// The macro expands to the equivalent builder pattern:
/// ```ignore
/// // migrator!("entity", [V1, V2])
/// // expands to:
/// Migrator::define("entity")
///     .from::<V1>()
///     .into::<V2>()
/// ```
#[macro_export]
macro_rules! migrator {
    // Basic: migrator!("entity", [V1, V2, V3, ...])
    ($entity:expr, [$first:ty, $($rest:ty),+ $(,)?]) => {
        $crate::migrator_vec_helper!($first; $($rest),+; $entity)
    };

    // With custom keys: migrator!("entity", [V1, V2, ...], version_key = "v", data_key = "d")
    ($entity:expr, [$first:ty, $($rest:ty),+ $(,)?], version_key = $version_key:expr, data_key = $data_key:expr) => {
        $crate::migrator_vec_helper_with_keys!($first; $($rest),+; $entity; $version_key; $data_key)
    };
}

/// Helper macro for Vec notation without custom keys
#[doc(hidden)]
#[macro_export]
macro_rules! migrator_vec_helper {
    // Base case: two versions left
    ($first:ty; $last:ty; $entity:expr) => {
        $crate::Migrator::define($entity)
            .from::<$first>()
            .into::<$last>()
    };

    // Recursive case: more than two versions
    ($first:ty; $second:ty, $($rest:ty),+; $entity:expr) => {
        $crate::migrator_vec_build_steps!($first; $($rest),+; $entity; {
            $crate::Migrator::define($entity).from::<$first>().step::<$second>()
        })
    };
}

/// Helper for building all steps, then applying final .into()
#[doc(hidden)]
#[macro_export]
macro_rules! migrator_vec_build_steps {
    // Final case: last version, call .into()
    ($first:ty; $last:ty; $entity:expr; { $builder:expr }) => {
        $builder.into::<$last>()
    };

    // Recursive case: add .step() and continue
    ($first:ty; $current:ty, $($rest:ty),+; $entity:expr; { $builder:expr }) => {
        $crate::migrator_vec_build_steps!($first; $($rest),+; $entity; {
            $builder.step::<$current>()
        })
    };
}

/// Helper macro for Vec notation with custom keys
#[doc(hidden)]
#[macro_export]
macro_rules! migrator_vec_helper_with_keys {
    // Base case: two versions left
    ($first:ty; $last:ty; $entity:expr; $version_key:expr; $data_key:expr) => {
        $crate::Migrator::define($entity)
            .with_keys($version_key, $data_key)
            .from::<$first>()
            .into::<$last>()
    };

    // Recursive case: more than two versions
    ($first:ty; $second:ty, $($rest:ty),+; $entity:expr; $version_key:expr; $data_key:expr) => {
        $crate::migrator_vec_build_steps_with_keys!($first; $($rest),+; $entity; $version_key; $data_key; {
            $crate::Migrator::define($entity).with_keys($version_key, $data_key).from::<$first>().step::<$second>()
        })
    };
}

/// Helper for building all steps with custom keys, then applying final .into()
#[doc(hidden)]
#[macro_export]
macro_rules! migrator_vec_build_steps_with_keys {
    // Final case: last version, call .into()
    ($first:ty; $last:ty; $entity:expr; $version_key:expr; $data_key:expr; { $builder:expr }) => {
        $builder.into::<$last>()
    };

    // Recursive case: add .step() and continue
    ($first:ty; $current:ty, $($rest:ty),+; $entity:expr; $version_key:expr; $data_key:expr; { $builder:expr }) => {
        $crate::migrator_vec_build_steps_with_keys!($first; $($rest),+; $entity; $version_key; $data_key; {
            $builder.step::<$current>()
        })
    };
}

// Re-export error types
pub use errors::{IoOperationKind, MigrationError};

// Re-export migrator types
pub use migrator::{ConfigMigrator, MigrationPath, Migrator};

// Re-export storage types
pub use storage::{
    AtomicWriteConfig, FileStorage, FileStorageStrategy, FormatStrategy, LoadBehavior,
};

// Re-export dir_storage types
pub use dir_storage::{DirStorage, DirStorageStrategy, FilenameEncoding};

#[cfg(feature = "async")]
pub use dir_storage::AsyncDirStorage;

// Re-export paths types
pub use paths::{AppPaths, PathStrategy, PrefPath};

// Re-export async-trait for user convenience
#[cfg(feature = "async")]
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

/// Converts a domain model back into a versioned DTO.
///
/// This trait should be implemented on versioned DTOs to enable conversion
/// from the domain model back to the versioned format for serialization.
///
/// # Example
///
/// ```ignore
/// impl FromDomain<TaskEntity> for TaskV1_1_0 {
///     fn from_domain(domain: TaskEntity) -> Self {
///         TaskV1_1_0 {
///             id: domain.id,
///             title: domain.title,
///             description: domain.description,
///         }
///     }
/// }
/// ```
pub trait FromDomain<D>: Versioned + Serialize {
    /// Converts a domain model into this versioned format.
    fn from_domain(domain: D) -> Self;
}

/// Associates a domain entity with its latest versioned representation.
///
/// This trait enables automatic saving of domain entities using their latest version.
/// It should typically be derived using the `#[version_migrate]` attribute macro.
///
/// # Example
///
/// ```ignore
/// #[derive(Serialize, Deserialize)]
/// #[version_migrate(entity = "task", latest = TaskV1_1_0)]
/// struct TaskEntity {
///     id: String,
///     title: String,
///     description: Option<String>,
/// }
///
/// // Now you can save entities directly
/// let entity = TaskEntity { ... };
/// let json = migrator.save_entity(entity)?;
/// ```
pub trait LatestVersioned: Sized {
    /// The latest versioned type for this entity.
    type Latest: Versioned + Serialize + FromDomain<Self>;

    /// The entity name used for migration paths.
    const ENTITY_NAME: &'static str;

    /// Whether this entity supports saving functionality.
    /// When `false` (default), uses `into()` for read-only access.
    /// When `true`, uses `into_with_save()` to enable domain entity saving.
    const SAVE: bool = false;

    /// Converts this domain entity into its latest versioned format.
    fn to_latest(self) -> Self::Latest {
        Self::Latest::from_domain(self)
    }
}

/// Marks a domain type as queryable, associating it with an entity name.
///
/// This trait enables `ConfigMigrator` to automatically determine which entity
/// path to use when querying or updating data.
///
/// # Example
///
/// ```ignore
/// impl Queryable for TaskEntity {
///     const ENTITY_NAME: &'static str = "task";
/// }
///
/// let tasks: Vec<TaskEntity> = config.query("tasks")?;
/// ```
pub trait Queryable {
    /// The entity name used to look up migration paths in the `Migrator`.
    const ENTITY_NAME: &'static str;
}

/// Async version of `MigratesTo` for migrations requiring I/O operations.
///
/// Use this trait when migrations need to perform asynchronous operations
/// such as database queries or API calls.
#[cfg(feature = "async")]
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
#[cfg(feature = "async")]
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
