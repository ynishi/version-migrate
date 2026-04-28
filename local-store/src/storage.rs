//! Raw file storage with ACID guarantees.
//!
//! Provides atomic file operations, format dispatch, and file locking.
//! This module is intentionally free of any migration or versioning logic.

use crate::atomic_io;
use crate::errors::{IoOperationKind, StoreError};
use crate::format_convert::json_to_toml;
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

/// Behavior when loading a file that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadBehavior {
    /// Proceed normally with empty content if file is missing.
    CreateIfMissing,
    /// Serialize `default_value` and write to disk if file is missing.
    SaveIfMissing,
    /// Return an error if file is missing.
    ErrorIfMissing,
}

/// Strategy for file storage operations.
#[derive(Debug, Clone)]
pub struct FileStorageStrategy {
    /// File format to use.
    pub format: FormatStrategy,
    /// Atomic write configuration.
    pub atomic_write: AtomicWriteConfig,
    /// Behavior when file does not exist.
    pub load_behavior: LoadBehavior,
    /// Default value used when `SaveIfMissing` is set (as JSON Value).
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

    /// Set whether to clean up temporary files.
    pub fn with_cleanup(mut self, cleanup: bool) -> Self {
        self.atomic_write.cleanup_tmp_files = cleanup;
        self
    }

    /// Set the load behavior.
    pub fn with_load_behavior(mut self, behavior: LoadBehavior) -> Self {
        self.load_behavior = behavior;
        self
    }

    /// Set the default value used when `SaveIfMissing` is set.
    pub fn with_default_value(mut self, value: JsonValue) -> Self {
        self.default_value = Some(value);
        self
    }
}

/// Raw file storage with ACID guarantees.
///
/// Holds only `path` and `strategy`; no migration or versioning state.
/// Higher-level wrappers (e.g. `VersionedFileStorage`) own the migration logic
/// and delegate raw IO to this struct.
pub struct FileStorage {
    path: PathBuf,
    strategy: FileStorageStrategy,
}

impl FileStorage {
    /// Create a new `FileStorage` and handle missing-file behavior.
    ///
    /// - `CreateIfMissing`: succeeds without writing when file is absent.
    /// - `SaveIfMissing`: serializes `strategy.default_value` (or `{}`) and writes it.
    /// - `ErrorIfMissing`: returns `StoreError` when file is absent.
    pub fn new(path: PathBuf, strategy: FileStorageStrategy) -> Result<Self, StoreError> {
        let file_was_missing = !path.exists();

        if file_was_missing {
            match strategy.load_behavior {
                LoadBehavior::ErrorIfMissing => {
                    return Err(StoreError::IoError {
                        operation: IoOperationKind::Read,
                        path: path.display().to_string(),
                        context: None,
                        error: "File not found".to_string(),
                    });
                }
                LoadBehavior::CreateIfMissing => {
                    // Nothing to do; caller reads via read_string() on demand.
                }
                LoadBehavior::SaveIfMissing => {
                    // Serialize default_value (or "{}") and persist immediately.
                    let storage = Self { path, strategy };
                    let content = storage.default_value_as_string()?;
                    storage.write_string(&content)?;
                    return Ok(storage);
                }
            }
        }

        Ok(Self { path, strategy })
    }

    /// Read raw file contents as a string.
    ///
    /// Returns the content exactly as stored on disk.
    pub fn read_string(&self) -> Result<String, StoreError> {
        fs::read_to_string(&self.path).map_err(|e| StoreError::IoError {
            operation: IoOperationKind::Read,
            path: self.path.display().to_string(),
            context: None,
            error: e.to_string(),
        })
    }

