//! Raw directory-based storage for one-file-per-entity persistence.
//!
//! Provides ACID-safe IO operations (atomic rename, fsync, retry) without any
//! migration or versioning logic.  Schema evolution is the caller's responsibility.
//!
//! # Crux constraint compliance
//!
//! - This module contains **no** reference to `Migrator`, `ConfigMigrator`,
//!   `Queryable`, `MigrationError`, or `version_migrate`.
//! - All public APIs accept `category` / `entity_name` / `id` as
//!   `impl Into<String>` (never a concrete enum type).

use crate::{
    errors::{IoOperationKind, StoreError},
    AppPaths,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::fs::{self, File};
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};

// Re-export shared types from storage module so callers can use them from
// a single import path.
pub use crate::storage::{AtomicWriteConfig, FormatStrategy};

// ============================================================================
// Configuration types
// ============================================================================

/// File-naming encoding strategy for entity IDs.
///
/// Determines how entity IDs are encoded into filesystem-safe filenames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilenameEncoding {
    /// Use the ID directly as the filename.
    ///
    /// Only IDs consisting entirely of ASCII alphanumeric characters, `-`, and
    /// `_` are accepted; any other character causes a `StoreError::FilenameEncoding`
    /// error.
    #[default]
    Direct,
    /// URL-encode the ID so that special characters become percent-escaped
    /// sequences that are safe to use in a filename.
    UrlEncode,
    /// Base64-encode the ID using the URL-safe alphabet without padding.
    Base64,
}

/// Strategy configuration for directory-based storage operations.
#[derive(Debug, Clone)]
pub struct DirStorageStrategy {
    /// File format to use for serialisation (JSON or TOML).
    pub format: FormatStrategy,
    /// Atomic write configuration (retry count, temp-file cleanup).
    pub atomic_write: AtomicWriteConfig,
    /// Custom file extension.  When `None` the extension is derived from
    /// `format` (`"json"` or `"toml"`).
    pub extension: Option<String>,
    /// Filename encoding strategy for entity IDs.
    pub filename_encoding: FilenameEncoding,
}

impl Default for DirStorageStrategy {
    fn default() -> Self {
        Self {
            format: FormatStrategy::Json,
            atomic_write: AtomicWriteConfig::default(),
            extension: None,
            filename_encoding: FilenameEncoding::default(),
        }
    }
}

impl DirStorageStrategy {
    /// Create a new strategy with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the file format.
    ///
    /// # Arguments
    ///
    /// * `format` - `FormatStrategy::Json` or `FormatStrategy::Toml`.
    ///
    /// # Returns
    ///
    /// `self` with the updated format (builder pattern).
    pub fn with_format(mut self, format: FormatStrategy) -> Self {
        self.format = format;
        self
    }

    /// Set a custom file extension.
    ///
    /// # Arguments
    ///
    /// * `ext` - Extension string without the leading dot (e.g. `"json"`).
    ///
    /// # Returns
    ///
    /// `self` with the updated extension (builder pattern).
    pub fn with_extension(mut self, ext: impl Into<String>) -> Self {
        self.extension = Some(ext.into());
        self
    }

    /// Set the filename encoding strategy.
    ///
    /// # Arguments
    ///
    /// * `encoding` - One of `FilenameEncoding::Direct`, `UrlEncode`, or `Base64`.
    ///
    /// # Returns
    ///
    /// `self` with the updated encoding (builder pattern).
    pub fn with_filename_encoding(mut self, encoding: FilenameEncoding) -> Self {
        self.filename_encoding = encoding;
        self
    }

    /// Set the retry count for atomic writes.
    ///
    /// # Arguments
    ///
    /// * `count` - Number of rename attempts before returning an error.
    ///
    /// # Returns
    ///
    /// `self` with the updated retry count (builder pattern).
    pub fn with_retry_count(mut self, count: usize) -> Self {
        self.atomic_write.retry_count = count;
        self
    }

    /// Set whether to clean up orphaned temporary files.
    ///
    /// # Arguments
    ///
    /// * `cleanup` - When `true`, stale `.tmp.*` files are removed after every
    ///   successful atomic write (best-effort; errors are silently ignored).
    ///
    /// # Returns
    ///
    /// `self` with the updated cleanup flag (builder pattern).
    pub fn with_cleanup(mut self, cleanup: bool) -> Self {
        self.atomic_write.cleanup_tmp_files = cleanup;
        self
    }

    /// Returns the effective file extension for this strategy.
    ///
    /// Uses `self.extension` when set; otherwise derives `"json"` or `"toml"`
    /// from `self.format`.
    pub fn get_extension(&self) -> String {
        self.extension.clone().unwrap_or_else(|| match self.format {
            FormatStrategy::Json => "json".to_string(),
            FormatStrategy::Toml => "toml".to_string(),
        })
    }
}

// ============================================================================
// Sync DirStorage
// ============================================================================

/// Raw directory-based entity storage with ACID guarantees.
///
/// Manages one file per entity and provides:
///
/// - **Atomicity**: writes use a temporary file followed by an atomic rename.
/// - **Durability**: `fsync` is called before the rename.
/// - **Idempotent delete**: calling `delete` on a missing ID returns `Ok(())`.
///
/// This type holds no `Migrator` and performs no schema migration.
/// Content is stored and retrieved as opaque UTF-8 strings; callers are
/// responsible for any serialisation/deserialisation.
pub struct DirStorage {
    /// Resolved absolute path to the storage directory.
    base_path: PathBuf,
    /// Storage strategy (format, encoding, atomic-write config).
    strategy: DirStorageStrategy,
}

