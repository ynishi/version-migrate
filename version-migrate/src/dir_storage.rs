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
use std::path::Path;

// Re-export shared types from local_store.
pub use local_store::{AtomicWriteConfig, DirStorageStrategy, FilenameEncoding, FormatStrategy};

/// Directory-based entity storage with ACID guarantees and automatic migrations.
///
/// Manages one file per entity. Raw IO (atomic rename, fsync, temp-file cleanup,
/// ID encoding/decoding, directory listing) is fully delegated to
/// `local_store::DirStorage`.
///
/// # Responsibilities
///
/// - Serialising/deserialising entities to/from the configured format.
/// - Delegating all ACID / atomic-rename / lock operations to `inner`.
/// - Applying `Migrator`-based schema evolution on load.
pub struct DirStorage {
    /// Raw ACID-safe directory store (no migration knowledge).
    inner: local_store::DirStorage,
    /// Migrator for schema evolution on save/load.
    migrator: Migrator,
    /// Strategy for format dispatch (JSON / TOML).
    strategy: local_store::DirStorageStrategy,
}

impl DirStorage {
    /// Create a new `DirStorage` instance.
    ///
    /// # Arguments
    ///
    /// * `paths` - Application paths manager.
    /// * `domain_name` - Domain-specific subdirectory name (e.g., `"sessions"`).
    /// * `migrator` - Migrator instance with registered migration paths.
    /// * `strategy` - Storage strategy (format, encoding, atomic-write config).
    ///
    /// # Behavior
    ///
    /// Delegates directory creation to `local_store::DirStorage::new`. The base
    /// path is resolved as `data_dir/domain_name` and created if absent.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::Store` wrapping a `StoreError::IoError` if
    /// directory creation fails.
    pub fn new(
        paths: AppPaths,
        domain_name: &str,
        migrator: Migrator,
        strategy: DirStorageStrategy,
    ) -> Result<Self, MigrationError> {
        let inner = local_store::DirStorage::new(paths, domain_name, strategy.clone())
            .map_err(store_err_to_migration)?;
        Ok(Self {
            inner,
            migrator,
            strategy,
        })
    }

    /// Save an entity to its file atomically.
    ///
    /// # Arguments
    ///
    /// * `entity_name` - Entity name registered in the migrator.
    /// * `id` - Unique identifier for this entity (used as the filename stem).
    /// * `entity` - Value to persist; must implement `serde::Serialize`.
    ///
    /// # Process
    ///
    /// 1. Converts `entity` to its latest versioned DTO via `migrator.save_domain_flat`.
    /// 2. Serialises to the configured format (JSON or TOML).
    /// 3. Delegates atomic write (tmp file + fsync + rename) to `inner.save_raw_string`.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if:
    /// - `entity_name` is not registered in the migrator.
    /// - `id` contains invalid characters for the configured encoding.
    /// - Serialisation or format conversion fails.
    /// - The underlying file write fails.
    pub fn save<T>(&self, entity_name: &str, id: &str, entity: T) -> Result<(), MigrationError>
    where
        T: serde::Serialize,
    {
        let json_string = self.migrator.save_domain_flat(entity_name, entity)?;

        let versioned_value: serde_json::Value = serde_json::from_str(&json_string)
            .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;

        let content = match self.strategy.format {
            FormatStrategy::Json => serde_json::to_string_pretty(&versioned_value)
                .map_err(|e| MigrationError::SerializationError(e.to_string()))?,
            FormatStrategy::Toml => {
                let tv = local_store::json_to_toml(&versioned_value).map_err(|e| {
                    MigrationError::Store(local_store::StoreError::FormatConvert(e))
                })?;
                toml::to_string_pretty(&tv)
                    .map_err(|e| MigrationError::TomlSerializeError(e.to_string()))?
            }
        };

        self.inner
            .save_raw_string(entity_name, id, &content)
            .map_err(store_err_to_migration)
    }

