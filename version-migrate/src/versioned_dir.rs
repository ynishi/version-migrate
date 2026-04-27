//! Version-aware directory storage wrappers.
//!
//! Provides `VersionedDirStorage` (sync) and, when the `async` feature is
//! enabled, `VersionedAsyncDirStorage`.  Both wrap their counterparts in
//! `local_store` for raw ACID file operations and layer `Migrator`-based
//! schema evolution on top.

use crate::{AppPaths, MigrationError, Migrator};
use local_store::{DirStorageStrategy, FormatStrategy};
use std::path::Path;

// ============================================================================
// VersionedDirStorage (sync)
// ============================================================================

/// Version-aware directory-based entity storage.
///
/// Wraps `local_store::DirStorage` for raw IO and layers `Migrator`-based
/// schema evolution on top.
///
/// # Responsibilities
///
/// - Serialising/deserialising entities to/from the configured format.
/// - Delegating all ACID / atomic-rename / lock operations to `inner`.
/// - Applying migrator-based schema evolution on load.
///
/// Raw IO (`atomic_rename`, `get_temp_path`, `cleanup_temp_files`) lives
/// exclusively inside `local_store::DirStorage`.
pub struct VersionedDirStorage {
    /// Raw ACID-safe directory store (no migration knowledge).
    inner: local_store::DirStorage,
    /// Migrator for schema evolution on save/load.
    migrator: Migrator,
    /// Strategy for format dispatch (JSON / TOML).
    strategy: DirStorageStrategy,
}

impl VersionedDirStorage {
    /// Create a new `VersionedDirStorage` instance.
    ///
    /// Resolves the base path as `paths.data_dir()?.join(category)`, creates
    /// the directory when absent, and wraps the raw `local_store::DirStorage`.
    ///
    /// # Arguments
    ///
    /// * `paths` - Application paths manager.
    /// * `category` - Domain-specific subdirectory name (e.g. `"sessions"`).
    /// * `migrator` - `Migrator` instance with registered migration paths.
    /// * `strategy` - Storage strategy configuration (format, encoding, etc.).
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::Store` if directory creation fails.
    pub fn new(
        paths: AppPaths,
        category: impl Into<String>,
        migrator: Migrator,
        strategy: DirStorageStrategy,
    ) -> Result<Self, MigrationError> {
        let inner = local_store::DirStorage::new(paths, category, strategy.clone())
            .map_err(MigrationError::Store)?;
        Ok(Self {
            inner,
            migrator,
            strategy,
        })
    }

    /// Save an entity to a file.
    ///
    /// Converts the entity to its latest versioned DTO via `Migrator::save_domain_flat`,
    /// applies format serialisation, and writes atomically through
    /// `local_store::DirStorage::save_raw_string`.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` on serialisation failure, invalid ID characters,
    /// or IO errors.
    pub fn save<T>(
        &self,
        entity_name: impl Into<String>,
        id: impl Into<String>,
        entity: T,
    ) -> Result<(), MigrationError>
    where
        T: serde::Serialize,
    {
        let entity_name = entity_name.into();
        let id = id.into();

        let json_string = self.migrator.save_domain_flat(&entity_name, entity)?;

        let versioned_value: serde_json::Value = serde_json::from_str(&json_string)
            .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;

        let content = self.serialize_content(&versioned_value)?;

        self.inner
            .save_raw_string(&entity_name, &id, &content)
            .map_err(store_err_to_migration)
    }

    /// Load an entity from a file.
    ///
    /// Reads the raw string from `local_store::DirStorage::load_raw_string`,
    /// deserialises the content to a `serde_json::Value`, and migrates to the
    /// target domain type via `Migrator::load_flat_from`.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError` if the file is not found, deserialisation fails,
    /// or migration fails.
    pub fn load<D>(
        &self,
        entity_name: impl Into<String>,
        id: impl Into<String>,
    ) -> Result<D, MigrationError>
    where
        D: serde::de::DeserializeOwned,
    {
        let entity_name = entity_name.into();
        let id = id.into();

        let content = self
            .inner
            .load_raw_string(&id)
            .map_err(store_err_to_migration)?;

        let value = self.deserialize_content(&content)?;
        self.migrator.load_flat_from(&entity_name, value)
    }

    /// List all entity IDs in the storage directory.
    ///
    /// Delegates directly to `local_store::DirStorage::list_ids`.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::Store` on IO failure or
    /// `MigrationError::FilenameEncoding` when a stored filename cannot be decoded.
    pub fn list_ids(&self) -> Result<Vec<String>, MigrationError> {
        self.inner.list_ids().map_err(store_err_to_migration)
    }

    /// Load all entities from the storage directory.
    ///
    /// Calls `list_ids` and then `load` for each ID.  If any load fails the
    /// entire operation fails.
    ///
    /// # Errors
    ///
    /// Returns the first `MigrationError` encountered during loading.
    pub fn load_all<D>(
        &self,
        entity_name: impl Into<String>,
    ) -> Result<Vec<(String, D)>, MigrationError>
    where
        D: serde::de::DeserializeOwned,
    {
        let entity_name = entity_name.into();
        let ids = self.list_ids()?;
        let mut results = Vec::new();
        for id in ids {
            let entity = self.load(&entity_name, &id)?;
            results.push((id, entity));
        }
        Ok(results)
    }