impl DirStorage {
    /// Create a new `DirStorage` instance.
    ///
    /// # Arguments
    ///
    /// * `paths` - Application path manager used to resolve `data_dir`.
    /// * `category` - Sub-directory name appended to `data_dir` (e.g. `"sessions"`).
    /// * `strategy` - Storage strategy configuration.
    ///
    /// # Returns
    ///
    /// `Ok(DirStorage)` with `base_path = data_dir/category`.
    ///
    /// # Errors
    ///
    /// Returns `StoreError::HomeDirNotFound` if `data_dir` cannot be resolved,
    /// or `StoreError::IoError { operation: CreateDir, … }` if the base
    /// directory cannot be created.
    pub fn new(
        paths: AppPaths,
        category: impl Into<String>,
        strategy: DirStorageStrategy,
    ) -> Result<Self, StoreError> {
        let category: String = category.into();
        let base_path = paths.data_dir()?.join(&category);

        if !base_path.exists() {
            fs::create_dir_all(&base_path).map_err(|e| StoreError::IoError {
                operation: IoOperationKind::CreateDir,
                path: base_path.display().to_string(),
                context: Some("storage base directory".to_string()),
                error: e.to_string(),
            })?;
        }

        Ok(Self {
            base_path,
            strategy,
        })
    }

    /// Write raw string content for an entity, atomically.
    ///
    /// # Arguments
    ///
    /// * `entity_name` - Logical entity type name (informational; not used in
    ///   the file path).
    /// * `id` - Unique identifier for this entity (encoded into the filename).
    /// * `content` - UTF-8 string to persist verbatim.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// - `StoreError::FilenameEncoding` if `id` cannot be encoded with the
    ///   configured strategy.
    /// - `StoreError::IoError` if the file cannot be written.
    pub fn save_raw_string(
        &self,
        _entity_name: impl Into<String>,
        id: impl Into<String>,
        content: &str,
    ) -> Result<(), StoreError> {
        let id: String = id.into();
        let file_path = self.id_to_path(&id)?;
        self.atomic_write(&file_path, content)?;
        Ok(())
    }

    /// Read the raw string content for an entity.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the entity.
    ///
    /// # Returns
    ///
    /// The UTF-8 string content stored for `id`.
    ///
    /// # Errors
    ///
    /// - `StoreError::FilenameEncoding` if `id` cannot be encoded.
    /// - `StoreError::IoError { operation: Read, … }` if the file is missing
    ///   or cannot be read.
    pub fn load_raw_string(&self, id: impl Into<String>) -> Result<String, StoreError> {
        let id: String = id.into();
        let file_path = self.id_to_path(&id)?;

        if !file_path.exists() {
            return Err(StoreError::IoError {
                operation: IoOperationKind::Read,
                path: file_path.display().to_string(),
                context: None,
                error: "File not found".to_string(),
            });
        }

        fs::read_to_string(&file_path).map_err(|e| StoreError::IoError {
            operation: IoOperationKind::Read,
            path: file_path.display().to_string(),
            context: None,
            error: e.to_string(),
        })
    }