    /// Load an entity from its file, applying schema migrations if needed.
    ///
    /// # Arguments
    ///
    /// * `entity_name` - Entity name registered in the migrator.
    /// * `id` - Unique identifier for the entity.
    ///
    /// # Process
    ///
    /// 1. Reads raw string content via `inner.load_raw_string`.
    /// 2. Deserialises to `serde_json::Value` (converting from TOML if needed).
    /// 3. Applies schema migration via `migrator.load_flat_from`.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if the file is missing, parsing fails, or
    /// migration fails.
    pub fn load<D>(&self, entity_name: &str, id: &str) -> Result<D, MigrationError>
    where
        D: serde::de::DeserializeOwned,
    {
        let content = self
            .inner
            .load_raw_string(id)
            .map_err(store_err_to_migration)?;

        let value = match self.strategy.format {
            FormatStrategy::Json => serde_json::from_str(&content)
                .map_err(|e| MigrationError::DeserializationError(e.to_string()))?,
            FormatStrategy::Toml => {
                let tv: toml::Value = toml::from_str(&content)
                    .map_err(|e| MigrationError::TomlParseError(e.to_string()))?;
                toml_to_json(tv)?
            }
        };

        self.migrator.load_flat_from(entity_name, value)
    }

    /// List all entity IDs in the storage directory in lexicographic ascending order.
    ///
    /// # Returns
    ///
    /// A `Vec<String>` of decoded entity IDs, sorted lexicographically.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if the directory cannot be read or a filename
    /// cannot be decoded.
    pub fn list_ids(&self) -> Result<Vec<String>, MigrationError> {
        let mut ids = self.inner.list_ids().map_err(store_err_to_migration)?;
        // Guarantee lexicographic ascending order even if inner changes behaviour.
        ids.sort();
        Ok(ids)
    }

    /// Load all entities from the storage directory.
    ///
    /// # Arguments
    ///
    /// * `entity_name` - Entity name registered in the migrator.
    ///
    /// # Returns
    ///
    /// A `Vec<(id, entity)>` of all stored entities, ordered by ID.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if any entity fails to load. The whole
    /// operation fails atomically.
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

    /// Check whether an entity file exists.
    ///
    /// # Arguments
    ///
    /// * `id` - Entity identifier.
    ///
    /// # Returns
    ///
    /// `true` if the file exists; `false` otherwise.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if `id` cannot be encoded.
    pub fn exists(&self, id: &str) -> Result<bool, MigrationError> {
        self.inner.exists(id).map_err(store_err_to_migration)
    }

    /// Delete an entity file (idempotent).
    ///
    /// # Arguments
    ///
    /// * `id` - Entity identifier.
    ///
    /// # Behavior
    ///
    /// Deleting a non-existent entity is not an error (`local_store::DirStorage`
    /// guarantees idempotency).
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if the underlying file deletion fails.
    pub fn delete(&self, id: &str) -> Result<(), MigrationError> {
        self.inner.delete(id).map_err(store_err_to_migration)
    }

    /// Returns a reference to the base directory path.
    ///
    /// # Returns
    ///
    /// A reference to the resolved base directory path where entity files are stored.
    pub fn base_path(&self) -> &Path {
        self.inner.base_path()
    }
}

/// Convert a `local_store::StoreError` to `MigrationError`, promoting
/// `StoreError::FilenameEncoding` to the dedicated `MigrationError::FilenameEncoding`
/// variant.
fn store_err_to_migration(e: local_store::StoreError) -> MigrationError {
    match e {
        local_store::StoreError::FilenameEncoding { id, reason } => {
            MigrationError::FilenameEncoding { id, reason }
        }
        other => MigrationError::Store(other),
    }
}

/// Convert JSON value to TOML value.
///
/// Used by `async_impl` (AsyncDirStorage) for TOML serialisation on save.
#[cfg(feature = "async")]
fn json_to_toml(json_value: &serde_json::Value) -> Result<toml::Value, MigrationError> {
    let json_str = serde_json::to_string(json_value)
        .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
    let toml_value: toml::Value = serde_json::from_str(&json_str)
        .map_err(|e| MigrationError::TomlParseError(e.to_string()))?;
    Ok(toml_value)
}

/// Convert TOML value to JSON value.
///
/// Used by the sync `DirStorage::load` for TOML deserialisation, and also by
/// `async_impl` (AsyncDirStorage) via `use super::toml_to_json`.
fn toml_to_json(toml_value: toml::Value) -> Result<serde_json::Value, MigrationError> {
    let json_str = serde_json::to_string(&toml_value)
        .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
    let json_value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;
    Ok(json_value)
}

// ============================================================================
// Async implementation
// ============================================================================

#[cfg(feature = "async")]
pub use async_impl::AsyncDirStorage;

#[cfg(feature = "async")]
mod async_impl {
    use crate::{
        errors::{IoOperationKind, StoreError},
        AppPaths, MigrationError, Migrator,
    };
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use std::path::{Path, PathBuf};
    use tokio::io::AsyncWriteExt;

    use super::{json_to_toml, toml_to_json, DirStorageStrategy, FilenameEncoding, FormatStrategy};