    /// Check whether an entity file exists.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::Store` if ID encoding fails.
    pub fn exists(&self, id: impl Into<String>) -> Result<bool, MigrationError> {
        self.inner.exists(id).map_err(store_err_to_migration)
    }

    /// Delete an entity file.
    ///
    /// This operation is idempotent: deleting a non-existent file is not an error.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::Store` if file deletion fails.
    pub fn delete(&self, id: impl Into<String>) -> Result<(), MigrationError> {
        self.inner.delete(id).map_err(store_err_to_migration)
    }

    /// Returns a reference to the base directory path.
    pub fn base_path(&self) -> &Path {
        self.inner.base_path()
    }

    // ====================================================================
    // Private format helpers
    // ====================================================================

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

// ============================================================================
// Format conversion helpers (private, shared with async_impl)
// ============================================================================

fn toml_to_json(toml_value: toml::Value) -> Result<serde_json::Value, MigrationError> {
    let json_str = serde_json::to_string(&toml_value)
        .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
    let json_value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;
    Ok(json_value)
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

// ============================================================================
// VersionedAsyncDirStorage
// ============================================================================

#[cfg(feature = "async")]
pub use async_impl::VersionedAsyncDirStorage;

#[cfg(feature = "async")]
mod async_impl {
    use crate::{AppPaths, MigrationError, Migrator};
    use local_store::DirStorageStrategy;
    use std::path::Path;

    use super::{store_err_to_migration, toml_to_json, FormatStrategy};

    /// Async version of `VersionedDirStorage`.
    ///
    /// Wraps `local_store::AsyncDirStorage` for raw async IO and layers
    /// `Migrator`-based schema evolution on top.
    ///
    /// # Responsibilities
    ///
    /// - Serialising/deserialising entities to/from the configured format.
    /// - Delegating all ACID / atomic-rename / lock operations to `inner`.
    /// - Applying migrator-based schema evolution on load.
    pub struct VersionedAsyncDirStorage {
        /// Raw ACID-safe async directory store (no migration knowledge).
        inner: local_store::AsyncDirStorage,
        /// Migrator for schema evolution on save/load.
        migrator: Migrator,
        /// Strategy for format dispatch (JSON / TOML).
        strategy: DirStorageStrategy,
    }

    impl VersionedAsyncDirStorage {
        /// Create a new `VersionedAsyncDirStorage` instance (async).
        ///
        /// Resolves the base path as `paths.data_dir()?.join(category)`, creates
        /// the directory when absent, and wraps the raw
        /// `local_store::AsyncDirStorage`.
        ///
        /// # Errors
        ///
        /// Returns `MigrationError::Store` if directory creation fails.
        pub async fn new(
            paths: AppPaths,
            category: impl Into<String>,
            migrator: Migrator,
            strategy: DirStorageStrategy,
        ) -> Result<Self, MigrationError> {
            let inner = local_store::AsyncDirStorage::new(paths, category, strategy.clone())
                .await
                .map_err(MigrationError::Store)?;
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
            entity_name: impl Into<String>,
            id: impl Into<String>,
            entity: T,
        ) -> Result<(), MigrationError>
        where
            T: serde::Serialize,
        {
            let entity_name = entity_name.into();
            let id = id.into();

            let json_string = self.migrator.save_domain_flat(&entity_name, entity)?;

            let versioned_value: serde_json::Value = serde_json::from_str(&json_string)
                .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;

            let content = self.serialize_content(&versioned_value)?;

            self.inner
                .save_raw_string(&entity_name, &id, &content)
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
        pub async fn load<D>(
            &self,
            entity_name: impl Into<String>,
            id: impl Into<String>,
        ) -> Result<D, MigrationError>
        where
            D: serde::de::DeserializeOwned,
        {
            let entity_name = entity_name.into();
            let id = id.into();

            let content = self
                .inner
                .load_raw_string(&id)
                .await
                .map_err(store_err_to_migration)?;

            let value = self.deserialize_content(&content)?;
            self.migrator.load_flat_from(&entity_name, value)
        }

        /// List all entity IDs in the storage directory (async).
        ///
        /// # Errors
        ///
        /// Returns `MigrationError::Store` if the directory read fails.
        pub async fn list_ids(&self) -> Result<Vec<String>, MigrationError> {
            self.inner.list_ids().await.map_err(store_err_to_migration)
        }

        /// Load all entities from the storage directory (async).
        ///
        /// # Errors
        ///
        /// Returns the first `MigrationError` encountered during loading.
        pub async fn load_all<D>(
            &self,
            entity_name: impl Into<String>,
        ) -> Result<Vec<(String, D)>, MigrationError>
        where
            D: serde::de::DeserializeOwned,
        {
            let entity_name = entity_name.into();
            let ids = self.list_ids().await?;
            let mut results = Vec::new();
            for id in ids {
                let entity = self.load(&entity_name, &id).await?;
                results.push((id, entity));
            }
            Ok(results)
        }

        /// Check whether an entity file exists (async).
        ///
        /// # Errors
        ///
        /// Returns `MigrationError::Store` on ID encoding failure or IO error.
        pub async fn exists(&self, id: impl Into<String>) -> Result<bool, MigrationError> {
            self.inner.exists(id).await.map_err(store_err_to_migration)
        }

        /// Delete an entity file (async).
        ///
        /// This operation is idempotent: deleting a non-existent file is not an error.
        ///
        /// # Errors
        ///
        /// Returns `MigrationError::Store` if file deletion fails.
        pub async fn delete(&self, id: impl Into<String>) -> Result<(), MigrationError> {
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
}

// ============================================================================
// Sync tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppPaths, FromDomain, IntoDomain, MigratesTo, PathStrategy, Versioned};
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

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
        // SAFETY: register only fails on circular paths or duplicate entity names.
        migrator.register(path).unwrap();
        migrator
    }

    #[test]
    fn test_versioned_dir_storage_new_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp")
            .data_strategy(PathStrategy::CustomBase(temp_dir.path().to_path_buf()));

        let migrator = Migrator::new();
        let strategy = DirStorageStrategy::default();

        let storage = VersionedDirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        assert!(storage.base_path().exists());
        assert!(storage.base_path().is_dir());
        assert!(storage.base_path().ends_with("data/testapp/sessions"));
    }

    #[test]
    fn test_versioned_dir_storage_category_into_string() {
        // Verify that category accepts impl Into<String> (both &str and String)
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp")
            .data_strategy(PathStrategy::CustomBase(temp_dir.path().to_path_buf()));

        let migrator = Migrator::new();
        let strategy = DirStorageStrategy::default();

        // &str
        let result = VersionedDirStorage::new(paths.clone(), "sessions", migrator, strategy);
        assert!(result.is_ok());

        let migrator2 = Migrator::new();
        let strategy2 = DirStorageStrategy::default();
        // String
        let result2 =
            VersionedDirStorage::new(paths, String::from("sessions2"), migrator2, strategy2);
        assert!(result2.is_ok());
    }

    #[test]
    fn test_versioned_dir_storage_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp")
            .data_strategy(PathStrategy::CustomBase(temp_dir.path().to_path_buf()));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = VersionedDirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        let session = SessionEntity {
            id: "session-1".to_string(),
            user_id: "user-1".to_string(),
            created_at: Some("2024-01-01".to_string()),
        };

        storage
            .save("session", &session.id, session.clone())
            .unwrap();

        let loaded: SessionEntity = storage.load("session", "session-1").unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.user_id, session.user_id);
        assert_eq!(loaded.created_at, session.created_at);
    }