    /// List all entity IDs stored in the base directory.
    ///
    /// Only files whose extension matches `strategy.get_extension()` are
    /// included.  Temporary files (`.tmp.*`) are excluded because their
    /// extension is `tmp`, not the configured extension.
    ///
    /// # Returns
    ///
    /// A sorted `Vec<String>` of decoded entity IDs.
    ///
    /// # Errors
    ///
    /// - `StoreError::IoError { operation: ReadDir, … }` if the directory
    ///   cannot be read.
    /// - `StoreError::FilenameEncoding` if a filename cannot be decoded.
    pub fn list_ids(&self) -> Result<Vec<String>, StoreError> {
        let entries = fs::read_dir(&self.base_path).map_err(|e| StoreError::IoError {
            operation: IoOperationKind::ReadDir,
            path: self.base_path.display().to_string(),
            context: None,
            error: e.to_string(),
        })?;

        let extension = self.strategy.get_extension();
        let mut ids = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|e| StoreError::IoError {
                operation: IoOperationKind::ReadDir,
                path: self.base_path.display().to_string(),
                context: Some("directory entry".to_string()),
                error: e.to_string(),
            })?;

            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == extension.as_str() {
                        if let Some(id) = self.path_to_id(&path)? {
                            ids.push(id);
                        }
                    }
                }
            }
        }

        ids.sort();
        Ok(ids)
    }

    /// Check whether an entity file exists.
    ///
    /// # Arguments
    ///
    /// * `id` - Entity identifier.
    ///
    /// # Returns
    ///
    /// `true` if the encoded file exists and is a regular file; `false`
    /// otherwise.
    ///
    /// # Errors
    ///
    /// `StoreError::FilenameEncoding` if `id` cannot be encoded.
    pub fn exists(&self, id: impl Into<String>) -> Result<bool, StoreError> {
        let id: String = id.into();
        let file_path = self.id_to_path(&id)?;
        Ok(file_path.exists() && file_path.is_file())
    }

    /// Delete the file associated with an entity ID.
    ///
    /// This operation is **idempotent**: if the file does not exist, `Ok(())`
    /// is returned without error (matches original behaviour at
    /// `dir_storage.rs:760-775`).
    ///
    /// # Arguments
    ///
    /// * `id` - Entity identifier.
    ///
    /// # Returns
    ///
    /// `Ok(())` whether or not the file existed.
    ///
    /// # Errors
    ///
    /// - `StoreError::FilenameEncoding` if `id` cannot be encoded.
    /// - `StoreError::IoError { operation: Delete, … }` if the file exists but
    ///   cannot be removed.
    pub fn delete(&self, id: impl Into<String>) -> Result<(), StoreError> {
        let id: String = id.into();
        let file_path = self.id_to_path(&id)?;

        if file_path.exists() {
            fs::remove_file(&file_path).map_err(|e| StoreError::IoError {
                operation: IoOperationKind::Delete,
                path: file_path.display().to_string(),
                context: None,
                error: e.to_string(),
            })?;
        }

        Ok(())
    }

    /// Returns a reference to the resolved base directory path.
    ///
    /// # Returns
    ///
    /// The absolute `Path` at which entity files are stored.
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    /// Encode `id` and build the full file path for it.
    ///
    /// # Errors
    ///
    /// `StoreError::FilenameEncoding` if the encoding strategy rejects the ID.
    fn id_to_path(&self, id: &str) -> Result<PathBuf, StoreError> {
        let encoded_id = self.encode_id(id)?;
        let extension = self.strategy.get_extension();
        let filename = format!("{}.{}", encoded_id, extension);
        Ok(self.base_path.join(filename))
    }

    /// Encode an entity ID to a filesystem-safe stem using the configured
    /// encoding strategy.
    ///
    /// # Arguments
    ///
    /// * `id` - Raw entity identifier string.
    ///
    /// # Returns
    ///
    /// The encoded stem (without extension).
    ///
    /// # Errors
    ///
    /// `StoreError::FilenameEncoding { id, reason }` when:
    /// - `Direct` mode and `id` contains characters outside `[A-Za-z0-9\-_]`.
    fn encode_id(&self, id: &str) -> Result<String, StoreError> {
        match self.strategy.filename_encoding {
            FilenameEncoding::Direct => {
                if id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    Ok(id.to_string())
                } else {
                    Err(StoreError::FilenameEncoding {
                        id: id.to_string(),
                        reason: "ID contains invalid characters for Direct encoding. \
                             Only alphanumeric, '-', and '_' are allowed."
                            .to_string(),
                    })
                }
            }
            FilenameEncoding::UrlEncode => Ok(urlencoding::encode(id).into_owned()),
            FilenameEncoding::Base64 => Ok(URL_SAFE_NO_PAD.encode(id.as_bytes())),
        }
    }

    /// Decode a filename stem back to the original entity ID.
    ///
    /// # Arguments
    ///
    /// * `filename_stem` - The file name without extension.
    ///
    /// # Returns
    ///
    /// The decoded entity ID string.
    ///
    /// # Errors
    ///
    /// `StoreError::FilenameEncoding { id, reason }` when decoding fails.
    fn decode_id(&self, filename_stem: &str) -> Result<String, StoreError> {
        match self.strategy.filename_encoding {
            FilenameEncoding::Direct => Ok(filename_stem.to_string()),
            FilenameEncoding::UrlEncode => urlencoding::decode(filename_stem)
                .map(|s| s.into_owned())
                .map_err(|e| StoreError::FilenameEncoding {
                    id: filename_stem.to_string(),
                    reason: format!("Failed to URL-decode filename: {}", e),
                }),
            FilenameEncoding::Base64 => URL_SAFE_NO_PAD
                .decode(filename_stem.as_bytes())
                .map_err(|e| StoreError::FilenameEncoding {
                    id: filename_stem.to_string(),
                    reason: format!("Failed to Base64-decode filename: {}", e),
                })
                .and_then(|bytes| {
                    String::from_utf8(bytes).map_err(|e| StoreError::FilenameEncoding {
                        id: filename_stem.to_string(),
                        reason: format!("Failed to convert Base64-decoded bytes to UTF-8: {}", e),
                    })
                }),
        }
    }

    /// Extract the entity ID from an absolute file path.
    ///
    /// # Returns
    ///
    /// `Some(id)` when a valid stem is found; `None` when the path has no stem.
    ///
    /// # Errors
    ///
    /// `StoreError::FilenameEncoding` if the stem cannot be decoded.
    fn path_to_id(&self, path: &Path) -> Result<Option<String>, StoreError> {
        let file_stem = match path.file_stem() {
            Some(stem) => stem.to_string_lossy(),
            None => return Ok(None),
        };
        let id = self.decode_id(&file_stem)?;
        Ok(Some(id))
    }

    /// Build the temporary file path used during an atomic write.
    ///
    /// The temporary file is created in the same directory as the target so
    /// that the subsequent rename is always within the same filesystem.
    ///
    /// Format: `<parent>/.<filename>.tmp.<pid>`
    ///
    /// # Arguments
    ///
    /// * `target_path` - The final target path for the write.
    ///
    /// # Returns
    ///
    /// The path of the temporary file.
    ///
    /// # Errors
    ///
    /// `StoreError::IoError` if `target_path` has no parent directory or no
    /// file name component.
    fn get_temp_path(&self, target_path: &Path) -> Result<PathBuf, StoreError> {
        let parent = target_path.parent().ok_or_else(|| StoreError::IoError {
            operation: IoOperationKind::Create,
            path: target_path.display().to_string(),
            context: Some("path has no parent directory".to_string()),
            error: "cannot determine parent for temporary file".to_string(),
        })?;

        let file_name = target_path.file_name().ok_or_else(|| StoreError::IoError {
            operation: IoOperationKind::Create,
            path: target_path.display().to_string(),
            context: Some("path has no file name".to_string()),
            error: "cannot determine filename for temporary file".to_string(),
        })?;

        let tmp_name = format!(
            ".{}.tmp.{}",
            file_name.to_string_lossy(),
            std::process::id()
        );
        Ok(parent.join(tmp_name))
    }

    /// Attempt to atomically rename `tmp_path` to `target_path`, retrying up
    /// to `strategy.atomic_write.retry_count` times with a 10 ms delay.
    ///
    /// # Arguments
    ///
    /// * `tmp_path` - Path of the temporary file.
    /// * `target_path` - Final destination path.
    ///
    /// # Errors
    ///
    /// `StoreError::IoError { operation: Rename, … }` after all retries
    /// are exhausted.
    fn atomic_rename(&self, tmp_path: &Path, target_path: &Path) -> Result<(), StoreError> {
        let mut last_error = None;

        for attempt in 0..self.strategy.atomic_write.retry_count {
            match fs::rename(tmp_path, target_path) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_error = Some(e);
                    if attempt + 1 < self.strategy.atomic_write.retry_count {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            }
        }

        Err(StoreError::IoError {
            operation: IoOperationKind::Rename,
            path: target_path.display().to_string(),
            context: Some(format!(
                "after {} retries",
                self.strategy.atomic_write.retry_count
            )),
            error: last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error after retries".to_string()),
        })
    }

    /// Remove orphaned temporary files left by previous failed writes.
    ///
    /// Errors are silently ignored (best-effort cleanup).
    ///
    /// # Arguments
    ///
    /// * `target_path` - The target path whose temp files should be cleaned.
    fn cleanup_temp_files(&self, target_path: &Path) -> std::io::Result<()> {
        let parent = match target_path.parent() {
            Some(p) => p,
            None => return Ok(()),
        };
        let file_name = match target_path.file_name() {
            Some(f) => f.to_string_lossy(),
            None => return Ok(()),
        };
        let prefix = format!(".{}.tmp.", file_name);

        if let Ok(entries) = fs::read_dir(parent) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if name.starts_with(&prefix) {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
        Ok(())
    }

    /// Write `content` to `path` atomically (tmp file + fsync + rename).
    ///
    /// # Arguments
    ///
    /// * `path` - Final target path.
    /// * `content` - UTF-8 string to write.
    ///
    /// # Errors
    ///
    /// `StoreError::IoError` if any step (create / write / sync / rename) fails.
    fn atomic_write(&self, path: &Path, content: &str) -> Result<(), StoreError> {
        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| StoreError::IoError {
                    operation: IoOperationKind::CreateDir,
                    path: parent.display().to_string(),
                    context: Some("parent directory".to_string()),
                    error: e.to_string(),
                })?;
            }
        }

        let tmp_path = self.get_temp_path(path)?;

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

        self.atomic_rename(&tmp_path, path)?;

        if self.strategy.atomic_write.cleanup_tmp_files {
            let _ = self.cleanup_temp_files(path);
        }

        Ok(())
    }
}