    /// Write `content` to the file atomically.
    ///
    /// Creates parent directories as needed, writes to a temp file, syncs,
    /// then renames atomically (with retry according to `strategy.atomic_write`).
    pub fn write_string(&self, content: &str) -> Result<(), StoreError> {
        // Ensure parent directory exists.
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| StoreError::IoError {
                    operation: IoOperationKind::CreateDir,
                    path: parent.display().to_string(),
                    context: Some("parent directory".to_string()),
                    error: e.to_string(),
                })?;
            }
        }

        let tmp_path = atomic_io::get_temp_path(&self.path)?;

        let mut tmp_file = File::create(&tmp_path).map_err(|e| StoreError::IoError {
            operation: IoOperationKind::Create,
            path: tmp_path.display().to_string(),
            context: Some("temporary file".to_string()),
            error: e.to_string(),
        })?;

        tmp_file
            .write_all(content.as_bytes())
            .map_err(|e| StoreError::IoError {
                operation: IoOperationKind::Write,
                path: tmp_path.display().to_string(),
                context: Some("temporary file".to_string()),
                error: e.to_string(),
            })?;

        tmp_file.sync_all().map_err(|e| StoreError::IoError {
            operation: IoOperationKind::Sync,
            path: tmp_path.display().to_string(),
            context: Some("temporary file".to_string()),
            error: e.to_string(),
        })?;

        drop(tmp_file);

        atomic_io::atomic_rename(
            &tmp_path,
            &self.path,
            self.strategy.atomic_write.retry_count,
        )?;

        if self.strategy.atomic_write.cleanup_tmp_files {
            let _ = atomic_io::cleanup_temp_files(&self.path);
        }

        Ok(())
    }

    /// Returns a reference to the storage file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a reference to the storage strategy.
    pub fn strategy(&self) -> &FileStorageStrategy {
        &self.strategy
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Serialize `strategy.default_value` (or `"{}"`) into the on-disk format.
    fn default_value_as_string(&self) -> Result<String, StoreError> {
        let json_value = self
            .strategy
            .default_value
            .clone()
            .unwrap_or(JsonValue::Object(Default::default()));

        match self.strategy.format {
            FormatStrategy::Json => {
                serde_json::to_string_pretty(&json_value).map_err(|e| StoreError::IoError {
                    operation: IoOperationKind::Write,
                    path: self.path.display().to_string(),
                    context: Some("serialize default value".to_string()),
                    error: e.to_string(),
                })
            }
            FormatStrategy::Toml => {
                let toml_value = json_to_toml(&json_value)?;
                toml::to_string_pretty(&toml_value).map_err(|e| StoreError::IoError {
                    operation: IoOperationKind::Write,
                    path: self.path.display().to_string(),
                    context: Some("serialize default value as toml".to_string()),
                    error: e.to_string(),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // R-S1-1: new() + SaveIfMissing writes default_value
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_creates_file_with_save_if_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        let strategy = FileStorageStrategy::new()
            .with_load_behavior(LoadBehavior::SaveIfMissing)
            .with_default_value(serde_json::json!({"key": "value"}));

        assert!(!path.exists());
        let storage = FileStorage::new(path.clone(), strategy).unwrap();
        assert!(path.exists(), "file must be created for SaveIfMissing");
        assert_eq!(storage.path(), path.as_path());
    }

    #[test]
    fn test_new_no_file_create_if_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.toml");

        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::CreateIfMissing);

        let storage = FileStorage::new(path.clone(), strategy).unwrap();
        // File is NOT written for CreateIfMissing.
        assert!(!path.exists());
        assert_eq!(storage.path(), path.as_path());
    }

    #[test]
    fn test_new_error_if_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("absent.toml");

        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::ErrorIfMissing);

        let result = FileStorage::new(path, strategy);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(StoreError::IoError {
                operation: IoOperationKind::Read,
                ..
            })
        ));
    }

    // -----------------------------------------------------------------------
    // read_string / write_string (core API)
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_string_returns_file_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.json");
        fs::write(&path, r#"{"hello":"world"}"#).unwrap();

        let strategy = FileStorageStrategy::new()
            .with_format(FormatStrategy::Json)
            .with_load_behavior(LoadBehavior::ErrorIfMissing);

        let storage = FileStorage::new(path, strategy).unwrap();
        let content = storage.read_string().unwrap();
        assert_eq!(content, r#"{"hello":"world"}"#);
    }

    #[test]
    fn test_write_string_creates_and_reads_back() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("out.json");

        let strategy = FileStorageStrategy::new()
            .with_format(FormatStrategy::Json)
            .with_load_behavior(LoadBehavior::CreateIfMissing);

        let storage = FileStorage::new(path.clone(), strategy).unwrap();
        storage.write_string(r#"{"x":1}"#).unwrap();

        let back = storage.read_string().unwrap();
        assert_eq!(back, r#"{"x":1}"#);
    }

    #[test]
    fn test_write_string_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a").join("b").join("c.toml");

        let strategy = FileStorageStrategy::new().with_load_behavior(LoadBehavior::CreateIfMissing);

        let storage = FileStorage::new(path.clone(), strategy).unwrap();
        storage.write_string("").unwrap();
        assert!(path.exists());
    }

    // -----------------------------------------------------------------------
    // R-S1-2: atomic_rename retry_count behaviour
    // -----------------------------------------------------------------------

    #[test]
    fn test_atomic_write_no_tmp_left() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("atomic.toml");
        let strategy = FileStorageStrategy::default();

        let storage = FileStorage::new(path.clone(), strategy).unwrap();
        storage.write_string("hello = true\n").unwrap();

        let tmp_files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".atomic.toml.tmp")
            })
            .collect();
        assert_eq!(tmp_files.len(), 0, "no temp files should remain");
    }

    // -----------------------------------------------------------------------
    // R-S1-3: cleanup_temp_files removes stale .tmp files
    // -----------------------------------------------------------------------

    #[test]
    fn test_cleanup_removes_stale_tmp_files() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cfg.toml");

        let fake1 = dir.path().join(".cfg.toml.tmp.11111");
        let fake2 = dir.path().join(".cfg.toml.tmp.22222");
        fs::write(&fake1, "stale1").unwrap();
        fs::write(&fake2, "stale2").unwrap();

        let strategy = FileStorageStrategy::default();
        let storage = FileStorage::new(path.clone(), strategy).unwrap();
        storage.write_string("cfg = true\n").unwrap();

        assert!(!fake1.exists(), "stale tmp 1 should be removed");
        assert!(!fake2.exists(), "stale tmp 2 should be removed");
    }

    #[test]
    fn test_no_cleanup_keeps_stale_tmp_files() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("no_clean.toml");
        let fake = dir.path().join(".no_clean.toml.tmp.99999");
        fs::write(&fake, "stale").unwrap();

        let strategy = FileStorageStrategy::new().with_cleanup(false);
        let storage = FileStorage::new(path.clone(), strategy).unwrap();
        storage.write_string("x = 1\n").unwrap();

        assert!(fake.exists(), "stale tmp must remain when cleanup=false");
    }

    // -----------------------------------------------------------------------
    // R-S1-4: FormatStrategy::Toml / Json dispatch via default_value_as_string
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_if_missing_json_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.json");

        let strategy = FileStorageStrategy::new()
            .with_format(FormatStrategy::Json)
            .with_load_behavior(LoadBehavior::SaveIfMissing)
            .with_default_value(serde_json::json!({"items": []}));

        FileStorage::new(path.clone(), strategy).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        // Must parse as valid JSON.
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed.get("items").is_some());
    }

    #[test]
    fn test_save_if_missing_toml_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.toml");

        let strategy = FileStorageStrategy::new()
            .with_format(FormatStrategy::Toml)
            .with_load_behavior(LoadBehavior::SaveIfMissing)
            .with_default_value(serde_json::json!({"name": "alice"}));

        FileStorage::new(path.clone(), strategy).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        // Must parse as valid TOML.
        let parsed: toml::Value = toml::from_str(&content).unwrap();
        assert!(parsed.get("name").is_some());
    }

    // -----------------------------------------------------------------------
    // Strategy builder
    // -----------------------------------------------------------------------

    #[test]
    fn test_strategy_builder() {
        let s = FileStorageStrategy::new()
            .with_format(FormatStrategy::Json)
            .with_retry_count(5)
            .with_cleanup(false)
            .with_load_behavior(LoadBehavior::ErrorIfMissing);

        assert_eq!(s.format, FormatStrategy::Json);
        assert_eq!(s.atomic_write.retry_count, 5);
        assert!(!s.atomic_write.cleanup_tmp_files);
        assert_eq!(s.load_behavior, LoadBehavior::ErrorIfMissing);
    }

    #[test]
    fn test_atomic_write_config_default() {
        let cfg = AtomicWriteConfig::default();
        assert_eq!(cfg.retry_count, 3);
        assert!(cfg.cleanup_tmp_files);
    }

    #[test]
    fn test_format_strategy_equality() {
        assert_eq!(FormatStrategy::Toml, FormatStrategy::Toml);
        assert_ne!(FormatStrategy::Toml, FormatStrategy::Json);
    }

    #[test]
    fn test_load_behavior_equality() {
        assert_eq!(LoadBehavior::CreateIfMissing, LoadBehavior::CreateIfMissing);
        assert_ne!(LoadBehavior::CreateIfMissing, LoadBehavior::ErrorIfMissing);
    }
}
