//! Directory-based storage layer for managing multiple entity files.
//!
//! Provides per-entity file storage with ACID guarantees, automatic migrations,
//! and flexible file naming strategies. Unlike `FileStorage` which stores multiple
//! entities in a single file, `DirStorage` creates one file per entity.
//!
//! # Use Cases
//!
//! - Session management: `sessions/session-123.json`
//! - Task management: `tasks/task-456.json`
//! - User data: `users/user-789.json`
//!
//! # Example
//!
//! ```ignore
//! use version_migrate::{AppPaths, Migrator, DirStorage, DirStorageStrategy};
//!
//! // Setup migrator with entity paths
//! let mut migrator = Migrator::new();
//! let session_path = Migrator::define("session")
//!     .from::<SessionV1_0_0>()
//!     .step::<SessionV1_1_0>()
//!     .into_with_save::<SessionEntity>();
//! migrator.register(session_path)?;
//!
//! // Create DirStorage
//! let paths = AppPaths::new("myapp");
//! let storage = DirStorage::new(
//!     paths,
//!     "sessions",
//!     migrator,
//!     DirStorageStrategy::default(),
//! )?;
//!
//! // Save and load entities
//! let session = SessionEntity { /* ... */ };
//! storage.save("session", "session-123", session)?;
//! let loaded: SessionEntity = storage.load("session", "session-123")?;
//! ```

use crate::{AppPaths, MigrationError, Migrator};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

// Re-export shared types from storage module
pub use crate::storage::{AtomicWriteConfig, FormatStrategy};

/// File naming encoding strategy for entity IDs.
///
/// Determines how entity IDs are encoded into filesystem-safe filenames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilenameEncoding {
    /// Use ID directly as filename (safe characters only: alphanumeric, `-`, `_`)
    Direct,
    /// URL-encode the ID (for IDs with special characters)
    UrlEncode,
    /// Base64-encode the ID (for maximum safety)
    Base64,
}

impl Default for FilenameEncoding {
    fn default() -> Self {
        Self::Direct
    }
}

/// Strategy configuration for directory-based storage operations.
#[derive(Debug, Clone)]
pub struct DirStorageStrategy {
    /// File format to use (JSON or TOML)
    pub format: FormatStrategy,
    /// Atomic write configuration
    pub atomic_write: AtomicWriteConfig,
    /// Custom file extension (if None, derived from format)
    pub extension: Option<String>,
    /// File naming encoding strategy
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
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the file format.
    #[allow(dead_code)]
    pub fn with_format(mut self, format: FormatStrategy) -> Self {
        self.format = format;
        self
    }

    /// Set a custom file extension.
    #[allow(dead_code)]
    pub fn with_extension(mut self, ext: impl Into<String>) -> Self {
        self.extension = Some(ext.into());
        self
    }

    /// Set the filename encoding strategy.
    #[allow(dead_code)]
    pub fn with_filename_encoding(mut self, encoding: FilenameEncoding) -> Self {
        self.filename_encoding = encoding;
        self
    }

    /// Set the retry count for atomic writes.
    #[allow(dead_code)]
    pub fn with_retry_count(mut self, count: usize) -> Self {
        self.atomic_write.retry_count = count;
        self
    }

    /// Set whether to cleanup temporary files.
    #[allow(dead_code)]
    pub fn with_cleanup(mut self, cleanup: bool) -> Self {
        self.atomic_write.cleanup_tmp_files = cleanup;
        self
    }

    /// Get the file extension (derived from format if not explicitly set).
    fn get_extension(&self) -> String {
        self.extension.clone().unwrap_or_else(|| match self.format {
            FormatStrategy::Json => "json".to_string(),
            FormatStrategy::Toml => "toml".to_string(),
        })
    }
}

/// Directory-based entity storage with ACID guarantees and automatic migrations.
///
/// Manages one file per entity, providing:
/// - **Atomicity**: Updates are all-or-nothing via tmp file + atomic rename
/// - **Consistency**: Format validation on load/save
/// - **Isolation**: Each entity has its own file
/// - **Durability**: Explicit fsync before rename
pub struct DirStorage {
    /// Resolved base directory path
    base_path: PathBuf,
    /// Migrator instance for handling version migrations
    migrator: Migrator,
    /// Storage strategy configuration
    strategy: DirStorageStrategy,
}