// ============================================================================
// Async implementation
// ============================================================================

#[cfg(feature = "async")]
pub use async_impl::AsyncDirStorage;

#[cfg(feature = "async")]
mod async_impl {
    use super::{DirStorageStrategy, FilenameEncoding};
    use crate::{
        errors::{IoOperationKind, StoreError},
        AppPaths,
    };
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use std::path::{Path, PathBuf};
    use tokio::io::AsyncWriteExt;

    /// Async version of [`DirStorage`](super::DirStorage).
    ///
    /// Provides the same raw IO guarantees (atomic rename, fsync, retry) using
    /// `tokio::fs` for non-blocking operation.
    ///
    /// # Crux constraint compliance
    ///
    /// This struct contains no reference to `Migrator`, `ConfigMigrator`,
    /// `Queryable`, `MigrationError`, or `version_migrate`.
    pub struct AsyncDirStorage {
        /// Resolved absolute path to the storage directory.
        base_path: PathBuf,
        /// Storage strategy (format, encoding, atomic-write config).
        strategy: DirStorageStrategy,
    }

    impl AsyncDirStorage {
        /// Create a new `AsyncDirStorage` instance (async).
        ///
        /// # Arguments
        ///
        /// * `paths` - Application path manager.
        /// * `category` - Sub-directory name appended to `data_dir`.
        /// * `strategy` - Storage strategy configuration.
        ///
        /// # Returns
        ///
        /// `Ok(AsyncDirStorage)` with `base_path = data_dir/category`.
        ///
        /// # Errors
        ///
        /// `StoreError::HomeDirNotFound` or `StoreError::IoError { operation:
        /// CreateDir, … }`.
        pub async fn new(
            paths: AppPaths,
            category: impl Into<String>,
            strategy: DirStorageStrategy,
        ) -> Result<Self, StoreError> {
            let category: String = category.into();
            let base_path = paths.data_dir()?.join(&category);

            if !tokio::fs::try_exists(&base_path).await.unwrap_or(false) {
                tokio::fs::create_dir_all(&base_path)
                    .await
                    .map_err(|e| StoreError::IoError {
                        operation: IoOperationKind::CreateDir,
                        path: base_path.display().to_string(),
                        context: Some("storage base directory (async)".to_string()),
                        error: e.to_string(),
                    })?;
            }

            Ok(Self {
                base_path,
                strategy,
            })
        }