    /// Async version of DirStorage for directory-based entity storage.
    ///
    /// Provides the same functionality as `DirStorage` but with async operations
    /// using `tokio::fs` for non-blocking I/O.
    pub struct AsyncDirStorage {
        /// Resolved base directory path
        base_path: PathBuf,
        /// Migrator instance for handling version migrations
        migrator: Migrator,
        /// Storage strategy configuration
        strategy: DirStorageStrategy,
    }

    impl AsyncDirStorage {
        /// Create a new AsyncDirStorage instance.
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
        /// - Creates the directory if it doesn't exist (async)
        /// - Does not load existing files (lazy loading)
        ///
        /// # Errors
        ///
        /// Returns `MigrationError::IoError` if directory creation fails.
        pub async fn new(
            paths: AppPaths,
            domain_name: &str,
            migrator: Migrator,
            strategy: DirStorageStrategy,
        ) -> Result<Self, MigrationError> {
            // Resolve base path: data_dir/domain_name
            let base_path = paths.data_dir()?.join(domain_name);

            // Create directory if it doesn't exist (async)
            if !tokio::fs::try_exists(&base_path).await.unwrap_or(false) {
                tokio::fs::create_dir_all(&base_path).await.map_err(|e| {
                    MigrationError::Store(StoreError::IoError {
                        operation: IoOperationKind::CreateDir,
                        path: base_path.display().to_string(),
                        context: Some("storage base directory (async)".to_string()),
                        error: e.to_string(),
                    })
                })?;
            }

            Ok(Self {
                base_path,
                migrator,
                strategy,
            })
        }

        /// Save an entity to a file (async).
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
        pub async fn save<T>(
            &self,
            entity_name: &str,
            id: &str,
            entity: T,
        ) -> Result<(), MigrationError>
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

            // Write atomically (async)
            self.atomic_write(&file_path, &content).await?;

            Ok(())
        }

        /// Load an entity from a file (async).
        ///
        /// # Arguments
        ///
        /// * `entity_name` - The entity name registered in the migrator
        /// * `id` - The unique identifier for the entity
        ///
        /// # Process
        ///
        /// 1. Gets the file path using `id_to_path`
        /// 2. Reads the file content to a string (async)
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
        pub async fn load<D>(&self, entity_name: &str, id: &str) -> Result<D, MigrationError>
        where
            D: serde::de::DeserializeOwned,
        {
            // Get file path
            let file_path = self.id_to_path(id)?;

            // Check if file exists (async)
            if !tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
                return Err(MigrationError::Store(StoreError::IoError {
                    operation: IoOperationKind::Read,
                    path: file_path.display().to_string(),
                    context: Some("async".to_string()),
                    error: "File not found".to_string(),
                }));
            }

            // Read file content (async)
            let content = tokio::fs::read_to_string(&file_path).await.map_err(|e| {
                MigrationError::Store(StoreError::IoError {
                    operation: IoOperationKind::Read,
                    path: file_path.display().to_string(),
                    context: Some("async".to_string()),
                    error: e.to_string(),
                })
            })?;

            // Deserialize content to JSON value
            let value = self.deserialize_content(&content)?;

