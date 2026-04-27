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

/// Convert TOML value to JSON value.
///
/// Used by the sync `DirStorage::load` for TOML deserialisation.
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
    use crate::{AppPaths, MigrationError, Migrator};
    use std::path::Path;

    use super::{store_err_to_migration, toml_to_json, DirStorageStrategy, FormatStrategy};

    /// Async version of DirStorage for directory-based entity storage.
    ///
    /// Wraps `local_store::AsyncDirStorage` for raw async IO and layers
    /// `Migrator`-based schema evolution on top. All atomic IO (atomic rename,
    /// fsync, retry) is fully delegated to `inner`; no `tokio::fs::*` calls
    /// remain in this struct.
    pub struct AsyncDirStorage {
        /// Raw ACID-safe async directory store (no migration knowledge).
        inner: local_store::AsyncDirStorage,
        /// Migrator for schema evolution on save/load.
        migrator: Migrator,
        /// Strategy for format dispatch (JSON / TOML).
        strategy: DirStorageStrategy,
    }

    impl AsyncDirStorage {
        /// Create a new `AsyncDirStorage` instance (async).
        ///
        /// Resolves the base path as `paths.data_dir()?.join(domain_name)`, creates
        /// the directory when absent, and wraps the raw
        /// `local_store::AsyncDirStorage`.
        ///
        /// # Errors
        ///
        /// Returns `MigrationError::Store` if directory creation fails.
        pub async fn new(
            paths: AppPaths,
            domain_name: &str,
            migrator: Migrator,
            strategy: DirStorageStrategy,
        ) -> Result<Self, MigrationError> {
            let inner = local_store::AsyncDirStorage::new(paths, domain_name, strategy.clone())
                .await
                .map_err(store_err_to_migration)?;
            Ok(Self {
                inner,
                migrator,
                strategy,
            })
        }

        /// Save an entity to a file (async).
        ///
        /// Converts the entity to its latest versioned DTO, applies format
        /// serialisation, and delegates the atomic write to
        /// `local_store::AsyncDirStorage::save_raw_string`.
        ///
        /// # Errors
        ///
        /// Returns `MigrationError` on serialisation failure, invalid ID, or IO errors.
        pub async fn save<T>(
            &self,
            entity_name: &str,
            id: &str,
            entity: T,
        ) -> Result<(), MigrationError>
        where
            T: serde::Serialize,
        {
            let json_string = self.migrator.save_domain_flat(entity_name, entity)?;

            let versioned_value: serde_json::Value = serde_json::from_str(&json_string)
                .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;

            let content = self.serialize_content(&versioned_value)?;

            self.inner
                .save_raw_string(entity_name, id, &content)
                .await
                .map_err(store_err_to_migration)
        }

        /// Load an entity from a file (async).
        ///
        /// Reads the raw string, deserialises to `serde_json::Value`, and migrates
        /// to the target domain type.
        ///
        /// # Errors
        ///
        /// Returns `MigrationError` if the file is not found, deserialisation fails,
        /// or migration fails.
        pub async fn load<D>(&self, entity_name: &str, id: &str) -> Result<D, MigrationError>
        where
            D: serde::de::DeserializeOwned,
        {
            let content = self
                .inner
                .load_raw_string(id)
                .await
                .map_err(store_err_to_migration)?;

            let value = self.deserialize_content(&content)?;
            self.migrator.load_flat_from(entity_name, value)
        }

        /// List all entity IDs in the storage directory in lexicographic ascending order (async).
        ///
        /// # Errors
        ///
        /// Returns `MigrationError::Store` if the directory read fails.
        pub async fn list_ids(&self) -> Result<Vec<String>, MigrationError> {
            let mut ids = self
                .inner
                .list_ids()
                .await
                .map_err(store_err_to_migration)?;
            ids.sort();
            Ok(ids)
        }

        /// Load all entities from the storage directory (async).
        ///
        /// # Errors
        ///
        /// Returns the first `MigrationError` encountered during loading.
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

        /// Check whether an entity file exists (async).
        ///
        /// # Errors
        ///
        /// Returns `MigrationError::Store` on ID encoding failure or IO error.
        pub async fn exists(&self, id: &str) -> Result<bool, MigrationError> {
            self.inner.exists(id).await.map_err(store_err_to_migration)
        }

        /// Delete an entity file (async).
        ///
        /// This operation is idempotent: deleting a non-existent file is not an error.
        ///
        /// # Errors
        ///
        /// Returns `MigrationError::Store` if file deletion fails.
        pub async fn delete(&self, id: &str) -> Result<(), MigrationError> {
            self.inner.delete(id).await.map_err(store_err_to_migration)
        }

        /// Returns a reference to the base directory path.
        pub fn base_path(&self) -> &Path {
            self.inner.base_path()
        }

        // ================================================================
        // Private format helpers
        // ================================================================

        fn serialize_content(&self, value: &serde_json::Value) -> Result<String, MigrationError> {
            match self.strategy.format {
                FormatStrategy::Json => serde_json::to_string_pretty(value)
                    .map_err(|e| MigrationError::SerializationError(e.to_string())),
                FormatStrategy::Toml => {
                    let tv = local_store::format_convert::json_to_toml(value).map_err(|e| {
                        MigrationError::Store(local_store::StoreError::FormatConvert(e))
                    })?;
                    toml::to_string_pretty(&tv)
                        .map_err(|e| MigrationError::TomlSerializeError(e.to_string()))
                }
            }
        }

        fn deserialize_content(&self, content: &str) -> Result<serde_json::Value, MigrationError> {
            match self.strategy.format {
                FormatStrategy::Json => serde_json::from_str(content)
                    .map_err(|e| MigrationError::DeserializationError(e.to_string())),
                FormatStrategy::Toml => {
                    let tv: toml::Value = toml::from_str(content)
                        .map_err(|e| MigrationError::TomlParseError(e.to_string()))?;
                    toml_to_json(tv)
                }
            }
        }
    }

    // Async tests
    #[cfg(all(test, feature = "async"))]
    mod async_tests {
        use super::*;
        use crate::{FromDomain, IntoDomain, MigratesTo, Versioned};
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
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
            assert!(storage.base_path().exists());
            assert!(storage.base_path().is_dir());
            assert!(storage.base_path().ends_with("data/testapp/sessions"));
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
            let file_path = storage.base_path().join(format!("{}.json", encoded_id));
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
            let file_path = storage.base_path().join(format!("{}.json", encoded_id));
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