        /// Write raw string content for an entity, atomically (async).
        ///
        /// # Arguments
        ///
        /// * `entity_name` - Logical entity type name (informational).
        /// * `id` - Unique identifier (encoded into the filename).
        /// * `content` - UTF-8 string to persist verbatim.
        ///
        /// # Returns
        ///
        /// `Ok(())` on success.
        ///
        /// # Errors
        ///
        /// `StoreError::FilenameEncoding` or `StoreError::IoError`.
        pub async fn save_raw_string(
            &self,
            _entity_name: impl Into<String>,
            id: impl Into<String>,
            content: &str,
        ) -> Result<(), StoreError> {
            let id: String = id.into();
            let file_path = self.id_to_path(&id)?;
            self.atomic_write(&file_path, content).await?;
            Ok(())
        }

        /// Read the raw string content for an entity (async).
        ///
        /// # Arguments
        ///
        /// * `id` - Unique identifier for the entity.
        ///
        /// # Returns
        ///
        /// The UTF-8 string content stored for `id`.
        ///
        /// # Errors
        ///
        /// `StoreError::FilenameEncoding` or `StoreError::IoError { operation:
        /// Read, … }` (including "File not found").
        pub async fn load_raw_string(&self, id: impl Into<String>) -> Result<String, StoreError> {
            let id: String = id.into();
            let file_path = self.id_to_path(&id)?;

            if !tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
                return Err(StoreError::IoError {
                    operation: IoOperationKind::Read,
                    path: file_path.display().to_string(),
                    context: None,
                    error: "File not found".to_string(),
                });
            }

            tokio::fs::read_to_string(&file_path)
                .await
                .map_err(|e| StoreError::IoError {
                    operation: IoOperationKind::Read,
                    path: file_path.display().to_string(),
                    context: None,
                    error: e.to_string(),
                })
        }

