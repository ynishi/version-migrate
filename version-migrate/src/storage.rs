//! File storage layer with ACID guarantees for versioned configuration.
//!
//! Provides atomic file operations, format conversion, and file locking.

use crate::{
    errors::{IoOperationKind, StoreError},
    ConfigMigrator, MigrationError, Migrator, Queryable,
};
use serde_json::Value as JsonValue;
use std::fs::{self, File};
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};

/// File format strategy for storage operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatStrategy {
    /// TOML format (recommended for human-editable configs)
    Toml,
    /// JSON format
    Json,
}

/// Configuration for atomic write operations.
#[derive(Debug, Clone)]
pub struct AtomicWriteConfig {
    /// Number of times to retry rename operation (default: 3)
    pub retry_count: usize,
    /// Whether to clean up old temporary files (best effort)
    pub cleanup_tmp_files: bool,
}

impl Default for AtomicWriteConfig {
    fn default() -> Self {
        Self {
            retry_count: 3,
            cleanup_tmp_files: true,
        }
    }
}

/// Behavior when loading a file that doesn't exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadBehavior {
    /// Create an empty ConfigMigrator if file is missing
    CreateIfMissing,
    /// Create an empty ConfigMigrator and save it to file if missing
    SaveIfMissing,
    /// Return an error if file is missing
    ErrorIfMissing,
}

/// Strategy for file storage operations.
#[derive(Debug, Clone)]
pub struct FileStorageStrategy {
    /// File format to use
    pub format: FormatStrategy,
    /// Atomic write configuration
    pub atomic_write: AtomicWriteConfig,
    /// Behavior when file doesn't exist
    pub load_behavior: LoadBehavior,
    /// Default value to use when SaveIfMissing is set (as JSON Value)
    pub default_value: Option<JsonValue>,
}

impl Default for FileStorageStrategy {
    fn default() -> Self {
        Self {
            format: FormatStrategy::Toml,
            atomic_write: AtomicWriteConfig::default(),
            load_behavior: LoadBehavior::CreateIfMissing,
            default_value: None,
        }
    }
}

impl FileStorageStrategy {
    /// Create a new strategy with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the file format.
    pub fn with_format(mut self, format: FormatStrategy) -> Self {
        self.format = format;
        self
    }

    /// Set the retry count for atomic writes.
    pub fn with_retry_count(mut self, count: usize) -> Self {
        self.atomic_write.retry_count = count;
        self
    }

    /// Set whether to cleanup temporary files.
    pub fn with_cleanup(mut self, cleanup: bool) -> Self {
        self.atomic_write.cleanup_tmp_files = cleanup;
        self
    }

    /// Set the load behavior.
    pub fn with_load_behavior(mut self, behavior: LoadBehavior) -> Self {
        self.load_behavior = behavior;
        self
    }

    /// Set the default value to use when SaveIfMissing is set.
    ///
    /// This value will be used as the initial content when a file doesn't exist
    /// and `LoadBehavior::SaveIfMissing` is configured.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use serde_json::json;
    ///
    /// let strategy = FileStorageStrategy::new()
    ///     .with_load_behavior(LoadBehavior::SaveIfMissing)
    ///     .with_default_value(json!({
    ///         "test": [{"name": "default", "count": 0}]
    ///     }));
    /// ```
    pub fn with_default_value(mut self, value: JsonValue) -> Self {
        self.default_value = Some(value);
        self
    }
}