impl DirStorage {
    /// Create a new DirStorage instance.
    ///
    /// # Arguments
    ///
    /// * `paths` - Application paths manager
    /// * `domain_name` - Domain-specific subdirectory name (e.g., "sessions", "tasks")
    /// * `migrator` - Migrator instance with registered migration paths
    /// * `strategy` - Storage strategy configuration
    ///
    /// # Behavior
    ///
    /// - Resolves the base path using `paths.data_dir()?.join(domain_name)`
    /// - Creates the directory if it doesn't exist
    /// - Does not load existing files (lazy loading)
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::IoError` if directory creation fails.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let paths = AppPaths::new("myapp");
    /// let storage = DirStorage::new(
    ///     paths,
    ///     "sessions",
    ///     migrator,
    ///     DirStorageStrategy::default(),
    /// )?;
    /// ```
    pub fn new(
        paths: AppPaths,
        domain_name: &str,
        migrator: Migrator,
        strategy: DirStorageStrategy,
    ) -> Result<Self, MigrationError> {
        // Resolve base path: data_dir/domain_name
        let base_path = paths.data_dir()?.join(domain_name);

        // Create directory if it doesn't exist
        if !base_path.exists() {
            std::fs::create_dir_all(&base_path).map_err(|e| MigrationError::IoError {
                path: base_path.display().to_string(),
                error: e.to_string(),
            })?;
        }

        Ok(Self {
            base_path,
            migrator,
            strategy,
        })
    }

    /// Save an entity to a file.
    ///
    /// # Arguments
    ///
    /// * `entity_name` - The entity name registered in the migrator
    /// * `id` - The unique identifier for this entity (used as filename)
    /// * `entity` - The entity to save
    ///
    /// # Process
    ///
    /// 1. Converts the entity to its latest versioned DTO
    /// 2. Serializes to the configured format (JSON/TOML)
    /// 3. Writes atomically using temporary file + rename
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Entity name not registered in migrator
    /// - ID contains invalid characters (for Direct encoding)
    /// - Serialization fails
    /// - File write fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let session = SessionEntity {
    ///     id: "session-123".to_string(),
    ///     user_id: "user-456".to_string(),
    /// };
    /// storage.save("session", "session-123", session)?;
    /// ```
    pub fn save<T>(&self, entity_name: &str, id: &str, entity: T) -> Result<(), MigrationError>
    where
        T: serde::Serialize,
    {
        // Convert entity to latest versioned DTO and get JSON string
        let json_string = self.migrator.save_domain_flat(entity_name, entity)?;

        // Parse back to JSON value for format conversion
        let versioned_value: serde_json::Value = serde_json::from_str(&json_string)
            .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;

        // Serialize to target format (JSON or TOML)
        let content = self.serialize_content(&versioned_value)?;

        // Get target file path
        let file_path = self.id_to_path(id)?;

        // Write atomically
        self.atomic_write(&file_path, &content)?;

        Ok(())
    }

    /// Convert an entity ID to a file path.
    ///
    /// Encodes the ID according to the configured filename encoding strategy
    /// and appends the appropriate file extension.
    ///
    /// # Arguments
    ///
    /// * `id` - The entity ID
    ///
    /// # Returns
    ///
    /// Full path: `base_path/encoded_id.extension`
    ///
    /// # Errors
    ///
    /// Returns error if ID encoding fails (e.g., invalid characters for Direct encoding).
    fn id_to_path(&self, id: &str) -> Result<PathBuf, MigrationError> {
        let encoded_id = self.encode_id(id)?;
        let extension = self.strategy.get_extension();
        let filename = format!("{}.{}", encoded_id, extension);
        Ok(self.base_path.join(filename))
    }