            // Migrate to domain type using load_flat_from
            self.migrator.load_flat_from(entity_name, value)
        }

        /// List all entity IDs in the storage directory (async).
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
        pub async fn list_ids(&self) -> Result<Vec<String>, MigrationError> {
            // Read directory (async)
            let mut entries = tokio::fs::read_dir(&self.base_path).await.map_err(|e| {
                MigrationError::Store(StoreError::IoError {
                    operation: IoOperationKind::ReadDir,
                    path: self.base_path.display().to_string(),
                    context: Some("async".to_string()),
                    error: e.to_string(),
                })
            })?;

            let extension = self.strategy.get_extension();
            let mut ids = Vec::new();

            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                MigrationError::Store(StoreError::IoError {
                    operation: IoOperationKind::ReadDir,
                    path: self.base_path.display().to_string(),
                    context: Some("directory entry (async)".to_string()),
                    error: e.to_string(),
                })
            })? {
                let path = entry.path();

                // Check if it's a file with the correct extension
                let metadata = tokio::fs::metadata(&path).await.map_err(|e| {
                    MigrationError::Store(StoreError::IoError {
                        operation: IoOperationKind::Read,
                        path: path.display().to_string(),
                        context: Some("metadata (async)".to_string()),
                        error: e.to_string(),
                    })
                })?;

                if metadata.is_file() {
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

        /// Load all entities from the storage directory (async).
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
        pub async fn load_all<D>(
            &self,
            entity_name: &str,
        ) -> Result<Vec<(String, D)>, MigrationError>
        where
            D: serde::de::DeserializeOwned,
        {
            let ids = self.list_ids().await?;
            let mut results = Vec::new();

            for id in ids {
                let entity = self.load(entity_name, &id).await?;
                results.push((id, entity));
            }

            Ok(results)
        }

        /// Check if an entity exists (async).
        ///
        /// # Arguments
        ///
        /// * `id` - The entity ID
        ///
        /// # Returns
        ///
        /// `true` if the file exists and is a file, `false` otherwise.
        pub async fn exists(&self, id: &str) -> Result<bool, MigrationError> {
            let file_path = self.id_to_path(id)?;

            if !tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
                return Ok(false);
            }

            let metadata = tokio::fs::metadata(&file_path).await.map_err(|e| {
                MigrationError::Store(StoreError::IoError {
                    operation: IoOperationKind::Read,
                    path: file_path.display().to_string(),
                    context: Some("metadata (async)".to_string()),
                    error: e.to_string(),
                })
            })?;

            Ok(metadata.is_file())
        }

        /// Delete an entity file (async).
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
        pub async fn delete(&self, id: &str) -> Result<(), MigrationError> {
            let file_path = self.id_to_path(id)?;

            if tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
                tokio::fs::remove_file(&file_path).await.map_err(|e| {
                    MigrationError::Store(StoreError::IoError {
                        operation: IoOperationKind::Delete,
                        path: file_path.display().to_string(),
                        context: Some("async".to_string()),
                        error: e.to_string(),
                    })
                })?;
            }

            Ok(())
        }

        /// Returns a reference to the base directory path.
        ///
        /// # Returns
        ///
        /// A reference to the resolved base directory path where entities are stored.
        pub fn base_path(&self) -> &Path {
            &self.base_path
        }

        // ====================================================================
        // Private helper methods (same as sync version but async where needed)
        // ====================================================================

        /// Convert an entity ID to a file path.
        fn id_to_path(&self, id: &str) -> Result<PathBuf, MigrationError> {
            let encoded_id = self.encode_id(id)?;
            let extension = self.strategy.get_extension();
            let filename = format!("{}.{}", encoded_id, extension);
            Ok(self.base_path.join(filename))
        }

        /// Encode an entity ID to a filesystem-safe filename.
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
                    // URL-encode the ID for filesystem safety
                    Ok(urlencoding::encode(id).into_owned())
                }
                FilenameEncoding::Base64 => {
                    // Base64-encode the ID using URL-safe encoding without padding
                    Ok(URL_SAFE_NO_PAD.encode(id.as_bytes()))
                }
            }
        }

        /// Serialize a JSON value to string based on the configured format.
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

        /// Write content to a file atomically (async).
        ///
        /// Uses the "temporary file + fsync + atomic rename" pattern to ensure
        /// durability and atomicity.
        async fn atomic_write(&self, path: &Path, content: &str) -> Result<(), MigrationError> {
            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                if !tokio::fs::try_exists(parent).await.unwrap_or(false) {
                    tokio::fs::create_dir_all(parent).await.map_err(|e| {
                        MigrationError::Store(StoreError::IoError {
                            operation: IoOperationKind::CreateDir,
                            path: parent.display().to_string(),
                            context: Some("parent directory (async)".to_string()),
                            error: e.to_string(),
                        })
                    })?;
                }
            }

            // Create temporary file path
            let tmp_path = self.get_temp_path(path)?;

            // Write to temporary file (async)
            let mut tmp_file = tokio::fs::File::create(&tmp_path).await.map_err(|e| {
                MigrationError::Store(StoreError::IoError {
                    operation: IoOperationKind::Create,
                    path: tmp_path.display().to_string(),
                    context: Some("temporary file (async)".to_string()),
                    error: e.to_string(),
                })
            })?;

            tmp_file.write_all(content.as_bytes()).await.map_err(|e| {
                MigrationError::Store(StoreError::IoError {
                    operation: IoOperationKind::Write,
                    path: tmp_path.display().to_string(),
                    context: Some("temporary file (async)".to_string()),
                    error: e.to_string(),
                })
            })?;

            // Ensure data is written to disk
            tmp_file.sync_all().await.map_err(|e| {
                MigrationError::Store(StoreError::IoError {
                    operation: IoOperationKind::Sync,
                    path: tmp_path.display().to_string(),
                    context: Some("temporary file (async)".to_string()),
                    error: e.to_string(),
                })
            })?;

            drop(tmp_file);

            // Atomic rename with retry (async)
            self.atomic_rename(&tmp_path, path).await?;

            // Cleanup old temp files (best effort)
            if self.strategy.atomic_write.cleanup_tmp_files {
                let _ = self.cleanup_temp_files(path).await;
            }

            Ok(())
        }

        /// Get path to temporary file for atomic writes.
        fn get_temp_path(&self, target_path: &Path) -> Result<PathBuf, MigrationError> {
            let parent = target_path.parent().ok_or_else(|| {
                MigrationError::PathResolution("Path has no parent directory".to_string())
            })?;

            let file_name = target_path.file_name().ok_or_else(|| {
                MigrationError::PathResolution("Path has no file name".to_string())
            })?;

            let tmp_name = format!(
                ".{}.tmp.{}",
                file_name.to_string_lossy(),
                std::process::id()
            );
            Ok(parent.join(tmp_name))
        }

        /// Atomically rename temporary file to target path with retry (async).
        async fn atomic_rename(
            &self,
            tmp_path: &Path,
            target_path: &Path,
        ) -> Result<(), MigrationError> {
            let mut last_error = None;

            for attempt in 0..self.strategy.atomic_write.retry_count {
                match tokio::fs::rename(tmp_path, target_path).await {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        last_error = Some(e);
                        if attempt + 1 < self.strategy.atomic_write.retry_count {
                            // Small delay before retry
                            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                        }
                    }
                }
            }

            Err(MigrationError::Store(StoreError::IoError {
                operation: IoOperationKind::Rename,
                path: target_path.display().to_string(),
                context: Some(format!(
                    "after {} retries (async)",
                    self.strategy.atomic_write.retry_count
                )),
                error: last_error
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "unknown error after retries".to_string()),
            }))
        }

        /// Clean up old temporary files (best effort, async).
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
                        // Try to remove, but ignore errors (best effort)
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }

            Ok(())
        }

        /// Deserialize file content to a JSON value.
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
        fn decode_id(&self, filename_stem: &str) -> Result<String, MigrationError> {
            match self.strategy.filename_encoding {
                FilenameEncoding::Direct => {
                    // Direct encoding: filename is the ID
                    Ok(filename_stem.to_string())
                }
                FilenameEncoding::UrlEncode => {
                    // URL-decode the filename to get the original ID
                    urlencoding::decode(filename_stem)
                        .map(|s| s.into_owned())
                        .map_err(|e| MigrationError::FilenameEncoding {
                            id: filename_stem.to_string(),
                            reason: format!("Failed to URL-decode filename: {}", e),
                        })
                }
                FilenameEncoding::Base64 => {
                    // Base64-decode the filename using URL-safe encoding without padding
                    URL_SAFE_NO_PAD
                        .decode(filename_stem.as_bytes())
                        .map_err(|e| MigrationError::FilenameEncoding {
                            id: filename_stem.to_string(),
                            reason: format!("Failed to Base64-decode filename: {}", e),
                        })
                        .and_then(|bytes| {
                            String::from_utf8(bytes).map_err(|e| MigrationError::FilenameEncoding {
                                id: filename_stem.to_string(),
                                reason: format!(
                                    "Failed to convert Base64-decoded bytes to UTF-8: {}",
                                    e
                                ),
                            })
                        })
                }
            }
        }
    }

    // Async tests
    #[cfg(all(test, feature = "async"))]
    mod async_tests {
        use super::*;
        use crate::{FromDomain, IntoDomain, MigratesTo, Versioned};
        use serde::{Deserialize, Serialize};
        use tempfile::TempDir;

        // Test entity types (reused from sync tests)
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

        #[tokio::test]
        async fn test_async_dir_storage_new_creates_directory() {
            let temp_dir = TempDir::new().unwrap();
            let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
                temp_dir.path().to_path_buf(),
            ));

            let migrator = Migrator::new();
            let strategy = DirStorageStrategy::default();

            let storage = AsyncDirStorage::new(paths, "sessions", migrator, strategy)
                .await
                .unwrap();

            // Verify directory was created
            assert!(storage.base_path.exists());
            assert!(storage.base_path.is_dir());
            assert!(storage.base_path.ends_with("data/testapp/sessions"));
        }

        #[tokio::test]
        async fn test_async_dir_storage_save_and_load_roundtrip() {
            let temp_dir = TempDir::new().unwrap();
            let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
                temp_dir.path().to_path_buf(),
            ));

            let migrator = setup_session_migrator();
            let strategy = DirStorageStrategy::default();
            let storage = AsyncDirStorage::new(paths, "sessions", migrator, strategy)
                .await
                .unwrap();

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
                    .await
                    .unwrap();
            }

            // Load and verify each session
            for session in &sessions {
                let loaded: SessionEntity = storage.load("session", &session.id).await.unwrap();
                assert_eq!(loaded.id, session.id);
                assert_eq!(loaded.user_id, session.user_id);
                assert_eq!(loaded.created_at, session.created_at);
            }
        }

        #[tokio::test]
        async fn test_async_dir_storage_list_ids() {
            let temp_dir = TempDir::new().unwrap();
            let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
                temp_dir.path().to_path_buf(),
            ));

            let migrator = setup_session_migrator();
            let strategy = DirStorageStrategy::default();
            let storage = AsyncDirStorage::new(paths, "sessions", migrator, strategy)
                .await
                .unwrap();

            // Save multiple sessions
            let ids = vec!["session-c", "session-a", "session-b"];
            for id in &ids {
                let session = SessionEntity {
                    id: id.to_string(),
                    user_id: "user".to_string(),
                    created_at: None,
                };
                storage.save("session", id, session).await.unwrap();
            }

            // List IDs
            let listed_ids = storage.list_ids().await.unwrap();
            assert_eq!(listed_ids.len(), 3);
            // Should be sorted
            assert_eq!(listed_ids, vec!["session-a", "session-b", "session-c"]);
        }

        #[tokio::test]
        async fn test_async_dir_storage_load_all() {
            let temp_dir = TempDir::new().unwrap();
            let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
                temp_dir.path().to_path_buf(),
            ));

            let migrator = setup_session_migrator();
            let strategy = DirStorageStrategy::default();
            let storage = AsyncDirStorage::new(paths, "sessions", migrator, strategy)
                .await
                .unwrap();

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
                    .await
                    .unwrap();
            }

            // Load all
            let results: Vec<(String, SessionEntity)> = storage.load_all("session").await.unwrap();
            assert_eq!(results.len(), 3);

            // Verify all sessions are loaded
            for (id, loaded) in &results {
                let original = sessions.iter().find(|s| &s.id == id).unwrap();
                assert_eq!(loaded.id, original.id);
                assert_eq!(loaded.user_id, original.user_id);
                assert_eq!(loaded.created_at, original.created_at);
            }
        }

        #[tokio::test]
        async fn test_async_dir_storage_delete() {
            let temp_dir = TempDir::new().unwrap();
            let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
                temp_dir.path().to_path_buf(),
            ));

            let migrator = setup_session_migrator();
            let strategy = DirStorageStrategy::default();
            let storage = AsyncDirStorage::new(paths, "sessions", migrator, strategy)
                .await
                .unwrap();

            // Save a session
            let session = SessionEntity {
                id: "session-delete".to_string(),
                user_id: "user-delete".to_string(),
                created_at: None,
            };
            storage
                .save("session", "session-delete", session)
                .await
                .unwrap();

            // Verify it exists
            assert!(storage.exists("session-delete").await.unwrap());

            // Delete it
            storage.delete("session-delete").await.unwrap();

            // Verify it doesn't exist
            assert!(!storage.exists("session-delete").await.unwrap());
        }

        #[tokio::test]
        async fn test_async_dir_storage_filename_encoding_url_roundtrip() {
            let temp_dir = TempDir::new().unwrap();
            let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
                temp_dir.path().to_path_buf(),
            ));

            let migrator = setup_session_migrator();
            let strategy =
                DirStorageStrategy::default().with_filename_encoding(FilenameEncoding::UrlEncode);
            let storage = AsyncDirStorage::new(paths, "sessions", migrator, strategy)
                .await
                .unwrap();

            // Use an ID with special characters that need URL encoding
            let complex_id = "user@example.com/path?query=1";
            let session = SessionEntity {
                id: complex_id.to_string(),
                user_id: "user-special".to_string(),
                created_at: Some("2024-05-01".to_string()),
            };

            // Save the entity
            storage
                .save("session", complex_id, session.clone())
                .await
                .unwrap();

            // Verify the file was created with encoded filename
            let encoded_id = urlencoding::encode(complex_id);
            let file_path = storage.base_path.join(format!("{}.json", encoded_id));
            assert!(file_path.exists());

            // Load it back
            let loaded: SessionEntity = storage.load("session", complex_id).await.unwrap();
            assert_eq!(loaded.id, session.id);
            assert_eq!(loaded.user_id, session.user_id);
            assert_eq!(loaded.created_at, session.created_at);

            // Verify list_ids works correctly
            let ids = storage.list_ids().await.unwrap();
            assert_eq!(ids.len(), 1);
            assert_eq!(ids[0], complex_id);
        }

        #[tokio::test]
        async fn test_async_dir_storage_filename_encoding_base64_roundtrip() {
            let temp_dir = TempDir::new().unwrap();
            let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
                temp_dir.path().to_path_buf(),
            ));

            let migrator = setup_session_migrator();
            let strategy =
                DirStorageStrategy::default().with_filename_encoding(FilenameEncoding::Base64);
            let storage = AsyncDirStorage::new(paths, "sessions", migrator, strategy)
                .await
                .unwrap();

            // Use a complex ID with various special characters
            let complex_id = "user@example.com/path?query=1&special=!@#$%";
            let session = SessionEntity {
                id: complex_id.to_string(),
                user_id: "user-base64".to_string(),
                created_at: Some("2024-06-01".to_string()),
            };

            // Save the entity
            storage
                .save("session", complex_id, session.clone())
                .await
                .unwrap();

            // Verify the file was created with Base64-encoded filename
            let encoded_id = URL_SAFE_NO_PAD.encode(complex_id.as_bytes());
            let file_path = storage.base_path.join(format!("{}.json", encoded_id));
            assert!(file_path.exists());

            // Load it back
            let loaded: SessionEntity = storage.load("session", complex_id).await.unwrap();
            assert_eq!(loaded.id, session.id);
            assert_eq!(loaded.user_id, session.user_id);
            assert_eq!(loaded.created_at, session.created_at);

            // Verify list_ids works correctly
            let ids = storage.list_ids().await.unwrap();
            assert_eq!(ids.len(), 1);
            assert_eq!(ids[0], complex_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use local_store::StoreError;
    use std::fs;
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
        assert!(storage.base_path().exists());
        assert!(storage.base_path().is_dir());
        assert!(storage.base_path().ends_with("data/testapp/sessions"));
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
        assert_eq!(storage1.base_path(), storage2.base_path());
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
        let file_path = storage.base_path().join("session-123.json");
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
        let file_path = storage.base_path().join("session-789.toml");
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
        let file_path = storage.base_path().join("session-custom.data");
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
        let file_path = storage.base_path().join("session-overwrite.json");
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
            MigrationError::Store(StoreError::IoError { .. })
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
        let corrupted_path = storage.base_path().join("session-corrupted.json");
        std::fs::write(&corrupted_path, "invalid json {{{").unwrap();

        // load_all should fail
        let result: Result<Vec<(String, SessionEntity)>, _> = storage.load_all("session");
        assert!(result.is_err());
    }

    #[test]
    fn test_dir_storage_filename_encoding_url_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy =
            DirStorageStrategy::default().with_filename_encoding(FilenameEncoding::UrlEncode);
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Use an ID with special characters that need URL encoding
        let complex_id = "user@example.com/path?query=1";
        let session = SessionEntity {
            id: complex_id.to_string(),
            user_id: "user-special".to_string(),
            created_at: Some("2024-05-01".to_string()),
        };

        // Save the entity
        storage
            .save("session", complex_id, session.clone())
            .unwrap();

        // Verify the file was created with encoded filename
        let encoded_id = urlencoding::encode(complex_id);
        let file_path = storage.base_path().join(format!("{}.json", encoded_id));
        assert!(file_path.exists());

        // Load it back
        let loaded: SessionEntity = storage.load("session", complex_id).unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.user_id, session.user_id);
        assert_eq!(loaded.created_at, session.created_at);

        // Verify list_ids works correctly
        let ids = storage.list_ids().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], complex_id);
    }

    #[test]
    fn test_dir_storage_filename_encoding_base64_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy =
            DirStorageStrategy::default().with_filename_encoding(FilenameEncoding::Base64);
        let storage = DirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        // Use a complex ID with various special characters
        let complex_id = "user@example.com/path?query=1&special=!@#$%";
        let session = SessionEntity {
            id: complex_id.to_string(),
            user_id: "user-base64".to_string(),
            created_at: Some("2024-06-01".to_string()),
        };

        // Save the entity
        storage
            .save("session", complex_id, session.clone())
            .unwrap();

        // Verify the file was created with Base64-encoded filename
        let encoded_id = URL_SAFE_NO_PAD.encode(complex_id.as_bytes());
        let file_path = storage.base_path().join(format!("{}.json", encoded_id));
        assert!(file_path.exists());

        // Load it back
        let loaded: SessionEntity = storage.load("session", complex_id).unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.user_id, session.user_id);
        assert_eq!(loaded.created_at, session.created_at);

        // Verify list_ids works correctly
        let ids = storage.list_ids().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], complex_id);
    }

    /// Test that list_ids returns a FilenameEncoding error when a file with an
    /// undecoded URL-encoded name (invalid UTF-8 percent sequence) is present in the
    /// storage directory.
    #[test]
    fn test_list_ids_url_decode_error() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy =
            DirStorageStrategy::default().with_filename_encoding(FilenameEncoding::UrlEncode);
        let storage = DirStorage::new(paths, "sessions_url_err", migrator, strategy).unwrap();

        // Manually create a file whose stem is an invalid UTF-8 percent sequence.
        // `%C0%C1` is an overlong encoding that the urlencoding crate rejects.
        let bad_stem = "%C0%C1";
        let bad_file = storage.base_path().join(format!("{}.json", bad_stem));
        std::fs::write(&bad_file, "{}").unwrap();

        let result = storage.list_ids();
        assert!(
            result.is_err(),
            "list_ids should propagate the FilenameEncoding decode error"
        );
        assert!(matches!(
            result.unwrap_err(),
            MigrationError::FilenameEncoding { .. }
        ));
    }

    /// Test that list_ids returns a FilenameEncoding error when a file with an
    /// invalid Base64-encoded name is present in the storage directory.
    #[test]
    fn test_list_ids_base64_decode_error() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy =
            DirStorageStrategy::default().with_filename_encoding(FilenameEncoding::Base64);
        let storage = DirStorage::new(paths, "sessions_b64_err", migrator, strategy).unwrap();

        // Manually create a file whose stem is not valid Base64.
        let bad_stem = "!!!invalid@@@";
        let bad_file = storage.base_path().join(format!("{}.json", bad_stem));
        std::fs::write(&bad_file, "{}").unwrap();

        let result = storage.list_ids();
        assert!(
            result.is_err(),
            "list_ids should propagate the FilenameEncoding decode error"
        );
        assert!(matches!(
            result.unwrap_err(),
            MigrationError::FilenameEncoding { .. }
        ));
    }

    #[test]
    fn test_dir_storage_base_path() {
        let temp_dir = TempDir::new().unwrap();
        let domain_name = "test_sessions";
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = Migrator::new();
        let strategy = DirStorageStrategy::default();

        let storage = DirStorage::new(paths, domain_name, migrator, strategy).unwrap();

        // Verify base_path() returns the expected path
        let returned_path = storage.base_path();
        assert!(returned_path.ends_with(domain_name));
        assert!(returned_path.exists());
    }

    #[test]
    #[cfg(unix)]
    fn test_dir_storage_create_dir_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();

        // Make the temp dir read-only
        let mut perms = fs::metadata(temp_dir.path()).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(temp_dir.path(), perms).unwrap();

        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = Migrator::new();
        let strategy = DirStorageStrategy::default();

        // Should fail because we can't create directory in read-only parent
        let result = DirStorage::new(paths, "sessions", migrator, strategy);

        // Restore permissions before assertion (so TempDir cleanup works)
        let mut perms = fs::metadata(temp_dir.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(temp_dir.path(), perms).unwrap();

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(MigrationError::Store(StoreError::IoError { .. }))
        ));
    }

    #[test]
    #[cfg(unix)]
    fn test_dir_storage_save_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let domain_name = "sessions_readonly";
        let paths = AppPaths::new("testapp").data_strategy(crate::PathStrategy::CustomBase(
            temp_dir.path().to_path_buf(),
        ));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();

        // Create storage first (creates directory)
        let storage = DirStorage::new(paths, domain_name, migrator, strategy).unwrap();

        // Make the storage directory read-only
        let mut perms = fs::metadata(storage.base_path()).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(storage.base_path(), perms).unwrap();

        let session = SessionEntity {
            id: "test".to_string(),
            user_id: "user".to_string(),
            created_at: None,
        };

        // Should fail because directory is read-only
        let result = storage.save("session", "test-session", session);

        // Restore permissions before assertion
        let mut perms = fs::metadata(storage.base_path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(storage.base_path(), perms).unwrap();

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(MigrationError::Store(StoreError::IoError { .. }))
        ));
    }
}