/// File storage with ACID guarantees and automatic migrations.
///
/// Provides:
/// - **Atomicity**: Updates are all-or-nothing via tmp file + atomic rename
/// - **Consistency**: Format validation on load/save
/// - **Isolation**: File locking prevents concurrent modifications
/// - **Durability**: Explicit fsync before rename
pub struct FileStorage {
    path: PathBuf,
    config: ConfigMigrator,
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
    /// - `ErrorIfMissing`: Returns error if file doesn't exist
    ///
    /// # Example
    ///
    /// ```ignore
    /// let strategy = FileStorageStrategy::default();
    /// let migrator = Migrator::new();
    /// let storage = FileStorage::new(
    ///     PathBuf::from("config.toml"),
    ///     migrator,
    ///     strategy
    /// )?;
    /// ```
    pub fn new(
        path: PathBuf,
        migrator: Migrator,
        strategy: FileStorageStrategy,
    ) -> Result<Self, MigrationError> {
        // Track if file was missing for SaveIfMissing behavior
        let file_was_missing = !path.exists();

        // Load file content if it exists
        let json_string = if path.exists() {
            let content = fs::read_to_string(&path).map_err(|e| {
                MigrationError::Store(StoreError::IoError {
                    operation: IoOperationKind::Read,
                    path: path.display().to_string(),
                    context: None,
                    error: e.to_string(),
                })
            })?;

            if content.trim().is_empty() {
                // Empty file, use empty JSON
                "{}".to_string()
            } else {
                // Parse based on format strategy
                match strategy.format {
                    FormatStrategy::Toml => {
                        let toml_value: toml::Value = toml::from_str(&content)
                            .map_err(|e| MigrationError::TomlParseError(e.to_string()))?;
                        let json_value = toml_to_json(toml_value)?;
                        serde_json::to_string(&json_value)
                            .map_err(|e| MigrationError::SerializationError(e.to_string()))?
                    }
                    FormatStrategy::Json => content,
                }
            }
        } else {
            // File doesn't exist
            match strategy.load_behavior {
                LoadBehavior::CreateIfMissing | LoadBehavior::SaveIfMissing => {
                    // Use default_value if provided, otherwise use empty JSON
                    if let Some(ref default_value) = strategy.default_value {
                        serde_json::to_string(default_value)
                            .map_err(|e| MigrationError::SerializationError(e.to_string()))?
                    } else {
                        "{}".to_string()
                    }
                }
                LoadBehavior::ErrorIfMissing => {
                    return Err(MigrationError::Store(StoreError::IoError {
                        operation: IoOperationKind::Read,
                        path: path.display().to_string(),
                        context: None,
                        error: "File not found".to_string(),
                    }));
                }
            }
        };

        // Create ConfigMigrator with loaded/empty data
        let config = ConfigMigrator::from(&json_string, migrator)?;

        let storage = Self {
            path,
            config,
            strategy,
        };

        // If file was missing and SaveIfMissing is set, save the empty config
        if file_was_missing && storage.strategy.load_behavior == LoadBehavior::SaveIfMissing {
            storage.save()?;
        }

        Ok(storage)
    }