    /// Encode an entity ID to a filesystem-safe filename.
    ///
    /// # Arguments
    ///
    /// * `id` - The entity ID to encode
    ///
    /// # Encoding Strategies
    ///
    /// - **Direct**: Use ID as-is (validates alphanumeric, `-`, `_` only)
    /// - **UrlEncode**: URL-encode special characters (not yet implemented)
    /// - **Base64**: Base64-encode the ID (not yet implemented)
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::FilenameEncoding` if:
    /// - Direct encoding with invalid characters
    /// - Encoding strategy not yet implemented
    fn encode_id(&self, id: &str) -> Result<String, MigrationError> {
        match self.strategy.filename_encoding {
            FilenameEncoding::Direct => {
                // Validate that ID contains only safe characters
                if id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    Ok(id.to_string())
                } else {
                    Err(MigrationError::FilenameEncoding {
                        id: id.to_string(),
                        reason: "ID contains invalid characters for Direct encoding. Only alphanumeric, '-', and '_' are allowed.".to_string(),
                    })
                }
            }
            FilenameEncoding::UrlEncode => {
                // TODO: Implement URL encoding
                todo!("UrlEncode is not yet implemented")
            }
            FilenameEncoding::Base64 => {
                // TODO: Implement Base64 encoding
                todo!("Base64 encoding is not yet implemented")
            }
        }
    }

    /// Serialize a JSON value to string based on the configured format.
    ///
    /// # Arguments
    ///
    /// * `value` - The JSON value to serialize
    ///
    /// # Returns
    ///
    /// Pretty-printed string in the configured format (JSON or TOML).
    ///
    /// # Errors
    ///
    /// Returns error if serialization or format conversion fails.
    fn serialize_content(&self, value: &serde_json::Value) -> Result<String, MigrationError> {
        match self.strategy.format {
            FormatStrategy::Json => serde_json::to_string_pretty(value)
                .map_err(|e| MigrationError::SerializationError(e.to_string())),
            FormatStrategy::Toml => {
                let toml_value = json_to_toml(value)?;
                toml::to_string_pretty(&toml_value)
                    .map_err(|e| MigrationError::TomlSerializeError(e.to_string()))
            }
        }
    }

    /// Write content to a file atomically.
    ///
    /// Uses the "temporary file + fsync + atomic rename" pattern to ensure
    /// durability and atomicity.
    ///
    /// # Process
    ///
    /// 1. Create temporary file with unique name (`.filename.tmp.{pid}`)
    /// 2. Write content to temporary file
    /// 3. Sync to disk (fsync)
    /// 4. Atomically rename to target path
    /// 5. Retry on failure (configured retry count)
    /// 6. Clean up old temporary files (best effort)
    ///
    /// # Arguments
    ///
    /// * `path` - Target file path
    /// * `content` - Content to write
    ///
    /// # Errors
    ///
    /// Returns error if file creation, write, sync, or rename fails.
    fn atomic_write(&self, path: &Path, content: &str) -> Result<(), MigrationError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| MigrationError::IoError {
                    path: parent.display().to_string(),
                    error: e.to_string(),
                })?;
            }
        }

        // Create temporary file path
        let tmp_path = self.get_temp_path(path)?;

        // Write to temporary file
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
        self.atomic_rename(&tmp_path, path)?;

        // Cleanup old temp files (best effort)
        if self.strategy.atomic_write.cleanup_tmp_files {
            let _ = self.cleanup_temp_files(path);
        }

        Ok(())
    }

    /// Get path to temporary file for atomic writes.
    ///
    /// Creates a unique temporary filename in the same directory as the target file.
    /// Format: `.{filename}.tmp.{process_id}`
    ///
    /// # Arguments
    ///
    /// * `target_path` - The target file path
    ///
    /// # Returns
    ///
    /// Path to temporary file in the same directory.
    ///
    /// # Errors
    ///
    /// Returns error if the path has no parent directory or filename.
    fn get_temp_path(&self, target_path: &Path) -> Result<PathBuf, MigrationError> {
        let parent = target_path.parent().ok_or_else(|| {
            MigrationError::PathResolution("Path has no parent directory".to_string())
        })?;

        let file_name = target_path
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
    ///
    /// Retries the rename operation according to the configured retry count,
    /// with a small delay between attempts.
    ///
    /// # Arguments
    ///
    /// * `tmp_path` - Path to temporary file
    /// * `target_path` - Target file path
    ///
    /// # Errors
    ///
    /// Returns error if all retry attempts fail.
    fn atomic_rename(&self, tmp_path: &Path, target_path: &Path) -> Result<(), MigrationError> {
        let mut last_error = None;

        for attempt in 0..self.strategy.atomic_write.retry_count {
            match fs::rename(tmp_path, target_path) {
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
            path: target_path.display().to_string(),
            error: format!(
                "Failed to rename after {} attempts: {}",
                self.strategy.atomic_write.retry_count,
                last_error.unwrap()
            ),
        })
    }

    /// Clean up old temporary files (best effort).
    ///
    /// Attempts to remove old temporary files that may have been left behind
    /// from previous failed operations. Errors are silently ignored.
    ///
    /// # Arguments
    ///
    /// * `target_path` - The target file path (used to find related temp files)
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
                        // Try to remove, but ignore errors (best effort)
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }

        Ok(())
    }

    /// Load an entity from a file.
    ///
    /// # Arguments
    ///
    /// * `entity_name` - The entity name registered in the migrator
    /// * `id` - The unique identifier for the entity
    ///
    /// # Process
    ///
    /// 1. Gets the file path using `id_to_path`
    /// 2. Reads the file content to a string
    /// 3. Deserializes the content to a `serde_json::Value`
    /// 4. Migrates the `Value` to the target domain type
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Entity name not registered in migrator
    /// - File not found
    /// - Deserialization fails
    /// - Migration fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let session: SessionEntity = storage.load("session", "session-123")?;
    /// ```
    pub fn load<D>(&self, entity_name: &str, id: &str) -> Result<D, MigrationError>
    where
        D: serde::de::DeserializeOwned,
    {
        // Get file path
        let file_path = self.id_to_path(id)?;

        // Check if file exists
        if !file_path.exists() {
            return Err(MigrationError::IoError {
                path: file_path.display().to_string(),
                error: "File not found".to_string(),
            });
        }

        // Read file content
        let content = fs::read_to_string(&file_path).map_err(|e| MigrationError::IoError {
            path: file_path.display().to_string(),
            error: e.to_string(),
        })?;

        // Deserialize content to JSON value
        let value = self.deserialize_content(&content)?;

        // Migrate to domain type using load_flat_from
        self.migrator.load_flat_from(entity_name, value)
    }

