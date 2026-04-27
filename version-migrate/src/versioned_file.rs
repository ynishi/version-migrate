//! Version-aware file storage wrapper.
//!
//! Provides `VersionedFileStorage`, which wraps `local_store::FileStorage` for
//! raw ACID file operations and layers `ConfigMigrator`-based schema evolution
//! on top.

use crate::{ConfigMigrator, MigrationError, Migrator, Queryable};
use local_store::{FileStorageStrategy, FormatStrategy, LoadBehavior};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

/// Version-aware file storage that wraps `local_store::FileStorage`.
///
/// # Responsibilities
///
/// This struct handles **only**:
/// - Constructing a `ConfigMigrator` from the file content on init.
/// - Format dispatch (TOML ↔ JSON serialisation) on save.
/// - Delegating all ACID / atomic-rename / lock operations to `inner`.
///
/// Raw IO (`atomic_rename`, `get_temp_path`, `cleanup_temp_files`) lives
/// exclusively inside `local_store::FileStorage`.
pub struct VersionedFileStorage {
    /// Raw ACID-safe file store (no migration knowledge).
    inner: local_store::FileStorage,
    /// In-memory versioned configuration (migration layer).
    config: ConfigMigrator,
    /// Strategy governing format, load behaviour, etc.
    strategy: FileStorageStrategy,
}

impl VersionedFileStorage {
    /// Create a new `VersionedFileStorage` instance and load data from the file.
    ///
    /// Reads the file (if present), applies TOML→JSON conversion when needed,
    /// constructs a `ConfigMigrator` from the resulting JSON string, and—when
    /// `LoadBehavior::SaveIfMissing` is configured—writes the initial content to
    /// disk.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the storage file.
    /// * `migrator` - `Migrator` instance with registered migration paths.
    /// * `strategy` - Storage strategy (format, load behaviour, atomic-write config).
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::Store` on IO failure or
    /// `MigrationError::TomlParseError` / `MigrationError::SerializationError` on
    /// format conversion failure.  Returns `MigrationError::Store(IoError)` when
    /// `LoadBehavior::ErrorIfMissing` is set and the file is absent.
    pub fn new(
        path: PathBuf,
        migrator: Migrator,
        strategy: FileStorageStrategy,
    ) -> Result<Self, MigrationError> {
        // Track whether the file existed before we open it.
        let file_was_missing = !path.exists();

        // Build an inner strategy that always uses CreateIfMissing so the raw
        // layer does not interfere with our own LoadBehavior logic.
        let inner_strategy = FileStorageStrategy {
            load_behavior: LoadBehavior::CreateIfMissing,
            ..strategy.clone()
        };
        let inner = local_store::FileStorage::new(path.clone(), inner_strategy)
            .map_err(MigrationError::Store)?;

        // Determine the JSON string we hand to ConfigMigrator.
        let json_string = if !file_was_missing {
            // File existed: read it and convert to JSON.
            let raw = inner.read_string().map_err(MigrationError::Store)?;
            if raw.trim().is_empty() {
                "{}".to_string()
            } else {
                match strategy.format {
                    FormatStrategy::Toml => {
                        let tv: toml::Value = toml::from_str(&raw)
                            .map_err(|e| MigrationError::TomlParseError(e.to_string()))?;
                        let jv = toml_to_json(tv)?;
                        serde_json::to_string(&jv)
                            .map_err(|e| MigrationError::SerializationError(e.to_string()))?
                    }
                    FormatStrategy::Json => raw,
                }
            }
        } else {
            // File was missing: apply LoadBehavior.
            match strategy.load_behavior {
                LoadBehavior::ErrorIfMissing => {
                    return Err(MigrationError::Store(local_store::StoreError::IoError {
                        operation: local_store::IoOperationKind::Read,
                        path: path.display().to_string(),
                        context: None,
                        error: "File not found".to_string(),
                    }));
                }
                LoadBehavior::CreateIfMissing | LoadBehavior::SaveIfMissing => {
                    if let Some(ref default_value) = strategy.default_value {
                        serde_json::to_string(default_value)
                            .map_err(|e| MigrationError::SerializationError(e.to_string()))?
                    } else {
                        "{}".to_string()
                    }
                }
            }
        };

        let config = ConfigMigrator::from(&json_string, migrator)?;
        let storage = Self {
            inner,
            config,
            strategy,
        };

        // When SaveIfMissing is set and the file was absent, persist now.
        if file_was_missing && storage.strategy.load_behavior == LoadBehavior::SaveIfMissing {
            storage.save()?;
        }

        Ok(storage)
    }

    /// Save the current in-memory state to the file atomically.
    ///
    /// Serialises the `ConfigMigrator` value to the configured format (TOML or
    /// JSON) and delegates the atomic write (tmp file + fsync + rename) to
    /// `local_store::FileStorage::write_string`.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::Store` on IO failure or
    /// `MigrationError::TomlSerializeError` / `MigrationError::SerializationError`
    /// on serialisation failure.
    pub fn save(&self) -> Result<(), MigrationError> {
        let json_value = self.config.as_value();

        let content = match self.strategy.format {
            FormatStrategy::Toml => {
                let tv = json_to_toml(json_value)?;
                toml::to_string_pretty(&tv)
                    .map_err(|e| MigrationError::TomlSerializeError(e.to_string()))?
            }
            FormatStrategy::Json => serde_json::to_string_pretty(json_value)
                .map_err(|e| MigrationError::SerializationError(e.to_string()))?,
        };

        self.inner
            .write_string(&content)
            .map_err(MigrationError::Store)
    }