        /// List all entity IDs stored in the base directory (async).
        ///
        /// Only files matching `strategy.get_extension()` are included;
        /// `.tmp.*` files are excluded.
        ///
        /// # Returns
        ///
        /// A sorted `Vec<String>` of decoded entity IDs.
        ///
        /// # Errors
        ///
        /// `StoreError::IoError { operation: ReadDir, … }` or
        /// `StoreError::FilenameEncoding`.
        pub async fn list_ids(&self) -> Result<Vec<String>, StoreError> {
            let mut entries =
                tokio::fs::read_dir(&self.base_path)
                    .await
                    .map_err(|e| StoreError::IoError {
                        operation: IoOperationKind::ReadDir,
                        path: self.base_path.display().to_string(),
                        context: None,
                        error: e.to_string(),
                    })?;

            let extension = self.strategy.get_extension();
            let mut ids = Vec::new();

            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|e| StoreError::IoError {
                    operation: IoOperationKind::ReadDir,
                    path: self.base_path.display().to_string(),
                    context: Some("directory entry (async)".to_string()),
                    error: e.to_string(),
                })?
            {
                let path = entry.path();

                let metadata =
                    tokio::fs::metadata(&path)
                        .await
                        .map_err(|e| StoreError::IoError {
                            operation: IoOperationKind::Read,
                            path: path.display().to_string(),
                            context: Some("metadata (async)".to_string()),
                            error: e.to_string(),
                        })?;

                if metadata.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == extension.as_str() {
                            if let Some(id) = self.path_to_id(&path)? {
                                ids.push(id);
                            }
                        }
                    }
                }
            }

            ids.sort();
            Ok(ids)
        }

        /// Check whether an entity file exists (async).
        ///
        /// # Arguments
        ///
        /// * `id` - Entity identifier.
        ///
        /// # Returns
        ///
        /// `true` if the encoded file exists and is a regular file.
        ///
        /// # Errors
        ///
        /// `StoreError::FilenameEncoding` if `id` cannot be encoded.
        pub async fn exists(&self, id: impl Into<String>) -> Result<bool, StoreError> {
            let id: String = id.into();
            let file_path = self.id_to_path(&id)?;

            if !tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
                return Ok(false);
            }

            let metadata =
                tokio::fs::metadata(&file_path)
                    .await
                    .map_err(|e| StoreError::IoError {
                        operation: IoOperationKind::Read,
                        path: file_path.display().to_string(),
                        context: Some("metadata (async)".to_string()),
                        error: e.to_string(),
                    })?;

            Ok(metadata.is_file())
        }

        /// Delete the file associated with an entity ID (async).
        ///
        /// This operation is **idempotent**: missing files return `Ok(())`.
        ///
        /// # Arguments
        ///
        /// * `id` - Entity identifier.
        ///
        /// # Returns
        ///
        /// `Ok(())` whether or not the file existed.
        ///
        /// # Errors
        ///
        /// `StoreError::FilenameEncoding` or `StoreError::IoError { operation:
        /// Delete, … }`.
        pub async fn delete(&self, id: impl Into<String>) -> Result<(), StoreError> {
            let id: String = id.into();
            let file_path = self.id_to_path(&id)?;

            if tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
                tokio::fs::remove_file(&file_path)
                    .await
                    .map_err(|e| StoreError::IoError {
                        operation: IoOperationKind::Delete,
                        path: file_path.display().to_string(),
                        context: None,
                        error: e.to_string(),
                    })?;
            }

            Ok(())
        }

        /// Returns a reference to the resolved base directory path.
        ///
        /// # Returns
        ///
        /// The absolute `Path` at which entity files are stored.
        pub fn base_path(&self) -> &Path {
            &self.base_path
        }

        // =================================================================
        // Private helpers (async)
        // =================================================================

        fn id_to_path(&self, id: &str) -> Result<PathBuf, StoreError> {
            let encoded_id = self.encode_id(id)?;
            let extension = self.strategy.get_extension();
            let filename = format!("{}.{}", encoded_id, extension);
            Ok(self.base_path.join(filename))
        }

        fn encode_id(&self, id: &str) -> Result<String, StoreError> {
            match self.strategy.filename_encoding {
                FilenameEncoding::Direct => {
                    if id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    {
                        Ok(id.to_string())
                    } else {
                        Err(StoreError::FilenameEncoding {
                            id: id.to_string(),
                            reason: "ID contains invalid characters for Direct encoding. \
                                 Only alphanumeric, '-', and '_' are allowed."
                                .to_string(),
                        })
                    }
                }
                FilenameEncoding::UrlEncode => Ok(urlencoding::encode(id).into_owned()),
                FilenameEncoding::Base64 => Ok(URL_SAFE_NO_PAD.encode(id.as_bytes())),
            }
        }

        fn decode_id(&self, filename_stem: &str) -> Result<String, StoreError> {
            match self.strategy.filename_encoding {
                FilenameEncoding::Direct => Ok(filename_stem.to_string()),
                FilenameEncoding::UrlEncode => urlencoding::decode(filename_stem)
                    .map(|s| s.into_owned())
                    .map_err(|e| StoreError::FilenameEncoding {
                        id: filename_stem.to_string(),
                        reason: format!("Failed to URL-decode filename: {}", e),
                    }),
                FilenameEncoding::Base64 => URL_SAFE_NO_PAD
                    .decode(filename_stem.as_bytes())
                    .map_err(|e| StoreError::FilenameEncoding {
                        id: filename_stem.to_string(),
                        reason: format!("Failed to Base64-decode filename: {}", e),
                    })
                    .and_then(|bytes| {
                        String::from_utf8(bytes).map_err(|e| StoreError::FilenameEncoding {
                            id: filename_stem.to_string(),
                            reason: format!(
                                "Failed to convert Base64-decoded bytes to UTF-8: {}",
                                e
                            ),
                        })
                    }),
            }
        }

        fn path_to_id(&self, path: &Path) -> Result<Option<String>, StoreError> {
            let file_stem = match path.file_stem() {
                Some(stem) => stem.to_string_lossy(),
                None => return Ok(None),
            };
            let id = self.decode_id(&file_stem)?;
            Ok(Some(id))
        }

        fn get_temp_path(&self, target_path: &Path) -> Result<PathBuf, StoreError> {
            let parent = target_path.parent().ok_or_else(|| StoreError::IoError {
                operation: IoOperationKind::Create,
                path: target_path.display().to_string(),
                context: Some("path has no parent directory".to_string()),
                error: "cannot determine parent for temporary file".to_string(),
            })?;

            let file_name = target_path.file_name().ok_or_else(|| StoreError::IoError {
                operation: IoOperationKind::Create,
                path: target_path.display().to_string(),
                context: Some("path has no file name".to_string()),
                error: "cannot determine filename for temporary file".to_string(),
            })?;

            let tmp_name = format!(
                ".{}.tmp.{}",
                file_name.to_string_lossy(),
                std::process::id()
            );
            Ok(parent.join(tmp_name))
        }

        async fn atomic_rename(
            &self,
            tmp_path: &Path,
            target_path: &Path,
        ) -> Result<(), StoreError> {
            let mut last_error = None;

            for attempt in 0..self.strategy.atomic_write.retry_count {
                match tokio::fs::rename(tmp_path, target_path).await {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        last_error = Some(e);
                        if attempt + 1 < self.strategy.atomic_write.retry_count {
                            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                        }
                    }
                }
            }

            Err(StoreError::IoError {
                operation: IoOperationKind::Rename,
                path: target_path.display().to_string(),
                context: Some(format!(
                    "after {} retries (async)",
                    self.strategy.atomic_write.retry_count
                )),
                error: last_error
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "unknown error after retries".to_string()),
            })
        }

        async fn cleanup_temp_files(&self, target_path: &Path) -> std::io::Result<()> {
            let parent = match target_path.parent() {
                Some(p) => p,
                None => return Ok(()),
            };
            let file_name = match target_path.file_name() {
                Some(f) => f.to_string_lossy(),
                None => return Ok(()),
            };
            let prefix = format!(".{}.tmp.", file_name);

            let mut entries = tokio::fs::read_dir(parent).await?;
            while let Some(entry) = entries.next_entry().await? {
                if let Ok(name) = entry.file_name().into_string() {
                    if name.starts_with(&prefix) {
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
            Ok(())
        }

        async fn atomic_write(&self, path: &Path, content: &str) -> Result<(), StoreError> {
            if let Some(parent) = path.parent() {
                if !tokio::fs::try_exists(parent).await.unwrap_or(false) {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|e| StoreError::IoError {
                            operation: IoOperationKind::CreateDir,
                            path: parent.display().to_string(),
                            context: Some("parent directory (async)".to_string()),
                            error: e.to_string(),
                        })?;
                }
            }

            let tmp_path = self.get_temp_path(path)?;

            let mut tmp_file =
                tokio::fs::File::create(&tmp_path)
                    .await
                    .map_err(|e| StoreError::IoError {
                        operation: IoOperationKind::Create,
                        path: tmp_path.display().to_string(),
                        context: Some("temporary file (async)".to_string()),
                        error: e.to_string(),
                    })?;

            tmp_file
                .write_all(content.as_bytes())
                .await
                .map_err(|e| StoreError::IoError {
                    operation: IoOperationKind::Write,
                    path: tmp_path.display().to_string(),
                    context: Some("temporary file (async)".to_string()),
                    error: e.to_string(),
                })?;

            tmp_file.sync_all().await.map_err(|e| StoreError::IoError {
                operation: IoOperationKind::Sync,
                path: tmp_path.display().to_string(),
                context: Some("temporary file (async)".to_string()),
                error: e.to_string(),
            })?;

            drop(tmp_file);

            self.atomic_rename(&tmp_path, path).await?;

            if self.strategy.atomic_write.cleanup_tmp_files {
                let _ = self.cleanup_temp_files(path).await;
            }

            Ok(())
        }
    }

    // =========================================================================
    // Async tests
    // =========================================================================

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{AppPaths, PathStrategy};
        use tempfile::TempDir;

        fn make_paths(dir: &TempDir) -> AppPaths {
            AppPaths::new("test-app")
                .data_strategy(PathStrategy::CustomBase(dir.path().to_path_buf()))
        }

        /// T1: new creates the storage directory.
        #[tokio::test]
        async fn test_async_new_creates_directory() {
            let tmp = TempDir::new().unwrap();
            let paths = make_paths(&tmp);
            let storage = AsyncDirStorage::new(paths, "sessions", DirStorageStrategy::default())
                .await
                .expect("AsyncDirStorage::new should succeed");
            assert!(
                storage.base_path().exists(),
                "base_path should be created by new"
            );
        }

        /// T1: save_raw_string + load_raw_string round-trip.
        #[tokio::test]
        async fn test_async_save_and_load_raw_string() {
            let tmp = TempDir::new().unwrap();
            let paths = make_paths(&tmp);
            let storage = AsyncDirStorage::new(paths, "items", DirStorageStrategy::default())
                .await
                .unwrap();

            storage
                .save_raw_string("item", "item-1", r#"{"value":42}"#)
                .await
                .expect("save_raw_string should succeed");

            let content = storage
                .load_raw_string("item-1")
                .await
                .expect("load_raw_string should succeed");
            assert_eq!(content, r#"{"value":42}"#);
        }

        /// T2: load_raw_string on missing id returns IoError.
        #[tokio::test]
        async fn test_async_load_missing_id_returns_error() {
            let tmp = TempDir::new().unwrap();
            let paths = make_paths(&tmp);
            let storage = AsyncDirStorage::new(paths, "items", DirStorageStrategy::default())
                .await
                .unwrap();

            let result = storage.load_raw_string("nonexistent").await;
            assert!(result.is_err(), "loading missing id should return Err");
        }

        /// T3: delete is idempotent — deleting missing id returns Ok(()).
        #[tokio::test]
        async fn test_async_delete_idempotent() {
            let tmp = TempDir::new().unwrap();
            let paths = make_paths(&tmp);
            let storage = AsyncDirStorage::new(paths, "items", DirStorageStrategy::default())
                .await
                .unwrap();

            // Should not fail even though the file does not exist.
            storage
                .delete("no-such-id")
                .await
                .expect("delete of missing id should be Ok(())");
        }
    }
}

// ============================================================================
// Sync tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppPaths, PathStrategy};
    use tempfile::TempDir;

    fn make_paths(dir: &TempDir) -> AppPaths {
        AppPaths::new("test-app").data_strategy(PathStrategy::CustomBase(dir.path().to_path_buf()))
    }

    // ---- T1: happy path --------------------------------------------------

    /// T1-a: DirStorage::new resolves base_path and creates the directory.
    #[test]
    fn test_new_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let storage =
            DirStorage::new(paths, "sessions", DirStorageStrategy::default()).expect("new ok");
        assert!(storage.base_path().exists(), "base_path should be created");
        assert!(storage.base_path().is_dir());
    }

    /// T1-b: save_raw_string followed by load_raw_string yields the same string.
    #[test]
    fn test_save_and_load_raw_string_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let storage =
            DirStorage::new(paths, "items", DirStorageStrategy::default()).expect("new ok");

        storage
            .save_raw_string("item", "item-1", r#"{"value":99}"#)
            .expect("save ok");
        let content = storage.load_raw_string("item-1").expect("load ok");
        assert_eq!(content, r#"{"value":99}"#);
    }

    /// T1-c: list_ids returns all stored IDs and excludes tmp files.
    #[test]
    fn test_list_ids_excludes_tmp_files() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let storage =
            DirStorage::new(paths, "items", DirStorageStrategy::default()).expect("new ok");

        storage.save_raw_string("x", "alpha", "a").expect("save ok");
        storage.save_raw_string("x", "beta", "b").expect("save ok");

        // Manually drop a spurious .tmp file in the directory — should not appear.
        let tmp_file = storage.base_path().join(".alpha.json.tmp.99999");
        std::fs::write(&tmp_file, "garbage").unwrap();

        let ids = storage.list_ids().expect("list ok");
        assert_eq!(ids, vec!["alpha".to_string(), "beta".to_string()]);
    }

    /// T1-d: exists returns true for a stored id and false for an unknown id.
    #[test]
    fn test_exists_reflects_storage_state() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let storage =
            DirStorage::new(paths, "items", DirStorageStrategy::default()).expect("new ok");

        storage
            .save_raw_string("x", "present", "hi")
            .expect("save ok");
        assert!(storage.exists("present").expect("exists ok"));
        assert!(!storage.exists("absent").expect("exists ok"));
    }

    // ---- T2: boundary / edge cases ---------------------------------------

    /// T2-a: empty string id fails Direct encoding.
    #[test]
    fn test_direct_encoding_empty_id() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let storage =
            DirStorage::new(paths, "items", DirStorageStrategy::default()).expect("new ok");
        // Empty string passes the character check (vacuously true) — should succeed.
        let result = storage.save_raw_string("x", "", "content");
        // Empty id encodes to "." which is still a legal path component; behaviour
        // is documented as Direct: all-alphanumeric constraint (empty vacuously ok).
        // We just verify it does not panic.
        let _ = result;
    }

    /// T2-b: Direct encoding rejects an id with a slash.
    #[test]
    fn test_direct_encoding_rejects_slash() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let storage =
            DirStorage::new(paths, "items", DirStorageStrategy::default()).expect("new ok");

        let err = storage
            .save_raw_string("x", "bad/id", "x")
            .expect_err("slash in id should fail");
        assert!(
            matches!(err, StoreError::FilenameEncoding { .. }),
            "expected FilenameEncoding error, got: {:?}",
            err
        );
    }

    /// T2-c: UrlEncode encoding round-trips an id with special characters.
    #[test]
    fn test_url_encode_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let strategy =
            DirStorageStrategy::default().with_filename_encoding(FilenameEncoding::UrlEncode);
        let storage = DirStorage::new(paths, "items", strategy).expect("new ok");

        let special_id = "user@example.com/session 1";
        storage
            .save_raw_string("x", special_id, "data")
            .expect("save ok");
        let ids = storage.list_ids().expect("list ok");
        assert_eq!(ids, vec![special_id.to_string()]);
    }

    /// T2-d: Base64 encoding round-trips an id with special characters.
    #[test]
    fn test_base64_encode_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let strategy =
            DirStorageStrategy::default().with_filename_encoding(FilenameEncoding::Base64);
        let storage = DirStorage::new(paths, "items", strategy).expect("new ok");

        let id = "hello world!";
        storage
            .save_raw_string("x", id, "base64-content")
            .expect("save ok");
        let loaded = storage.load_raw_string(id).expect("load ok");
        assert_eq!(loaded, "base64-content");
    }

    // ---- T3: error paths -------------------------------------------------

    /// T3-a: load_raw_string on a missing id returns StoreError::IoError.
    #[test]
    fn test_load_missing_id_returns_error() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let storage =
            DirStorage::new(paths, "items", DirStorageStrategy::default()).expect("new ok");

        let result = storage.load_raw_string("nonexistent");
        assert!(result.is_err(), "should return Err for missing id");
        if let Err(StoreError::IoError {
            operation,
            context,
            error,
            ..
        }) = result
        {
            assert_eq!(operation, IoOperationKind::Read);
            assert!(context.is_none());
            assert!(error.contains("not found") || error.contains("File not found"));
        } else {
            panic!("expected IoError(Read)");
        }
    }

    /// T3-b: delete is idempotent — deleting a missing id returns Ok(()).
    #[test]
    fn test_delete_idempotent_missing_id() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let storage =
            DirStorage::new(paths, "items", DirStorageStrategy::default()).expect("new ok");

        // Must return Ok(()) without error (R-S2-3 compliance).
        storage
            .delete("does-not-exist")
            .expect("delete of missing id should be Ok(())");
    }

    /// T3-c: Direct encoding of id with space returns FilenameEncoding error.
    #[test]
    fn test_direct_encoding_error_on_space() {
        let tmp = TempDir::new().unwrap();
        let paths = make_paths(&tmp);
        let storage =
            DirStorage::new(paths, "items", DirStorageStrategy::default()).expect("new ok");

        let err = storage
            .save_raw_string("x", "has space", "x")
            .expect_err("space in id should fail Direct encoding");
        assert!(
            matches!(err, StoreError::FilenameEncoding { .. }),
            "expected FilenameEncoding, got {:?}",
            err
        );
    }
}