    /// List all entity IDs in the storage directory.
    ///
    /// # Returns
    ///
    /// A sorted vector of entity IDs (decoded from filenames).
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Directory read fails
    /// - Filename decoding fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ids = storage.list_ids()?;
    /// for id in ids {
    ///     println!("Found entity: {}", id);
    /// }
    /// ```
    pub fn list_ids(&self) -> Result<Vec<String>, MigrationError> {
        // Read directory
        let entries = fs::read_dir(&self.base_path).map_err(|e| MigrationError::IoError {
            path: self.base_path.display().to_string(),
            error: e.to_string(),
        })?;

        let extension = self.strategy.get_extension();
        let mut ids = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|e| MigrationError::IoError {
                path: self.base_path.display().to_string(),
                error: e.to_string(),
            })?;

            let path = entry.path();

            // Check if it's a file with the correct extension
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == extension.as_str() {
                        // Extract ID from filename
                        if let Some(id) = self.path_to_id(&path)? {
                            ids.push(id);
                        }
                    }
                }
            }
        }

        // Sort IDs for consistent ordering
        ids.sort();
        Ok(ids)
    }

    /// Load all entities from the storage directory.
    ///
    /// # Arguments
    ///
    /// * `entity_name` - The entity name registered in the migrator
    ///
    /// # Returns
    ///
    /// A vector of `(id, entity)` tuples.
    ///
    /// # Errors
    ///
    /// Returns error if any entity fails to load. This operation is atomic:
    /// if any load fails, the whole operation fails.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let sessions: Vec<(String, SessionEntity)> = storage.load_all("session")?;
    /// for (id, session) in sessions {
    ///     println!("Loaded session {} for user {}", id, session.user_id);
    /// }
    /// ```
    pub fn load_all<D>(&self, entity_name: &str) -> Result<Vec<(String, D)>, MigrationError>
    where
        D: serde::de::DeserializeOwned,
    {
        let ids = self.list_ids()?;
        let mut results = Vec::new();

        for id in ids {
            let entity = self.load(entity_name, &id)?;
            results.push((id, entity));
        }

        Ok(results)
    }

    /// Check if an entity exists.
    ///
    /// # Arguments
    ///
    /// * `id` - The entity ID
    ///
    /// # Returns
    ///
    /// `true` if the file exists and is a file, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```ignore
    /// if storage.exists("session-123")? {
    ///     println!("Session exists");
    /// }
    /// ```
    pub fn exists(&self, id: &str) -> Result<bool, MigrationError> {
        let file_path = self.id_to_path(id)?;
        Ok(file_path.exists() && file_path.is_file())
    }

    /// Delete an entity file.
    ///
    /// # Arguments
    ///
    /// * `id` - The entity ID
    ///
    /// # Behavior
    ///
    /// This operation is idempotent: deleting a non-existent file is not an error.
    ///
    /// # Errors
    ///
    /// Returns error if file deletion fails (but not if file doesn't exist).
    ///
    /// # Example
    ///
    /// ```ignore
    /// storage.delete("session-123")?;
    /// ```
    pub fn delete(&self, id: &str) -> Result<(), MigrationError> {
        let file_path = self.id_to_path(id)?;

        if file_path.exists() {
            fs::remove_file(&file_path).map_err(|e| MigrationError::IoError {
                path: file_path.display().to_string(),
                error: e.to_string(),
            })?;
        }

        Ok(())
    }

    /// Deserialize file content to a JSON value.
    ///
    /// # Arguments
    ///
    /// * `content` - The file content as a string
    ///
    /// # Returns
    ///
    /// A `serde_json::Value` representing the deserialized content.
    ///
    /// # Errors
    ///
    /// Returns error if deserialization fails.
    fn deserialize_content(&self, content: &str) -> Result<serde_json::Value, MigrationError> {
        match self.strategy.format {
            FormatStrategy::Json => serde_json::from_str(content)
                .map_err(|e| MigrationError::DeserializationError(e.to_string())),
            FormatStrategy::Toml => {
                let toml_value: toml::Value = toml::from_str(content)
                    .map_err(|e| MigrationError::TomlParseError(e.to_string()))?;
                toml_to_json(toml_value)
            }
        }
    }

    /// Extract the entity ID from a file path.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path
    ///
    /// # Returns
    ///
    /// `Some(id)` if the path is valid, `None` otherwise.
    ///
    /// # Errors
    ///
    /// Returns error if ID decoding fails.
    fn path_to_id(&self, path: &Path) -> Result<Option<String>, MigrationError> {
        // Get file stem (filename without extension)
        let file_stem = match path.file_stem() {
            Some(stem) => stem.to_string_lossy(),
            None => return Ok(None),
        };

        // Decode ID
        let id = self.decode_id(&file_stem)?;
        Ok(Some(id))
    }

    /// Decode a filename stem to an entity ID.
    ///
    /// # Arguments
    ///
    /// * `filename_stem` - The filename without extension
    ///
    /// # Returns
    ///
    /// The decoded entity ID.
    ///
    /// # Encoding Strategies
    ///
    /// - **Direct**: Use filename as-is (no decoding needed)
    /// - **UrlEncode**: URL-decode the filename (not yet implemented)
    /// - **Base64**: Base64-decode the filename (not yet implemented)
    ///
    /// # Errors
    ///
    /// Returns error if decoding fails or strategy is not yet implemented.
    fn decode_id(&self, filename_stem: &str) -> Result<String, MigrationError> {
        match self.strategy.filename_encoding {
            FilenameEncoding::Direct => {
                // Direct encoding: filename is the ID
                Ok(filename_stem.to_string())
            }
            FilenameEncoding::UrlEncode => {
                // TODO: Implement URL decoding
                todo!("UrlEncode decoding is not yet implemented")
            }
            FilenameEncoding::Base64 => {
                // TODO: Implement Base64 decoding
                todo!("Base64 decoding is not yet implemented")
            }
        }
    }
}