    #[test]
    fn test_versioned_dir_storage_list_ids() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp")
            .data_strategy(PathStrategy::CustomBase(temp_dir.path().to_path_buf()));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = VersionedDirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        let ids = ["session-c", "session-a", "session-b"];
        for id in &ids {
            let session = SessionEntity {
                id: id.to_string(),
                user_id: "user".to_string(),
                created_at: None,
            };
            storage.save("session", *id, session).unwrap();
        }

        let listed = storage.list_ids().unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed, vec!["session-a", "session-b", "session-c"]);
    }

    #[test]
    fn test_versioned_dir_storage_load_all() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp")
            .data_strategy(PathStrategy::CustomBase(temp_dir.path().to_path_buf()));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = VersionedDirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        let sessions = vec![
            SessionEntity {
                id: "s1".to_string(),
                user_id: "u1".to_string(),
                created_at: None,
            },
            SessionEntity {
                id: "s2".to_string(),
                user_id: "u2".to_string(),
                created_at: Some("2024-01-01".to_string()),
            },
        ];

        for s in &sessions {
            storage.save("session", &s.id, s.clone()).unwrap();
        }

        let results: Vec<(String, SessionEntity)> = storage.load_all("session").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_versioned_dir_storage_exists_and_delete() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("testapp")
            .data_strategy(PathStrategy::CustomBase(temp_dir.path().to_path_buf()));

        let migrator = setup_session_migrator();
        let strategy = DirStorageStrategy::default();
        let storage = VersionedDirStorage::new(paths, "sessions", migrator, strategy).unwrap();

        let session = SessionEntity {
            id: "del-session".to_string(),
            user_id: "u".to_string(),
            created_at: None,
        };
        storage.save("session", "del-session", session).unwrap();

        assert!(storage.exists("del-session").unwrap());
        storage.delete("del-session").unwrap();
        assert!(!storage.exists("del-session").unwrap());
    }

    #[test]
    fn test_versioned_dir_storage_base_path() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AppPaths::new("myapp")
            .data_strategy(PathStrategy::CustomBase(temp_dir.path().to_path_buf()));

        let migrator = Migrator::new();
        let strategy = DirStorageStrategy::default();
        let storage = VersionedDirStorage::new(paths, "entities", migrator, strategy).unwrap();

        assert!(storage.base_path().ends_with("entities"));
    }
}
