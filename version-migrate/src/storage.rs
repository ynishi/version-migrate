//! File storage layer with migration support.
//!
//! Provides `FileStorage`, which wraps `local_store::FileStorage` for raw ACID
//! file operations and layers `ConfigMigrator`-based schema evolution on top.

use crate::{ConfigMigrator, MigrationError, Migrator, Queryable};
use local_store::{FileStorageStrategy, FormatStrategy, LoadBehavior};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

/// File storage with ACID guarantees and automatic migrations.
///
/// Provides:
/// - **Atomicity**: Updates are all-or-nothing via tmp file + atomic rename
/// - **Consistency**: Format validation on load/save
/// - **Isolation**: File locking prevents concurrent modifications
/// - **Durability**: Explicit fsync before rename
///
/// Raw IO (`atomic_rename`, `get_temp_path`, `cleanup_temp_files`) lives
/// exclusively inside `local_store::FileStorage`.
pub struct FileStorage {
    /// Raw ACID-safe file store (no migration knowledge).
    inner: local_store::FileStorage,
    /// In-memory versioned configuration (migration layer).
    config: ConfigMigrator,
    /// Strategy governing format, load behaviour, etc.
    strategy: FileStorageStrategy,
}

impl FileStorage {
    /// Create a new FileStorage instance and load data from file.
    ///
    /// This combines initialization and loading into a single operation.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the storage file
    /// * `migrator` - Migrator instance with registered migration paths
    /// * `strategy` - Storage strategy configuration
    ///
    /// # Behavior
    ///
    /// Depends on `strategy.load_behavior`:
    /// - `CreateIfMissing`: Creates empty config if file doesn't exist
    /// - `SaveIfMissing`: Creates empty config and saves it if file doesn't exist
    /// - `ErrorIfMissing`: Returns error if file doesn't exist
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

    /// Save current state to file atomically.
    ///
    /// Serialises the `ConfigMigrator` value to the configured format (TOML or
    /// JSON) and delegates the atomic write (tmp file + fsync + rename) to
    /// `local_store::FileStorage::write_string`.
    pub fn save(&self) -> Result<(), MigrationError> {
        let json_value = self.config.as_value();

        let content = match self.strategy.format {
            FormatStrategy::Toml => {
                let tv = local_store::json_to_toml(json_value).map_err(|e| {
                    MigrationError::Store(local_store::StoreError::FormatConvert(e))
                })?;
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

    /// Get immutable reference to the ConfigMigrator.
    pub fn config(&self) -> &ConfigMigrator {
        &self.config
    }

    /// Get mutable reference to the ConfigMigrator.
    pub fn config_mut(&mut self) -> &mut ConfigMigrator {
        &mut self.config
    }

    /// Query entities from storage.
    ///
    /// Delegates to `ConfigMigrator::query()`.
    pub fn query<T>(&self, key: &str) -> Result<Vec<T>, MigrationError>
    where
        T: Queryable + for<'de> serde::Deserialize<'de>,
    {
        self.config.query(key)
    }

    /// Update entities in memory (does not save to file).
    ///
    /// Delegates to `ConfigMigrator::update()`.
    pub fn update<T>(&mut self, key: &str, value: Vec<T>) -> Result<(), MigrationError>
    where
        T: Queryable + serde::Serialize,
    {
        self.config.update(key, value)
    }

    /// Update entities and immediately save to file atomically.
    pub fn update_and_save<T>(&mut self, key: &str, value: Vec<T>) -> Result<(), MigrationError>
    where
        T: Queryable + serde::Serialize,
    {
        self.update(key, value)?;
        self.save()
    }

    /// Returns a reference to the storage file path.
    ///
    /// # Returns
    ///
    /// A reference to the file path where the configuration is stored.
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
        migrator.register(path).unwrap();
        migrator
    }

    #[test]
    fn test_file_storage_strategy_builder() {
        let strategy = FileStorageStrategy::new()
            .with_format(FormatStrategy::Json)
            .with_retry_count(5)
            .with_cleanup(false)
            .with_load_behavior(LoadBehavior::ErrorIfMissing);

        assert_eq!(strategy.format, FormatStrategy::Json);
        assert_eq!(strategy.atomic_write.retry_count, 5);
        assert!(!strategy.atomic_write.cleanup_tmp_files);
        assert_eq!(strategy.load_behavior, LoadBehavior::ErrorIfMissing);
    }

    #[test]
    fn test_save_and_load_toml() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::default(); // TOML by default

        let mut storage = FileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        // Update and save
        let entities = vec![TestEntity {
            name: "test".to_string(),
            count: 42,
        }];
        storage.update_and_save("test", entities).unwrap();

        // Create new storage and load from saved file
        let migrator2 = setup_migrator();
        let storage2 =
            FileStorage::new(file_path, migrator2, FileStorageStrategy::default()).unwrap();

        // Query and verify
        let loaded: Vec<TestEntity> = storage2.query("test").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "test");
        assert_eq!(loaded[0].count, 42);
    }

