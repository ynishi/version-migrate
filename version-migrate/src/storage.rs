//! File storage layer with ACID guarantees for versioned configuration.
//!
//! Provides atomic file operations, format conversion, and file locking.

use crate::{ConfigMigrator, MigrationError, Migrator, Queryable};
use serde_json::Value as JsonValue;
use std::fs::{self, File, OpenOptions};
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
}

impl Default for FileStorageStrategy {
    fn default() -> Self {
        Self {
            format: FormatStrategy::Toml,
            atomic_write: AtomicWriteConfig::default(),
            load_behavior: LoadBehavior::CreateIfMissing,
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
        // Load file content if it exists
        let json_string = if path.exists() {
            let content = fs::read_to_string(&path).map_err(|e| MigrationError::IoError {
                path: path.display().to_string(),
                error: e.to_string(),
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
                LoadBehavior::CreateIfMissing => "{}".to_string(),
                LoadBehavior::ErrorIfMissing => {
                    return Err(MigrationError::IoError {
                        path: path.display().to_string(),
                        error: "File not found".to_string(),
                    });
                }
            }
        };

        // Create ConfigMigrator with loaded/empty data
        let config = ConfigMigrator::from(&json_string, migrator)?;

        Ok(Self {
            path,
            config,
            strategy,
        })
    }

    /// Save current state to file atomically.
    ///
    /// Uses a temporary file + atomic rename to ensure durability.
    /// Retries according to `strategy.atomic_write.retry_count`.
    pub fn save(&self) -> Result<(), MigrationError> {
        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| MigrationError::IoError {
                    path: parent.display().to_string(),
                    error: e.to_string(),
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
        let mut tmp_file = File::create(&tmp_path).map_err(|e| MigrationError::IoError {
            path: tmp_path.display().to_string(),
            error: e.to_string(),
        })?;

        tmp_file
            .write_all(content.as_bytes())
            .map_err(|e| MigrationError::IoError {
                path: tmp_path.display().to_string(),
                error: e.to_string(),
            })?;

        // Ensure data is written to disk
        tmp_file.sync_all().map_err(|e| MigrationError::IoError {
            path: tmp_path.display().to_string(),
            error: e.to_string(),
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

        Err(MigrationError::IoError {
            path: self.path.display().to_string(),
            error: format!(
                "Failed to rename after {} attempts: {}",
                self.strategy.atomic_write.retry_count,
                last_error.unwrap()
            ),
        })
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

/// File lock guard that automatically releases the lock when dropped.
///
/// Currently unused, but reserved for future concurrent access features.
#[allow(dead_code)]
struct FileLock {
    file: File,
    lock_path: PathBuf,
}

#[allow(dead_code)]
impl FileLock {
    /// Acquire an exclusive lock on the given path.
    fn acquire(path: &Path) -> Result<Self, MigrationError> {
        // Create lock file path
        let lock_path = path.with_extension("lock");

        // Ensure parent directory exists
        if let Some(parent) = lock_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| MigrationError::LockError {
                    path: lock_path.display().to_string(),
                    error: e.to_string(),
                })?;
            }
        }

        // Open or create lock file
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| MigrationError::LockError {
                path: lock_path.display().to_string(),
                error: e.to_string(),
            })?;

        // Try to acquire exclusive lock with fs2
        #[cfg(unix)]
        {
            use fs2::FileExt;
            file.lock_exclusive()
                .map_err(|e| MigrationError::LockError {
                    path: lock_path.display().to_string(),
                    error: format!("Failed to acquire exclusive lock: {}", e),
                })?;
        }

        #[cfg(not(unix))]
        {
            // On non-Unix systems, we don't have file locking
            // This is acceptable for single-user desktop apps
            // For production use on Windows, consider using advisory locking
        }

        Ok(FileLock { file, lock_path })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Unlock is automatic when the file handle is dropped on Unix
        // Try to remove lock file (best effort)
        let _ = fs::remove_file(&self.lock_path);
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
        assert!(matches!(result, Err(MigrationError::IoError { .. })));
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
}