    /// Get an immutable reference to the `ConfigMigrator`.
    pub fn config(&self) -> &ConfigMigrator {
        &self.config
    }

    /// Get a mutable reference to the `ConfigMigrator`.
    pub fn config_mut(&mut self) -> &mut ConfigMigrator {
        &mut self.config
    }

    /// Query entities from the in-memory configuration.
    ///
    /// Delegates to `ConfigMigrator::query`.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if the key is not found or deserialisation fails.
    pub fn query<T>(&self, key: &str) -> Result<Vec<T>, MigrationError>
    where
        T: Queryable + for<'de> serde::Deserialize<'de>,
    {
        self.config.query(key)
    }

    /// Update entities in memory without saving to disk.
    ///
    /// Delegates to `ConfigMigrator::update`.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if serialisation or internal update fails.
    pub fn update<T>(&mut self, key: &str, value: Vec<T>) -> Result<(), MigrationError>
    where
        T: Queryable + serde::Serialize,
    {
        self.config.update(key, value)
    }

    /// Update entities in memory and immediately save to disk atomically.
    ///
    /// Combines `update` and `save` in a single operation.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` on update or IO failure.
    pub fn update_and_save<T>(&mut self, key: &str, value: Vec<T>) -> Result<(), MigrationError>
    where
        T: Queryable + serde::Serialize,
    {
        self.update(key, value)?;
        self.save()
    }

    /// Returns a reference to the storage file path.
    pub fn path(&self) -> &Path {
        self.inner.path()
    }
}

// ============================================================================
// Private format-conversion helpers
// ============================================================================

/// Convert a `toml::Value` to a `serde_json::Value`.
fn toml_to_json(toml_value: toml::Value) -> Result<JsonValue, MigrationError> {
    let json_str = serde_json::to_string(&toml_value)
        .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
    let json_value: JsonValue = serde_json::from_str(&json_str)
        .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;
    Ok(json_value)
}

/// Convert a `serde_json::Value` to a `toml::Value`.
fn json_to_toml(json_value: &JsonValue) -> Result<toml::Value, MigrationError> {
    let json_str = serde_json::to_string(json_value)
        .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
    let toml_value: toml::Value = serde_json::from_str(&json_str)
        .map_err(|e| MigrationError::TomlParseError(e.to_string()))?;
    Ok(toml_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntoDomain, MigratesTo, Versioned};
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestEntity {
        name: String,
        count: u32,
    }

    impl Queryable for TestEntity {
        const ENTITY_NAME: &'static str = "test";
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestV1 {
        name: String,
    }

    impl Versioned for TestV1 {
        const VERSION: &'static str = "1.0.0";
    }

    impl MigratesTo<TestV2> for TestV1 {
        fn migrate(self) -> TestV2 {
            TestV2 {
                name: self.name,
                count: 0,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestV2 {
        name: String,
        count: u32,
    }

    impl Versioned for TestV2 {
        const VERSION: &'static str = "2.0.0";
    }

    impl IntoDomain<TestEntity> for TestV2 {
        fn into_domain(self) -> TestEntity {
            TestEntity {
                name: self.name,
                count: self.count,
            }
        }
    }

    fn setup_migrator() -> Migrator {
        let path = Migrator::define("test")
            .from::<TestV1>()
            .step::<TestV2>()
            .into::<TestEntity>();

        let mut migrator = Migrator::new();
        // SAFETY: register only fails on circular paths or duplicate entity names,
        // neither of which applies to this static test setup.
        migrator.register(path).unwrap();
        migrator
    }

    #[test]
    fn test_versioned_file_storage_new_create_if_missing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::CreateIfMissing);

        let result = VersionedFileStorage::new(file_path, migrator, strategy);
        assert!(result.is_ok());
    }

    #[test]
    fn test_versioned_file_storage_new_error_if_missing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::ErrorIfMissing);

        let result = VersionedFileStorage::new(file_path, migrator, strategy);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(MigrationError::Store(
                local_store::StoreError::IoError { .. }
            ))
        ));
    }

    #[test]
    fn test_versioned_file_storage_save_and_reload() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("data.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::default();

        let mut storage = VersionedFileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        let entities = vec![TestEntity {
            name: "hello".to_string(),
            count: 7,
        }];
        storage.update_and_save("test", entities).unwrap();

        let migrator2 = setup_migrator();
        let storage2 =
            VersionedFileStorage::new(file_path, migrator2, FileStorageStrategy::default())
                .unwrap();

        let loaded: Vec<TestEntity> = storage2.query("test").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "hello");
        assert_eq!(loaded[0].count, 7);
    }

    #[test]
    fn test_versioned_file_storage_save_if_missing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("save_if_missing.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::SaveIfMissing);

        assert!(!file_path.exists());

        let result = VersionedFileStorage::new(file_path.clone(), migrator, strategy);
        assert!(result.is_ok());
        assert!(file_path.exists());
    }

    #[test]
    fn test_versioned_file_storage_path() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("config.toml");
        let migrator = setup_migrator();

        let storage =
            VersionedFileStorage::new(file_path.clone(), migrator, FileStorageStrategy::default())
                .unwrap();

        assert_eq!(storage.path(), file_path.as_path());
    }

    #[test]
    fn test_versioned_file_storage_config_accessors() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("config.toml");
        let migrator = setup_migrator();

        let mut storage =
            VersionedFileStorage::new(file_path, migrator, FileStorageStrategy::default()).unwrap();

        let _config = storage.config();
        let _config_mut = storage.config_mut();
    }
}