    #[test]
    fn test_save_and_load_json() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.json");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::new().with_format(FormatStrategy::Json);

        let mut storage = FileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        // Update and save
        let entities = vec![TestEntity {
            name: "json_test".to_string(),
            count: 100,
        }];
        storage.update_and_save("test", entities).unwrap();

        // Create new storage and load from saved file
        let migrator2 = setup_migrator();
        let strategy2 = FileStorageStrategy::new().with_format(FormatStrategy::Json);
        let storage2 = FileStorage::new(file_path, migrator2, strategy2).unwrap();

        // Query and verify
        let loaded: Vec<TestEntity> = storage2.query("test").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "json_test");
        assert_eq!(loaded[0].count, 100);
    }

    #[test]
    fn test_load_behavior_create_if_missing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::CreateIfMissing);

        let result = FileStorage::new(file_path, migrator, strategy);

        assert!(result.is_ok()); // Should not error when file doesn't exist
    }

    #[test]
    fn test_load_behavior_error_if_missing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::ErrorIfMissing);

        let result = FileStorage::new(file_path, migrator, strategy);

        assert!(result.is_err()); // Should error when file doesn't exist
        assert!(matches!(
            result,
            Err(MigrationError::Store(
                local_store::StoreError::IoError { .. }
            ))
        ));
    }

    #[test]
    fn test_load_behavior_save_if_missing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("save_if_missing.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::SaveIfMissing);

        // File should not exist initially
        assert!(!file_path.exists());

        let result = FileStorage::new(file_path.clone(), migrator, strategy.clone());

        // Should succeed and create the file
        assert!(result.is_ok());
        assert!(file_path.exists());

        // Verify we can read the file back
        let _storage = result.unwrap();
        let reloaded = FileStorage::new(file_path.clone(), setup_migrator(), strategy);
        assert!(reloaded.is_ok());
    }

    #[test]
    fn test_save_if_missing_with_default_value() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("default_value.toml");
        let migrator = setup_migrator();

        // Create default value with version info (using the latest version 2.0.0)
        let default_value = serde_json::json!({
            "test": [
                {
                    "version": "2.0.0",
                    "name": "default_user",
                    "count": 99
                }
            ]
        });

        let strategy = FileStorageStrategy::new()
            .with_load_behavior(LoadBehavior::SaveIfMissing)
            .with_default_value(default_value);

        // File should not exist initially
        assert!(!file_path.exists());

        let storage = FileStorage::new(file_path.clone(), migrator, strategy.clone()).unwrap();

        // File should have been created
        assert!(file_path.exists());

        // Verify the default value was saved
        let loaded: Vec<TestEntity> = storage.query("test").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "default_user");
        assert_eq!(loaded[0].count, 99);

        // Load again and verify persistence
        let reloaded = FileStorage::new(file_path.clone(), setup_migrator(), strategy).unwrap();
        let reloaded_entities: Vec<TestEntity> = reloaded.query("test").unwrap();
        assert_eq!(reloaded_entities.len(), 1);
        assert_eq!(reloaded_entities[0].name, "default_user");
        assert_eq!(reloaded_entities[0].count, 99);
    }

    #[test]
    fn test_create_if_missing_with_default_value() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("create_default.toml");
        let migrator = setup_migrator();

        let default_value = serde_json::json!({
            "test": [{
                "version": "2.0.0",
                "name": "created",
                "count": 42
            }]
        });

        let strategy = FileStorageStrategy::new()
            .with_load_behavior(LoadBehavior::CreateIfMissing)
            .with_default_value(default_value);

        // CreateIfMissing should not save the file automatically
        let storage = FileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        // Query should work with the default value in memory
        let loaded: Vec<TestEntity> = storage.query("test").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "created");
        assert_eq!(loaded[0].count, 42);
    }

    #[test]
    fn test_atomic_write_no_tmp_file_left() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("atomic.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::default();

        let mut storage = FileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        let entities = vec![TestEntity {
            name: "atomic".to_string(),
            count: 1,
        }];
        storage.update_and_save("test", entities).unwrap();

        // Verify no temp file left behind
        let entries: Vec<_> = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();

        let tmp_files: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".atomic.toml.tmp")
            })
            .collect();

        assert_eq!(tmp_files.len(), 0, "Temporary files should be cleaned up");
    }

    #[test]
    fn test_file_storage_path() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_config.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::default();

        let storage = FileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        // Verify path() returns the expected path
        let returned_path = storage.path();
        assert_eq!(returned_path, file_path.as_path());
    }

    #[test]
    fn test_load_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("empty.toml");

        // Create an empty file
        std::fs::write(&file_path, "").unwrap();

        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::default();

        // Should handle empty file gracefully (treat as empty JSON {})
        let result = FileStorage::new(file_path, migrator, strategy);
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_whitespace_only_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("whitespace.toml");

        // Create a file with only whitespace
        std::fs::write(&file_path, "   \n\t\n  ").unwrap();

        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::default();

        // Should handle whitespace-only file gracefully
        let result = FileStorage::new(file_path, migrator, strategy);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_accessors() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("config_access.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::default();

        let mut storage = FileStorage::new(file_path, migrator, strategy).unwrap();

        // Test config() immutable access
        let _config = storage.config();

        // Test config_mut() mutable access
        let _config_mut = storage.config_mut();
    }

    #[test]
    fn test_save_creates_parent_directory() {
        let temp_dir = TempDir::new().unwrap();
        // Create a path with non-existent parent directory
        let file_path = temp_dir
            .path()
            .join("subdir")
            .join("nested")
            .join("config.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::CreateIfMissing);

        let storage = FileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        // Save should create parent directories
        storage.save().unwrap();

        assert!(file_path.exists());
        assert!(file_path.parent().unwrap().exists());
    }

    #[test]
    fn test_cleanup_with_multiple_temp_files() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("cleanup_test.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::default();

        // Create some fake old temp files
        let fake_tmp1 = temp_dir.path().join(".cleanup_test.toml.tmp.99999");
        let fake_tmp2 = temp_dir.path().join(".cleanup_test.toml.tmp.88888");
        std::fs::write(&fake_tmp1, "old temp 1").unwrap();
        std::fs::write(&fake_tmp2, "old temp 2").unwrap();

        let mut storage = FileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        // Update and save - should cleanup old temp files
        let entities = vec![TestEntity {
            name: "cleanup".to_string(),
            count: 1,
        }];
        storage.update_and_save("test", entities).unwrap();

        // Old temp files should be cleaned up
        assert!(!fake_tmp1.exists());
        assert!(!fake_tmp2.exists());
    }

    #[test]
    fn test_save_without_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("no_cleanup.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::new().with_cleanup(false);

        // Create a fake old temp file
        let fake_tmp = temp_dir.path().join(".no_cleanup.toml.tmp.99999");
        std::fs::write(&fake_tmp, "old temp").unwrap();

        let mut storage = FileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        let entities = vec![TestEntity {
            name: "no_cleanup".to_string(),
            count: 1,
        }];
        storage.update_and_save("test", entities).unwrap();

        // Old temp file should NOT be cleaned up when cleanup is disabled
        assert!(fake_tmp.exists());
    }

    #[test]
    fn test_update_without_save() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("update_no_save.toml");
        let migrator = setup_migrator();
        let strategy = FileStorageStrategy::default();

        let mut storage = FileStorage::new(file_path.clone(), migrator, strategy).unwrap();

        // Update without save
        let entities = vec![TestEntity {
            name: "memory_only".to_string(),
            count: 42,
        }];
        storage.update("test", entities).unwrap();

        // Query should return the updated data (in memory)
        let loaded: Vec<TestEntity> = storage.query("test").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "memory_only");

        // File should not exist (never saved)
        assert!(!file_path.exists());
    }

    #[test]
    fn test_atomic_write_config_default() {
        let config = local_store::AtomicWriteConfig::default();
        assert_eq!(config.retry_count, 3);
        assert!(config.cleanup_tmp_files);
    }

    #[test]
    fn test_format_strategy_equality() {
        assert_eq!(FormatStrategy::Toml, FormatStrategy::Toml);
        assert_eq!(FormatStrategy::Json, FormatStrategy::Json);
        assert_ne!(FormatStrategy::Toml, FormatStrategy::Json);
    }

    #[test]
    fn test_load_behavior_equality() {
        assert_eq!(LoadBehavior::CreateIfMissing, LoadBehavior::CreateIfMissing);
        assert_eq!(LoadBehavior::SaveIfMissing, LoadBehavior::SaveIfMissing);
        assert_eq!(LoadBehavior::ErrorIfMissing, LoadBehavior::ErrorIfMissing);
        assert_ne!(LoadBehavior::CreateIfMissing, LoadBehavior::ErrorIfMissing);
    }
}