/// Convert JSON value to TOML value.
///
/// Helper function for format conversion during serialization.
fn json_to_toml(json_value: &serde_json::Value) -> Result<toml::Value, MigrationError> {
    let json_str = serde_json::to_string(json_value)
        .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
    let toml_value: toml::Value = serde_json::from_str(&json_str)
        .map_err(|e| MigrationError::TomlParseError(e.to_string()))?;
    Ok(toml_value)
}

/// Convert TOML value to JSON value.
///
/// Helper function for format conversion during deserialization.
fn toml_to_json(toml_value: toml::Value) -> Result<serde_json::Value, MigrationError> {
    let json_str = serde_json::to_string(&toml_value)
        .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
    let json_value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;
    Ok(json_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_filename_encoding_default() {
        assert_eq!(FilenameEncoding::default(), FilenameEncoding::Direct);
    }

    #[test]
    fn test_dir_storage_strategy_default() {
        let strategy = DirStorageStrategy::default();
        assert_eq!(strategy.format, FormatStrategy::Json);
        assert_eq!(strategy.extension, None);
        assert_eq!(strategy.filename_encoding, FilenameEncoding::Direct);
    }

    #[test]
    fn test_dir_storage_strategy_builder() {
        let strategy = DirStorageStrategy::new()
            .with_format(FormatStrategy::Toml)
            .with_extension("data")
            .with_filename_encoding(FilenameEncoding::Base64)
            .with_retry_count(5)
            .with_cleanup(false);

        assert_eq!(strategy.format, FormatStrategy::Toml);
        assert_eq!(strategy.extension, Some("data".to_string()));
        assert_eq!(strategy.filename_encoding, FilenameEncoding::Base64);
        assert_eq!(strategy.atomic_write.retry_count, 5);
        assert!(!strategy.atomic_write.cleanup_tmp_files);
    }

    #[test]
    fn test_dir_storage_strategy_get_extension() {
        // Default from JSON format
        let strategy1 = DirStorageStrategy::default();
        assert_eq!(strategy1.get_extension(), "json");

        // Default from TOML format
        let strategy2 = DirStorageStrategy::default().with_format(FormatStrategy::Toml);
        assert_eq!(strategy2.get_extension(), "toml");

        // Custom extension
        let strategy3 = DirStorageStrategy::default().with_extension("custom");
        assert_eq!(strategy3.get_extension(), "custom");
    }

    #[test]
    fn test_dir_storage_new_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = Migrator::new();
        let strategy = DirStorageStrategy::default();

        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Verify directory was created
        assert!(storage.base_path.exists());
        assert!(storage.base_path.is_dir());
        assert!(storage.base_path.ends_with("data/testapp/sessions"));
    }

    #[test]
    fn test_dir_storage_new_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator1 = Migrator::new();
        let migrator2 = Migrator::new();
        let strategy = DirStorageStrategy::default();

        // Create storage twice
        let storage1 =
            DirStorage::new(paths.clone(), "sessions", migrator1, strategy.clone()).unwrap();
        let storage2 = DirStorage::new(paths, "sessions", migrator2, strategy).unwrap();

        // Both should succeed and point to the same directory
        assert_eq!(storage1.base_path, storage2.base_path);
    }

    // Test entity types for save tests
    use crate::{FromDomain, IntoDomain, MigratesTo, Versioned};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct SessionV1_0_0 {
        id: String,
        user_id: String,
    }

    impl Versioned for SessionV1_0_0 {
        const VERSION: &'static str = "1.0.0";
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct SessionV1_1_0 {
        id: String,
        user_id: String,
        created_at: Option<String>,
    }

    impl Versioned for SessionV1_1_0 {
        const VERSION: &'static str = "1.1.0";
    }

    impl MigratesTo<SessionV1_1_0> for SessionV1_0_0 {
        fn migrate(self) -> SessionV1_1_0 {
            SessionV1_1_0 {
                id: self.id,
                user_id: self.user_id,
                created_at: None,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct SessionEntity {
        id: String,
        user_id: String,
        created_at: Option<String>,
    }

    impl IntoDomain<SessionEntity> for SessionV1_1_0 {
        fn into_domain(self) -> SessionEntity {
            SessionEntity {
                id: self.id,
                user_id: self.user_id,
                created_at: self.created_at,
            }
        }
    }

    impl FromDomain<SessionEntity> for SessionV1_1_0 {
        fn from_domain(domain: SessionEntity) -> Self {
            SessionV1_1_0 {
                id: domain.id,
                user_id: domain.user_id,
                created_at: domain.created_at,
            }
        }
    }

    fn setup_session_migrator() -> Migrator {
        let path = Migrator::define("session")
            .from::<SessionV1_0_0>()
            .step::<SessionV1_1_0>()
            .into_with_save::<SessionEntity>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();
        migrator
    }

    #[test]
    fn test_dir_storage_save_json() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default().with_format(FormatStrategy::Json);
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Create a session entity
        let session = SessionEntity {
            id: "session-123".to_string(),
            user_id: "user-456".to_string(),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        // Save the entity
        storage.save("session", "session-123", session).unwrap();

        // Verify file was created
        let file_path = storage.base_path.join("session-123.json");
        assert!(file_path.exists());

        // Verify content is valid JSON with version
        let content = std::fs::read_to_string(&file_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["version"], "1.1.0");
        assert_eq!(json["id"], "session-123");
        assert_eq!(json["user_id"], "user-456");
    }

    #[test]
    fn test_dir_storage_save_toml() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default().with_format(FormatStrategy::Toml);
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Create a session entity with Some value (TOML doesn't support None/null)
        let session = SessionEntity {
            id: "session-789".to_string(),
            user_id: "user-101".to_string(),
            created_at: Some("2024-01-15T10:30:00Z".to_string()),
        };

        // Save the entity
        storage.save("session", "session-789", session).unwrap();

        // Verify file was created
        let file_path = storage.base_path.join("session-789.toml");
        assert!(file_path.exists());

        // Verify content is valid TOML with version
        let content = std::fs::read_to_string(&file_path).unwrap();
        let toml: toml::Value = toml::from_str(&content).unwrap();
        assert_eq!(toml["version"].as_str().unwrap(), "1.1.0");
        assert_eq!(toml["id"].as_str().unwrap(), "session-789");
        assert_eq!(toml["created_at"].as_str().unwrap(), "2024-01-15T10:30:00Z");
    }

    #[test]
    fn test_dir_storage_save_with_invalid_id() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        let session = SessionEntity {
            id: "invalid/id".to_string(),
            user_id: "user-456".to_string(),
            created_at: None,
        };

        // Should fail due to invalid characters in ID
        let result = storage.save("session", "invalid/id", session);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::MigrationError::FilenameEncoding { .. }
        ));
    }

    #[test]
    fn test_dir_storage_save_with_custom_extension() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default()
            .with_format(FormatStrategy::Json)
            .with_extension("data");
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        let session = SessionEntity {
            id: "session-custom".to_string(),
            user_id: "user-999".to_string(),
            created_at: None,
        };

        storage.save("session", "session-custom", session).unwrap();

        // Verify custom extension is used
        let file_path = storage.base_path.join("session-custom.data");
        assert!(file_path.exists());
    }

    #[test]
    fn test_dir_storage_save_overwrites_existing() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Save initial version
        let session1 = SessionEntity {
            id: "session-overwrite".to_string(),
            user_id: "user-111".to_string(),
            created_at: Some("2024-01-01".to_string()),
        };
        storage
            .save("session", "session-overwrite", session1)
            .unwrap();

        // Save updated version
        let session2 = SessionEntity {
            id: "session-overwrite".to_string(),
            user_id: "user-222".to_string(),
            created_at: Some("2024-01-02".to_string()),
        };
        storage
            .save("session", "session-overwrite", session2)
            .unwrap();

        // Verify file was overwritten
        let file_path = storage.base_path.join("session-overwrite.json");
        let content = std::fs::read_to_string(&file_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["user_id"], "user-222");
        assert_eq!(json["created_at"], "2024-01-02");
    }

    #[test]
    fn test_dir_storage_load_success() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Save a session
        let session = SessionEntity {
            id: "session-load".to_string(),
            user_id: "user-999".to_string(),
            created_at: Some("2024-02-01".to_string()),
        };
        storage
            .save("session", "session-load", session.clone())
            .unwrap();

        // Load it back
        let loaded: SessionEntity = storage.load("session", "session-load").unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.user_id, session.user_id);
        assert_eq!(loaded.created_at, session.created_at);
    }

    #[test]
    fn test_dir_storage_load_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Try to load non-existent session
        let result: Result<SessionEntity, _> = storage.load("session", "non-existent");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MigrationError::IoError { .. }
        ));
    }

    #[test]
    fn test_dir_storage_save_and_load_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Test multiple sessions
        let sessions = vec![
            SessionEntity {
                id: "session-1".to_string(),
                user_id: "user-1".to_string(),
                created_at: Some("2024-01-01".to_string()),
            },
            SessionEntity {
                id: "session-2".to_string(),
                user_id: "user-2".to_string(),
                created_at: None,
            },
            SessionEntity {
                id: "session-3".to_string(),
                user_id: "user-3".to_string(),
                created_at: Some("2024-03-01".to_string()),
            },
        ];

        // Save all sessions
        for session in &sessions {
            storage
                .save("session", &session.id, session.clone())
                .unwrap();
        }

        // Load and verify each session
        for session in &sessions {
            let loaded: SessionEntity = storage.load("session", &session.id).unwrap();
            assert_eq!(loaded.id, session.id);
            assert_eq!(loaded.user_id, session.user_id);
            assert_eq!(loaded.created_at, session.created_at);
        }
    }

    #[test]
    fn test_dir_storage_list_ids_empty() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // List IDs from empty directory
        let ids = storage.list_ids().unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn test_dir_storage_list_ids() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Save multiple sessions
        let ids = vec!["session-c", "session-a", "session-b"];
        for id in &ids {
            let session = SessionEntity {
                id: id.to_string(),
                user_id: "user".to_string(),
                created_at: None,
            };
            storage.save("session", id, session).unwrap();
        }

        // List IDs
        let listed_ids = storage.list_ids().unwrap();
        assert_eq!(listed_ids.len(), 3);
        // Should be sorted
        assert_eq!(listed_ids, vec!["session-a", "session-b", "session-c"]);
    }

    #[test]
    fn test_dir_storage_load_all_empty() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Load all from empty directory
        let results: Vec<(String, SessionEntity)> = storage.load_all("session").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_dir_storage_load_all() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Save multiple sessions
        let sessions = vec![
            SessionEntity {
                id: "session-x".to_string(),
                user_id: "user-x".to_string(),
                created_at: Some("2024-01-01".to_string()),
            },
            SessionEntity {
                id: "session-y".to_string(),
                user_id: "user-y".to_string(),
                created_at: None,
            },
            SessionEntity {
                id: "session-z".to_string(),
                user_id: "user-z".to_string(),
                created_at: Some("2024-03-01".to_string()),
            },
        ];

        for session in &sessions {
            storage
                .save("session", &session.id, session.clone())
                .unwrap();
        }

        // Load all
        let results: Vec<(String, SessionEntity)> = storage.load_all("session").unwrap();
        assert_eq!(results.len(), 3);

        // Verify all sessions are loaded
        for (id, loaded) in &results {
            let original = sessions.iter().find(|s| &s.id == id).unwrap();
            assert_eq!(loaded.id, original.id);
            assert_eq!(loaded.user_id, original.user_id);
            assert_eq!(loaded.created_at, original.created_at);
        }
    }

    #[test]
    fn test_dir_storage_exists() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Non-existent file
        assert!(!storage.exists("session-exists").unwrap());

        // Save a session
        let session = SessionEntity {
            id: "session-exists".to_string(),
            user_id: "user-exists".to_string(),
            created_at: None,
        };
        storage.save("session", "session-exists", session).unwrap();

        // Should exist now
        assert!(storage.exists("session-exists").unwrap());
    }

    #[test]
    fn test_dir_storage_delete() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Save a session
        let session = SessionEntity {
            id: "session-delete".to_string(),
            user_id: "user-delete".to_string(),
            created_at: None,
        };
        storage.save("session", "session-delete", session).unwrap();

        // Verify it exists
        assert!(storage.exists("session-delete").unwrap());

        // Delete it
        storage.delete("session-delete").unwrap();

        // Verify it doesn't exist
        assert!(!storage.exists("session-delete").unwrap());
    }

    #[test]
    fn test_dir_storage_delete_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Delete non-existent file (should not error)
        storage.delete("non-existent").unwrap();

        // Delete again (should still not error)
        storage.delete("non-existent").unwrap();
    }

    #[test]
    fn test_dir_storage_load_toml() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default().with_format(FormatStrategy::Toml);
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Save a session
        let session = SessionEntity {
            id: "session-toml".to_string(),
            user_id: "user-toml".to_string(),
            created_at: Some("2024-04-01".to_string()),
        };
        storage
            .save("session", "session-toml", session.clone())
            .unwrap();

        // Load it back
        let loaded: SessionEntity = storage.load("session", "session-toml").unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.user_id, session.user_id);
        assert_eq!(loaded.created_at, session.created_at);
    }

    #[test]
    fn test_dir_storage_list_ids_with_custom_extension() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default().with_extension("data");
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Save sessions with custom extension
        let session = SessionEntity {
            id: "session-ext".to_string(),
            user_id: "user-ext".to_string(),
            created_at: None,
        };
        storage.save("session", "session-ext", session).unwrap();

        // List IDs should find the file
        let ids = storage.list_ids().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "session-ext");
    }

    #[test]
    fn test_dir_storage_load_all_atomic_failure() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Save valid sessions
        let session1 = SessionEntity {
            id: "session-1".to_string(),
            user_id: "user-1".to_string(),
            created_at: None,
        };
        storage.save("session", "session-1", session1).unwrap();

        // Manually create a corrupted file
        let corrupted_path = storage.base_path.join("session-corrupted.json");
        std::fs::write(&corrupted_path, "invalid json {{{").unwrap();

        // load_all should fail
        let result: Result<Vec<(String, SessionEntity)>, _> = storage.load_all("session");
        assert!(result.is_err());
    }
}