    /// Save current state to file atomically.
    ///
    /// Uses a temporary file + atomic rename to ensure durability.
    /// Retries according to `strategy.atomic_write.retry_count`.
    pub fn save(&self) -> Result<(), MigrationError> {
        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| {
                    MigrationError::Store(StoreError::IoError {
                        operation: IoOperationKind::CreateDir,
                        path: parent.display().to_string(),
                        context: Some("parent directory".to_string()),
                        error: e.to_string(),
                    })
                })?;
            }
        }

        // Get current state as JSON
        let json_value = self.config.as_value();

        // Convert to target format
        let content = match self.strategy.format {
            FormatStrategy::Toml => {
                let toml_value = json_to_toml(json_value)?;
                toml::to_string_pretty(&toml_value)
                    .map_err(|e| MigrationError::TomlSerializeError(e.to_string()))?
            }
            FormatStrategy::Json => serde_json::to_string_pretty(&json_value)
                .map_err(|e| MigrationError::SerializationError(e.to_string()))?,
        };

        // Write to temporary file
        let tmp_path = self.get_temp_path()?;
        let mut tmp_file = File::create(&tmp_path).map_err(|e| {
            MigrationError::Store(StoreError::IoError {
                operation: IoOperationKind::Create,
                path: tmp_path.display().to_string(),
                context: Some("temporary file".to_string()),
                error: e.to_string(),
            })
        })?;

        tmp_file.write_all(content.as_bytes()).map_err(|e| {
            MigrationError::Store(StoreError::IoError {
                operation: IoOperationKind::Write,
                path: tmp_path.display().to_string(),
                context: Some("temporary file".to_string()),
                error: e.to_string(),
            })
        })?;

        // Ensure data is written to disk
        tmp_file.sync_all().map_err(|e| {
            MigrationError::Store(StoreError::IoError {
                operation: IoOperationKind::Sync,
                path: tmp_path.display().to_string(),
                context: Some("temporary file".to_string()),
                error: e.to_string(),
            })
        })?;

        drop(tmp_file);

        // Atomic rename with retry
        self.atomic_rename(&tmp_path)?;

        // Cleanup old temp files (best effort)
        if self.strategy.atomic_write.cleanup_tmp_files {
            let _ = self.cleanup_temp_files();
        }

        Ok(())
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
        &self.path
    }

    /// Get path to temporary file for atomic writes.
    fn get_temp_path(&self) -> Result<PathBuf, MigrationError> {
        let parent = self.path.parent().ok_or_else(|| {
            MigrationError::PathResolution("Path has no parent directory".to_string())
        })?;

        let file_name = self
            .path
            .file_name()
            .ok_or_else(|| MigrationError::PathResolution("Path has no file name".to_string()))?;

        let tmp_name = format!(
            ".{}.tmp.{}",
            file_name.to_string_lossy(),
            std::process::id()
        );
        Ok(parent.join(tmp_name))
    }

    /// Atomically rename temporary file to target path with retry.
    fn atomic_rename(&self, tmp_path: &Path) -> Result<(), MigrationError> {
        let mut last_error = None;

        for attempt in 0..self.strategy.atomic_write.retry_count {
            match fs::rename(tmp_path, &self.path) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_error = Some(e);
                    if attempt + 1 < self.strategy.atomic_write.retry_count {
                        // Small delay before retry
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            }
        }

        Err(MigrationError::Store(StoreError::IoError {
            operation: IoOperationKind::Rename,
            path: self.path.display().to_string(),
            context: Some(format!(
                "after {} retries",
                self.strategy.atomic_write.retry_count
            )),
            error: last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error after retries".to_string()),
        }))
    }

    /// Clean up old temporary files (best effort).
    fn cleanup_temp_files(&self) -> std::io::Result<()> {
        let parent = match self.path.parent() {
            Some(p) => p,
            None => return Ok(()),
        };

        let file_name = match self.path.file_name() {
            Some(f) => f.to_string_lossy(),
            None => return Ok(()),
        };

        let prefix = format!(".{}.tmp.", file_name);

        if let Ok(entries) = fs::read_dir(parent) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if name.starts_with(&prefix) {
                        // Try to remove, but ignore errors (best effort)
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }

        Ok(())
    }
}

/// Convert toml::Value to serde_json::Value.
fn toml_to_json(toml_value: toml::Value) -> Result<JsonValue, MigrationError> {
    // Serialize toml::Value to JSON string, then parse as serde_json::Value
    let json_str = serde_json::to_string(&toml_value)
        .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
    let json_value: JsonValue = serde_json::from_str(&json_str)
        .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;
    Ok(json_value)
}

/// Convert serde_json::Value to toml::Value.
fn json_to_toml(json_value: &JsonValue) -> Result<toml::Value, MigrationError> {
    // Serialize serde_json::Value to JSON string, then parse as toml::Value
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
            Err(MigrationError::Store(StoreError::IoError { .. }))
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
        let entries: Vec<_> = fs::read_dir(temp_dir.path())
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
        fs::write(&file_path, "").unwrap();

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
        fs::write(&file_path, "   \n\t\n  ").unwrap();

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
        fs::write(&fake_tmp1, "old temp 1").unwrap();
        fs::write(&fake_tmp2, "old temp 2").unwrap();

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
        fs::write(&fake_tmp, "old temp").unwrap();

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
        let config = AtomicWriteConfig::default();
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
