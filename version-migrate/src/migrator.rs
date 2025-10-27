//! Migration manager and builder pattern for defining type-safe migration paths.

use crate::errors::MigrationError;
use crate::{IntoDomain, MigratesTo, Versioned};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::marker::PhantomData;

type MigrationFn = Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError>>;

/// Type-erased function for saving domain entities
type DomainSaveFn =
    Box<dyn Fn(serde_json::Value, &str, &str) -> Result<String, MigrationError> + Send + Sync>;
type DomainSaveFlatFn =
    Box<dyn Fn(serde_json::Value, &str) -> Result<String, MigrationError> + Send + Sync>;

/// A registered migration path for a specific entity type.
struct EntityMigrationPath {
    /// Maps version -> migration function to next version
    steps: HashMap<String, MigrationFn>,
    /// The final conversion to domain model
    finalize:
        Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError> + Send + Sync>,
    /// Ordered list of versions in the migration path
    versions: Vec<String>,
    /// The key name for version field in serialized data
    version_key: String,
    /// The key name for data field in serialized data
    data_key: String,
}

/// Type-erased functions for saving domain entities by entity name
struct DomainSavers {
    save_fn: DomainSaveFn,
    save_flat_fn: DomainSaveFlatFn,
}

/// The migration manager that orchestrates all migrations.
pub struct Migrator {
    paths: HashMap<String, EntityMigrationPath>,
    default_version_key: Option<String>,
    default_data_key: Option<String>,
    domain_savers: HashMap<String, DomainSavers>,
}

impl Migrator {
    /// Creates a new, empty migrator.
    pub fn new() -> Self {
        Self {
            paths: HashMap::new(),
            default_version_key: None,
            default_data_key: None,
            domain_savers: HashMap::new(),
        }
    }

    /// Gets the latest version for a given entity.
    ///
    /// # Returns
    ///
    /// The latest version string if the entity is registered, `None` otherwise.
    pub fn get_latest_version(&self, entity: &str) -> Option<&str> {
        self.paths
            .get(entity)
            .and_then(|path| path.versions.last())
            .map(|v| v.as_str())
    }

    /// Creates a builder for configuring the migrator.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let migrator = Migrator::builder()
    ///     .default_version_key("schema_version")
    ///     .default_data_key("payload")
    ///     .build();
    /// ```
    pub fn builder() -> MigratorBuilder {
        MigratorBuilder::new()
    }

    /// Starts defining a migration path for an entity.
    pub fn define(entity: &str) -> MigrationPathBuilder<Start> {
        MigrationPathBuilder::new(entity.to_string())
    }

    /// Registers a migration path with validation.
    ///
    /// This method validates the migration path before registering it:
    /// - Checks for circular migration paths
    /// - Validates version ordering follows semver rules
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails.
    pub fn register<D>(&mut self, path: MigrationPath<D>) -> Result<(), MigrationError> {
        Self::validate_migration_path(&path.entity, &path.versions)?;

        // Resolve key priority: Path custom > Migrator default > EntityPath (trait constants)
        let version_key = path
            .custom_version_key
            .or_else(|| self.default_version_key.clone())
            .unwrap_or_else(|| path.inner.version_key.clone());

        let data_key = path
            .custom_data_key
            .or_else(|| self.default_data_key.clone())
            .unwrap_or_else(|| path.inner.data_key.clone());

        let entity_name = path.entity.clone();
        let final_path = EntityMigrationPath {
            steps: path.inner.steps,
            finalize: path.inner.finalize,
            versions: path.versions,
            version_key,
            data_key,
        };

        self.paths.insert(path.entity, final_path);

        // Register domain savers if available
        if let (Some(save_fn), Some(save_flat_fn)) = (path.save_fn, path.save_flat_fn) {
            self.domain_savers.insert(
                entity_name,
                DomainSavers {
                    save_fn,
                    save_flat_fn,
                },
            );
        }

        Ok(())
    }

    /// Validates a migration path for correctness.
    fn validate_migration_path(entity: &str, versions: &[String]) -> Result<(), MigrationError> {
        // Check for circular paths
        Self::check_circular_path(entity, versions)?;

        // Check version ordering
        Self::check_version_ordering(entity, versions)?;

        Ok(())
    }

    /// Checks if there are any circular dependencies in the migration path.
    fn check_circular_path(entity: &str, versions: &[String]) -> Result<(), MigrationError> {
        let mut seen = std::collections::HashSet::new();

        for version in versions {
            if !seen.insert(version) {
                // Found a duplicate - circular path detected
                let path = versions.join(" -> ");
                return Err(MigrationError::CircularMigrationPath {
                    entity: entity.to_string(),
                    path,
                });
            }
        }

        Ok(())
    }

    /// Checks if versions are ordered according to semver rules.
    fn check_version_ordering(entity: &str, versions: &[String]) -> Result<(), MigrationError> {
        for i in 0..versions.len().saturating_sub(1) {
            let current = &versions[i];
            let next = &versions[i + 1];

            // Parse versions
            let current_ver = semver::Version::parse(current).map_err(|e| {
                MigrationError::DeserializationError(format!("Invalid semver '{}': {}", current, e))
            })?;

            let next_ver = semver::Version::parse(next).map_err(|e| {
                MigrationError::DeserializationError(format!("Invalid semver '{}': {}", next, e))
            })?;

            // Check that next version is greater than current
            if next_ver <= current_ver {
                return Err(MigrationError::InvalidVersionOrder {
                    entity: entity.to_string(),
                    from: current.clone(),
                    to: next.clone(),
                });
            }
        }

        Ok(())
    }

    /// Loads and migrates data from any serde-compatible format.
    ///
    /// This is the generic version that accepts any type implementing `Serialize`.
    /// For JSON strings, use the convenience method `load` instead.
    ///
    /// # Arguments
    ///
    /// * `entity` - The entity name used when registering the migration path
    /// * `data` - Versioned data in any serde-compatible format (e.g., `toml::Value`, `serde_json::Value`)
    ///
    /// # Returns
    ///
    /// The migrated data as the domain model type
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The data cannot be converted to the internal format
    /// - The entity is not registered
    /// - A migration step fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Load from TOML
    /// let toml_data: toml::Value = toml::from_str(toml_str)?;
    /// let domain: TaskEntity = migrator.load_from("task", toml_data)?;
    ///
    /// // Load from JSON Value
    /// let json_data: serde_json::Value = serde_json::from_str(json_str)?;
    /// let domain: TaskEntity = migrator.load_from("task", json_data)?;
    /// ```
    pub fn load_from<D, T>(&self, entity: &str, data: T) -> Result<D, MigrationError>
    where
        D: DeserializeOwned,
        T: Serialize,
    {
        // Convert the input data to serde_json::Value for internal processing
        let value = serde_json::to_value(data).map_err(|e| {
            MigrationError::DeserializationError(format!(
                "Failed to convert input data to internal format: {}",
                e
            ))
        })?;

        // Get the migration path for this entity
        let path = self
            .paths
            .get(entity)
            .ok_or_else(|| MigrationError::EntityNotFound(entity.to_string()))?;

        let version_key = &path.version_key;
        let data_key = &path.data_key;

        // Extract version and data using custom keys
        let obj = value.as_object().ok_or_else(|| {
            MigrationError::DeserializationError(
                "Expected object with version and data fields".to_string(),
            )
        })?;

        let current_version = obj
            .get(version_key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                MigrationError::DeserializationError(format!(
                    "Missing or invalid '{}' field",
                    version_key
                ))
            })?
            .to_string();

        let mut current_data = obj
            .get(data_key)
            .ok_or_else(|| {
                MigrationError::DeserializationError(format!("Missing '{}' field", data_key))
            })?
            .clone();

        let mut current_version = current_version;

        // Apply migration steps until we reach a version with no further steps
        while let Some(migrate_fn) = path.steps.get(&current_version) {
            // Migration function returns raw value, no wrapping
            current_data = migrate_fn(current_data.clone())?;

            // Update version to the next step
            // Find the next version in the path
            match path.versions.iter().position(|v| v == &current_version) {
                Some(idx) if idx + 1 < path.versions.len() => {
                    current_version = path.versions[idx + 1].clone();
                }
                _ => break,
            }
        }

        // Finalize into domain model
        let domain_value = (path.finalize)(current_data)?;

        serde_json::from_value(domain_value).map_err(|e| {
            MigrationError::DeserializationError(format!("Failed to convert to domain: {}", e))
        })
    }

    /// Loads and migrates data from a JSON string.
    ///
    /// This is a convenience method for the common case of loading from JSON.
    /// For other formats, use `load_from` instead.
    ///
    /// # Arguments
    ///
    /// * `entity` - The entity name used when registering the migration path
    /// * `json` - A JSON string containing versioned data
    ///
    /// # Returns
    ///
    /// The migrated data as the domain model type
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JSON cannot be parsed
    /// - The entity is not registered
    /// - A migration step fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let json = r#"{"version":"1.0.0","data":{"id":"task-1","title":"My Task"}}"#;
    /// let domain: TaskEntity = migrator.load("task", json)?;
    /// ```
    pub fn load<D: DeserializeOwned>(&self, entity: &str, json: &str) -> Result<D, MigrationError> {
        let data: serde_json::Value = serde_json::from_str(json).map_err(|e| {
            MigrationError::DeserializationError(format!("Failed to parse JSON: {}", e))
        })?;
        self.load_from(entity, data)
    }

    /// Loads and migrates data from a flat format JSON string.
    ///
    /// This is a convenience method for loading from flat format JSON where the version
    /// field is at the same level as the data fields.
    ///
    /// # Arguments
    ///
    /// * `entity` - The entity name used when registering the migration path
    /// * `json` - A JSON string containing versioned data in flat format
    ///
    /// # Returns
    ///
    /// The migrated data as the domain model type
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JSON cannot be parsed
    /// - The entity is not registered
    /// - A migration step fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let json = r#"{"version":"1.0.0","id":"task-1","title":"My Task"}"#;
    /// let domain: TaskEntity = migrator.load_flat("task", json)?;
    /// ```
    pub fn load_flat<D: DeserializeOwned>(
        &self,
        entity: &str,
        json: &str,
    ) -> Result<D, MigrationError> {
        let data: serde_json::Value = serde_json::from_str(json).map_err(|e| {
            MigrationError::DeserializationError(format!("Failed to parse JSON: {}", e))
        })?;
        self.load_flat_from(entity, data)
    }

    /// Loads and migrates data from any serde-compatible format in flat format.
    ///
    /// This method expects the version field to be at the same level as the data fields.
    /// It uses the registered migration path's runtime-configured keys (respecting the
    /// Path > Migrator > Trait priority).
    ///
    /// # Arguments
    ///
    /// * `entity` - The entity name used when registering the migration path
    /// * `value` - A serde-compatible value containing versioned data in flat format
    ///
    /// # Returns
    ///
    /// The migrated data as the domain model type
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The entity is not registered
    /// - The data format is invalid
    /// - A migration step fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let toml_value: toml::Value = toml::from_str(toml_str)?;
    /// let domain: TaskEntity = migrator.load_flat_from("task", toml_value)?;
    /// ```
    pub fn load_flat_from<D, T>(&self, entity: &str, value: T) -> Result<D, MigrationError>
    where
        D: DeserializeOwned,
        T: Serialize,
    {
        let path = self
            .paths
            .get(entity)
            .ok_or_else(|| MigrationError::EntityNotFound(entity.to_string()))?;

        let version_key = &path.version_key;

        // Convert to serde_json::Value for manipulation
        let mut value = serde_json::to_value(value).map_err(|e| {
            MigrationError::SerializationError(format!("Failed to convert input: {}", e))
        })?;

        // Extract version from the flat structure
        let obj = value.as_object_mut().ok_or_else(|| {
            MigrationError::DeserializationError(
                "Expected object with version field at top level".to_string(),
            )
        })?;

        let current_version = obj
            .remove(version_key)
            .ok_or_else(|| {
                MigrationError::DeserializationError(format!(
                    "Missing '{}' field in flat format",
                    version_key
                ))
            })?
            .as_str()
            .ok_or_else(|| {
                MigrationError::DeserializationError(format!(
                    "Invalid '{}' field type",
                    version_key
                ))
            })?
            .to_string();

        // Now obj contains only data fields (version has been removed)
        let mut current_data = serde_json::Value::Object(obj.clone());
        let mut current_version = current_version;

        // Apply migration steps until we reach a version with no further steps
        while let Some(migrate_fn) = path.steps.get(&current_version) {
            // Migration function returns raw value, no wrapping
            current_data = migrate_fn(current_data.clone())?;

            // Update version to the next step
            match path.versions.iter().position(|v| v == &current_version) {
                Some(idx) if idx + 1 < path.versions.len() => {
                    current_version = path.versions[idx + 1].clone();
                }
                _ => break,
            }
        }

        // Finalize into domain model
        let domain_value = (path.finalize)(current_data)?;

        serde_json::from_value(domain_value).map_err(|e| {
            MigrationError::DeserializationError(format!("Failed to convert to domain: {}", e))
        })
    }

    /// Saves versioned data to a JSON string.
    ///
    /// This method wraps the provided data with its version information and serializes
    /// it to JSON format. The resulting JSON can later be loaded and migrated using
    /// the `load` method.
    ///
    /// # Arguments
    ///
    /// * `data` - The versioned data to save
    ///
    /// # Returns
    ///
    /// A JSON string with the format: `{"version":"x.y.z","data":{...}}`
    ///
    /// # Errors
    ///
    /// Returns `SerializationError` if the data cannot be serialized to JSON.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let task = TaskV1_0_0 {
    ///     id: "task-1".to_string(),
    ///     title: "My Task".to_string(),
    /// };
    ///
    /// let migrator = Migrator::new();
    /// let json = migrator.save(task)?;
    /// // json: {"version":"1.0.0","data":{"id":"task-1","title":"My Task"}}
    /// ```
    pub fn save<T: Versioned + Serialize>(&self, data: T) -> Result<String, MigrationError> {
        // Use custom keys from the type's Versioned trait
        let version_key = T::VERSION_KEY;
        let data_key = T::DATA_KEY;

        // Serialize the data
        let data_value = serde_json::to_value(&data).map_err(|e| {
            MigrationError::SerializationError(format!("Failed to serialize data: {}", e))
        })?;

        // Build the wrapper with custom keys
        let mut map = serde_json::Map::new();
        map.insert(
            version_key.to_string(),
            serde_json::Value::String(T::VERSION.to_string()),
        );
        map.insert(data_key.to_string(), data_value);

        serde_json::to_string(&map).map_err(|e| {
            MigrationError::SerializationError(format!("Failed to serialize wrapper: {}", e))
        })
    }

    /// Saves versioned data to a JSON string in flat format.
    ///
    /// Unlike `save()`, this method produces a flat JSON structure where the version
    /// field is at the same level as the data fields, not wrapped in a separate object.
    ///
    /// # Arguments
    ///
    /// * `data` - The versioned data to save
    ///
    /// # Returns
    ///
    /// A JSON string with the format: `{"version":"x.y.z","field1":"value1",...}`
    ///
    /// # Errors
    ///
    /// Returns `SerializationError` if the data cannot be serialized to JSON.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let task = TaskV1_0_0 {
    ///     id: "task-1".to_string(),
    ///     title: "My Task".to_string(),
    /// };
    ///
    /// let migrator = Migrator::new();
    /// let json = migrator.save_flat(task)?;
    /// // json: {"version":"1.0.0","id":"task-1","title":"My Task"}
    /// ```
    pub fn save_flat<T: Versioned + Serialize>(&self, data: T) -> Result<String, MigrationError> {
        let version_key = T::VERSION_KEY;

        // Serialize the data to a JSON object
        let mut data_value = serde_json::to_value(&data).map_err(|e| {
            MigrationError::SerializationError(format!("Failed to serialize data: {}", e))
        })?;

        // Ensure it's an object so we can add the version field
        let obj = data_value.as_object_mut().ok_or_else(|| {
            MigrationError::SerializationError(
                "Data must serialize to a JSON object for flat format".to_string(),
            )
        })?;

        // Add the version field to the same level as data fields
        obj.insert(
            version_key.to_string(),
            serde_json::Value::String(T::VERSION.to_string()),
        );

        serde_json::to_string(&obj).map_err(|e| {
            MigrationError::SerializationError(format!("Failed to serialize flat format: {}", e))
        })
    }

    /// Loads and migrates multiple entities from any serde-compatible format.
    ///
    /// This is the generic version that accepts any type implementing `Serialize`.
    /// For JSON arrays, use the convenience method `load_vec` instead.
    ///
    /// # Arguments
    ///
    /// * `entity` - The entity name used when registering the migration path
    /// * `data` - Array of versioned data in any serde-compatible format
    ///
    /// # Returns
    ///
    /// A vector of migrated data as domain model types
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The data cannot be converted to the internal format
    /// - The entity is not registered
    /// - Any migration step fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Load from TOML array
    /// let toml_array: Vec<toml::Value> = /* ... */;
    /// let domains: Vec<TaskEntity> = migrator.load_vec_from("task", toml_array)?;
    ///
    /// // Load from JSON Value array
    /// let json_array: Vec<serde_json::Value> = /* ... */;
    /// let domains: Vec<TaskEntity> = migrator.load_vec_from("task", json_array)?;
    /// ```
    pub fn load_vec_from<D, T>(&self, entity: &str, data: Vec<T>) -> Result<Vec<D>, MigrationError>
    where
        D: DeserializeOwned,
        T: Serialize,
    {
        data.into_iter()
            .map(|item| self.load_from(entity, item))
            .collect()
    }

    /// Loads and migrates multiple entities from a JSON array string.
    ///
    /// This is a convenience method for the common case of loading from a JSON array.
    /// For other formats, use `load_vec_from` instead.
    ///
    /// # Arguments
    ///
    /// * `entity` - The entity name used when registering the migration path
    /// * `json` - A JSON array string containing versioned data
    ///
    /// # Returns
    ///
    /// A vector of migrated data as domain model types
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JSON cannot be parsed
    /// - The entity is not registered
    /// - Any migration step fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let json = r#"[
    ///     {"version":"1.0.0","data":{"id":"task-1","title":"Task 1"}},
    ///     {"version":"1.0.0","data":{"id":"task-2","title":"Task 2"}}
    /// ]"#;
    /// let domains: Vec<TaskEntity> = migrator.load_vec("task", json)?;
    /// ```
    pub fn load_vec<D: DeserializeOwned>(
        &self,
        entity: &str,
        json: &str,
    ) -> Result<Vec<D>, MigrationError> {
        let data: Vec<serde_json::Value> = serde_json::from_str(json).map_err(|e| {
            MigrationError::DeserializationError(format!("Failed to parse JSON array: {}", e))
        })?;
        self.load_vec_from(entity, data)
    }

    /// Loads and migrates multiple entities from a flat format JSON array string.
    ///
    /// This is a convenience method for loading from a JSON array where each element
    /// has the version field at the same level as the data fields.
    ///
    /// # Arguments
    ///
    /// * `entity` - The entity name used when registering the migration path
    /// * `json` - A JSON array string containing versioned data in flat format
    ///
    /// # Returns
    ///
    /// A vector of migrated data as domain model types
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JSON cannot be parsed
    /// - The entity is not registered
    /// - Any migration step fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let json = r#"[
    ///     {"version":"1.0.0","id":"task-1","title":"Task 1"},
    ///     {"version":"1.0.0","id":"task-2","title":"Task 2"}
    /// ]"#;
    /// let domains: Vec<TaskEntity> = migrator.load_vec_flat("task", json)?;
    /// ```
    pub fn load_vec_flat<D: DeserializeOwned>(
        &self,
        entity: &str,
        json: &str,
    ) -> Result<Vec<D>, MigrationError> {
        let data: Vec<serde_json::Value> = serde_json::from_str(json).map_err(|e| {
            MigrationError::DeserializationError(format!("Failed to parse JSON array: {}", e))
        })?;
        self.load_vec_flat_from(entity, data)
    }

    /// Loads and migrates multiple entities from any serde-compatible format in flat format.
    ///
    /// This method expects each element to have the version field at the same level
    /// as the data fields. It uses the registered migration path's runtime-configured
    /// keys (respecting the Path > Migrator > Trait priority).
    ///
    /// # Arguments
    ///
    /// * `entity` - The entity name used when registering the migration path
    /// * `data` - Vector of serde-compatible values in flat format
    ///
    /// # Returns
    ///
    /// A vector of migrated data as domain model types
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The entity is not registered
    /// - The data format is invalid
    /// - Any migration step fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let toml_array: Vec<toml::Value> = /* ... */;
    /// let domains: Vec<TaskEntity> = migrator.load_vec_flat_from("task", toml_array)?;
    /// ```
    pub fn load_vec_flat_from<D, T>(
        &self,
        entity: &str,
        data: Vec<T>,
    ) -> Result<Vec<D>, MigrationError>
    where
        D: DeserializeOwned,
        T: Serialize,
    {
        data.into_iter()
            .map(|item| self.load_flat_from(entity, item))
            .collect()
    }

    /// Saves multiple versioned entities to a JSON array string.
    ///
    /// This method wraps each item with its version information and serializes
    /// them as a JSON array. The resulting JSON can later be loaded and migrated
    /// using the `load_vec` method.
    ///
    /// # Arguments
    ///
    /// * `data` - Vector of versioned data to save
    ///
    /// # Returns
    ///
    /// A JSON array string where each element has the format: `{"version":"x.y.z","data":{...}}`
    ///
    /// # Errors
    ///
    /// Returns `SerializationError` if the data cannot be serialized to JSON.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tasks = vec![
    ///     TaskV1_0_0 {
    ///         id: "task-1".to_string(),
    ///         title: "Task 1".to_string(),
    ///     },
    ///     TaskV1_0_0 {
    ///         id: "task-2".to_string(),
    ///         title: "Task 2".to_string(),
    ///     },
    /// ];
    ///
    /// let migrator = Migrator::new();
    /// let json = migrator.save_vec(tasks)?;
    /// // json: [{"version":"1.0.0","data":{"id":"task-1",...}}, ...]
    /// ```
    pub fn save_vec<T: Versioned + Serialize>(
        &self,
        data: Vec<T>,
    ) -> Result<String, MigrationError> {
        let version_key = T::VERSION_KEY;
        let data_key = T::DATA_KEY;

        let wrappers: Result<Vec<serde_json::Value>, MigrationError> = data
            .into_iter()
            .map(|item| {
                let data_value = serde_json::to_value(&item).map_err(|e| {
                    MigrationError::SerializationError(format!("Failed to serialize item: {}", e))
                })?;

                let mut map = serde_json::Map::new();
                map.insert(
                    version_key.to_string(),
                    serde_json::Value::String(T::VERSION.to_string()),
                );
                map.insert(data_key.to_string(), data_value);

                Ok(serde_json::Value::Object(map))
            })
            .collect();

        serde_json::to_string(&wrappers?).map_err(|e| {
            MigrationError::SerializationError(format!("Failed to serialize data array: {}", e))
        })
    }

    /// Saves multiple versioned entities to a JSON array string in flat format.
    ///
    /// This method serializes each item with the version field at the same level
    /// as the data fields, not wrapped in a separate object.
    ///
    /// # Arguments
    ///
    /// * `data` - Vector of versioned data to save
    ///
    /// # Returns
    ///
    /// A JSON array string where each element has the format: `{"version":"x.y.z","field1":"value1",...}`
    ///
    /// # Errors
    ///
    /// Returns `SerializationError` if the data cannot be serialized to JSON.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tasks = vec![
    ///     TaskV1_0_0 {
    ///         id: "task-1".to_string(),
    ///         title: "Task 1".to_string(),
    ///     },
    ///     TaskV1_0_0 {
    ///         id: "task-2".to_string(),
    ///         title: "Task 2".to_string(),
    ///     },
    /// ];
    ///
    /// let migrator = Migrator::new();
    /// let json = migrator.save_vec_flat(tasks)?;
    /// // json: [{"version":"1.0.0","id":"task-1",...}, ...]
    /// ```
    pub fn save_vec_flat<T: Versioned + Serialize>(
        &self,
        data: Vec<T>,
    ) -> Result<String, MigrationError> {
        let version_key = T::VERSION_KEY;

        let flat_items: Result<Vec<serde_json::Value>, MigrationError> = data
            .into_iter()
            .map(|item| {
                let mut data_value = serde_json::to_value(&item).map_err(|e| {
                    MigrationError::SerializationError(format!("Failed to serialize item: {}", e))
                })?;

                let obj = data_value.as_object_mut().ok_or_else(|| {
                    MigrationError::SerializationError(
                        "Data must serialize to a JSON object for flat format".to_string(),
                    )
                })?;

                // Add version field at the same level
                obj.insert(
                    version_key.to_string(),
                    serde_json::Value::String(T::VERSION.to_string()),
                );

                Ok(serde_json::Value::Object(obj.clone()))
            })
            .collect();

        serde_json::to_string(&flat_items?).map_err(|e| {
            MigrationError::SerializationError(format!(
                "Failed to serialize flat data array: {}",
                e
            ))
        })
    }

    /// Saves a domain entity to a JSON string using its latest versioned format.
    ///
    /// This method automatically converts the domain entity to its latest version
    /// and saves it with version information.
    ///
    /// # Arguments
    ///
    /// * `entity` - The domain entity to save (must implement `LatestVersioned`)
    ///
    /// # Returns
    ///
    /// A JSON string with the format: `{"version":"x.y.z","data":{...}}`
    ///
    /// # Errors
    ///
    /// Returns `SerializationError` if the entity cannot be serialized to JSON.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[version_migrate(entity = "task", latest = TaskV1_1_0)]
    /// struct TaskEntity {
    ///     id: String,
    ///     title: String,
    ///     description: Option<String>,
    /// }
    ///
    /// let entity = TaskEntity {
    ///     id: "task-1".to_string(),
    ///     title: "My Task".to_string(),
    ///     description: Some("Description".to_string()),
    /// };
    ///
    /// let migrator = Migrator::new();
    /// let json = migrator.save_entity(entity)?;
    /// // Automatically saved with latest version (1.1.0)
    /// ```
    pub fn save_entity<E: crate::LatestVersioned>(
        &self,
        entity: E,
    ) -> Result<String, MigrationError> {
        let latest = entity.to_latest();
        self.save(latest)
    }

    /// Saves a domain entity to a JSON string in flat format using its latest versioned format.
    ///
    /// This method automatically converts the domain entity to its latest version
    /// and saves it with the version field at the same level as data fields.
    ///
    /// # Arguments
    ///
    /// * `entity` - The domain entity to save (must implement `LatestVersioned`)
    ///
    /// # Returns
    ///
    /// A JSON string with the format: `{"version":"x.y.z","field1":"value1",...}`
    ///
    /// # Errors
    ///
    /// Returns `SerializationError` if the entity cannot be serialized to JSON.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[version_migrate(entity = "task", latest = TaskV1_1_0)]
    /// struct TaskEntity {
    ///     id: String,
    ///     title: String,
    ///     description: Option<String>,
    /// }
    ///
    /// let entity = TaskEntity {
    ///     id: "task-1".to_string(),
    ///     title: "My Task".to_string(),
    ///     description: Some("Description".to_string()),
    /// };
    ///
    /// let migrator = Migrator::new();
    /// let json = migrator.save_entity_flat(entity)?;
    /// // json: {"version":"1.1.0","id":"task-1","title":"My Task",...}
    /// ```
    pub fn save_entity_flat<E: crate::LatestVersioned>(
        &self,
        entity: E,
    ) -> Result<String, MigrationError> {
        let latest = entity.to_latest();
        self.save_flat(latest)
    }

    /// Saves multiple domain entities to a JSON array string using their latest versioned format.
    ///
    /// This method automatically converts each domain entity to its latest version
    /// and saves them as a JSON array.
    ///
    /// # Arguments
    ///
    /// * `entities` - Vector of domain entities to save
    ///
    /// # Returns
    ///
    /// A JSON array string where each element has the format: `{"version":"x.y.z","data":{...}}`
    ///
    /// # Errors
    ///
    /// Returns `SerializationError` if the entities cannot be serialized to JSON.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let entities = vec![
    ///     TaskEntity { id: "1".into(), title: "Task 1".into(), description: None },
    ///     TaskEntity { id: "2".into(), title: "Task 2".into(), description: None },
    /// ];
    ///
    /// let json = migrator.save_entity_vec(entities)?;
    /// ```
    pub fn save_entity_vec<E: crate::LatestVersioned>(
        &self,
        entities: Vec<E>,
    ) -> Result<String, MigrationError> {
        let versioned: Vec<E::Latest> = entities.into_iter().map(|e| e.to_latest()).collect();
        self.save_vec(versioned)
    }

    /// Saves multiple domain entities to a JSON array string in flat format using their latest versioned format.
    ///
    /// This method automatically converts each domain entity to its latest version
    /// and saves them with version fields at the same level as data fields.
    ///
    /// # Arguments
    ///
    /// * `entities` - Vector of domain entities to save
    ///
    /// # Returns
    ///
    /// A JSON array string where each element has the format: `{"version":"x.y.z","field1":"value1",...}`
    ///
    /// # Errors
    ///
    /// Returns `SerializationError` if the entities cannot be serialized to JSON.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let entities = vec![
    ///     TaskEntity { id: "1".into(), title: "Task 1".into(), description: None },
    ///     TaskEntity { id: "2".into(), title: "Task 2".into(), description: None },
    /// ];
    ///
    /// let json = migrator.save_entity_vec_flat(entities)?;
    /// ```
    pub fn save_entity_vec_flat<E: crate::LatestVersioned>(
        &self,
        entities: Vec<E>,
    ) -> Result<String, MigrationError> {
        let versioned: Vec<E::Latest> = entities.into_iter().map(|e| e.to_latest()).collect();
        self.save_vec_flat(versioned)
    }

    /// Saves a domain entity to a JSON string using its latest versioned format, by entity name.
    ///
    /// This method works without requiring the `VersionMigrate` macro on the entity type.
    /// Instead, it uses the save function registered during `register()` via `into_with_save()`.
    ///
    /// # Arguments
    ///
    /// * `entity_name` - The entity name used when registering the migration path
    /// * `entity` - The domain entity to save (must be Serialize)
    ///
    /// # Returns
    ///
    /// A JSON string with the format: `{"version":"x.y.z","data":{...}}`
    ///
    /// # Errors
    ///
    /// Returns `MigrationPathNotDefined` if the entity is not registered with save support.
    /// Returns `SerializationError` if the entity cannot be serialized.
    ///
    /// # Example
    ///
    /// ```ignore
    /// impl FromDomain<TaskEntity> for TaskV1_1_0 {
    ///     fn from_domain(entity: TaskEntity) -> Self { ... }
    /// }
    ///
    /// let path = Migrator::define("task")
    ///     .from::<TaskV1_0_0>()
    ///     .step::<TaskV1_1_0>()
    ///     .into_with_save::<TaskEntity>();
    ///
    /// migrator.register(path)?;
    ///
    /// let entity = TaskEntity { ... };
    /// let json = migrator.save_domain("task", entity)?;
    /// // → {"version":"1.1.0","data":{"id":"1","title":"My Task",...}}
    /// ```
    pub fn save_domain<T: Serialize>(
        &self,
        entity_name: &str,
        entity: T,
    ) -> Result<String, MigrationError> {
        let saver = self.domain_savers.get(entity_name).ok_or_else(|| {
            MigrationError::EntityNotFound(format!(
                "Entity '{}' is not registered with domain save support. Use into_with_save() when defining the migration path.",
                entity_name
            ))
        })?;

        // Get version/data keys from registered path
        let path = self.paths.get(entity_name).ok_or_else(|| {
            MigrationError::EntityNotFound(format!("Entity '{}' is not registered", entity_name))
        })?;

        let domain_value = serde_json::to_value(entity).map_err(|e| {
            MigrationError::SerializationError(format!("Failed to serialize entity: {}", e))
        })?;

        (saver.save_fn)(domain_value, &path.version_key, &path.data_key)
    }

    /// Saves a domain entity to a JSON string in flat format using its latest versioned format, by entity name.
    ///
    /// This method works without requiring the `VersionMigrate` macro on the entity type.
    /// The version field is placed at the same level as data fields.
    ///
    /// # Arguments
    ///
    /// * `entity_name` - The entity name used when registering the migration path
    /// * `entity` - The domain entity to save (must be Serialize)
    ///
    /// # Returns
    ///
    /// A JSON string with the format: `{"version":"x.y.z","field1":"value1",...}`
    ///
    /// # Errors
    ///
    /// Returns `MigrationPathNotDefined` if the entity is not registered with save support.
    /// Returns `SerializationError` if the entity cannot be serialized.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let json = migrator.save_domain_flat("task", entity)?;
    /// // → {"version":"1.1.0","id":"1","title":"My Task",...}
    /// ```
    pub fn save_domain_flat<T: Serialize>(
        &self,
        entity_name: &str,
        entity: T,
    ) -> Result<String, MigrationError> {
        let saver = self.domain_savers.get(entity_name).ok_or_else(|| {
            MigrationError::EntityNotFound(format!(
                "Entity '{}' is not registered with domain save support. Use into_with_save() when defining the migration path.",
                entity_name
            ))
        })?;

        // Get version key from registered path
        let path = self.paths.get(entity_name).ok_or_else(|| {
            MigrationError::EntityNotFound(format!("Entity '{}' is not registered", entity_name))
        })?;

        let domain_value = serde_json::to_value(entity).map_err(|e| {
            MigrationError::SerializationError(format!("Failed to serialize entity: {}", e))
        })?;

        (saver.save_flat_fn)(domain_value, &path.version_key)
    }
}

impl Default for Migrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for configuring a `Migrator` with default settings.
pub struct MigratorBuilder {
    default_version_key: Option<String>,
    default_data_key: Option<String>,
}

impl MigratorBuilder {
    pub(crate) fn new() -> Self {
        Self {
            default_version_key: None,
            default_data_key: None,
        }
    }

    /// Sets the default version key for all entities.
    ///
    /// This key will be used unless overridden by:
    /// - The entity's `MigrationPath` via `with_keys()`
    /// - The type's `Versioned` trait constants
    pub fn default_version_key(mut self, key: impl Into<String>) -> Self {
        self.default_version_key = Some(key.into());
        self
    }

    /// Sets the default data key for all entities.
    ///
    /// This key will be used unless overridden by:
    /// - The entity's `MigrationPath` via `with_keys()`
    /// - The type's `Versioned` trait constants
    pub fn default_data_key(mut self, key: impl Into<String>) -> Self {
        self.default_data_key = Some(key.into());
        self
    }

    /// Builds the `Migrator` with the configured defaults.
    pub fn build(self) -> Migrator {
        Migrator {
            paths: HashMap::new(),
            default_version_key: self.default_version_key,
            default_data_key: self.default_data_key,
            domain_savers: HashMap::new(),
        }
    }
}

/// Marker type for builder state: start
pub struct Start;

/// Marker type for builder state: has a starting version
pub struct HasFrom<V>(PhantomData<V>);

/// Marker type for builder state: has intermediate steps
pub struct HasSteps<V>(PhantomData<V>);

/// Builder for defining migration paths.
pub struct MigrationPathBuilder<State> {
    entity: String,
    steps: HashMap<String, MigrationFn>,
    versions: Vec<String>,
    version_key: String,
    data_key: String,
    custom_version_key: Option<String>,
    custom_data_key: Option<String>,
    _state: PhantomData<State>,
}

impl MigrationPathBuilder<Start> {
    fn new(entity: String) -> Self {
        Self {
            entity,
            steps: HashMap::new(),
            versions: Vec::new(),
            version_key: String::from("version"),
            data_key: String::from("data"),
            custom_version_key: None,
            custom_data_key: None,
            _state: PhantomData,
        }
    }

    /// Overrides the version and data keys for this migration path.
    ///
    /// This takes precedence over both the Migrator's defaults and the type's trait constants.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Migrator::define("task")
    ///     .with_keys("custom_version", "custom_data")
    ///     .from::<TaskV1>()
    ///     .into::<TaskDomain>();
    /// ```
    pub fn with_keys(
        mut self,
        version_key: impl Into<String>,
        data_key: impl Into<String>,
    ) -> Self {
        self.custom_version_key = Some(version_key.into());
        self.custom_data_key = Some(data_key.into());
        self
    }

    /// Sets the starting version for migrations.
    pub fn from<V: Versioned + DeserializeOwned>(self) -> MigrationPathBuilder<HasFrom<V>> {
        let mut versions = self.versions;
        versions.push(V::VERSION.to_string());

        MigrationPathBuilder {
            entity: self.entity,
            steps: self.steps,
            versions,
            version_key: V::VERSION_KEY.to_string(),
            data_key: V::DATA_KEY.to_string(),
            custom_version_key: self.custom_version_key,
            custom_data_key: self.custom_data_key,
            _state: PhantomData,
        }
    }
}

impl<V> MigrationPathBuilder<HasFrom<V>>
where
    V: Versioned + DeserializeOwned,
{
    /// Adds a migration step to the next version.
    pub fn step<Next>(mut self) -> MigrationPathBuilder<HasSteps<Next>>
    where
        V: MigratesTo<Next>,
        Next: Versioned + DeserializeOwned + Serialize,
    {
        let from_version = V::VERSION.to_string();
        let migration_fn: MigrationFn = Box::new(move |value| {
            let from_value: V = serde_json::from_value(value).map_err(|e| {
                MigrationError::DeserializationError(format!(
                    "Failed to deserialize version {}: {}",
                    V::VERSION,
                    e
                ))
            })?;

            let to_value = from_value.migrate();

            // Return the raw migrated value without wrapping
            serde_json::to_value(&to_value).map_err(|e| MigrationError::MigrationStepFailed {
                from: V::VERSION.to_string(),
                to: Next::VERSION.to_string(),
                error: e.to_string(),
            })
        });

        self.steps.insert(from_version, migration_fn);
        self.versions.push(Next::VERSION.to_string());

        MigrationPathBuilder {
            entity: self.entity,
            steps: self.steps,
            versions: self.versions,
            version_key: self.version_key,
            data_key: self.data_key,
            custom_version_key: self.custom_version_key,
            custom_data_key: self.custom_data_key,
            _state: PhantomData,
        }
    }

    /// Finalizes the migration path with conversion to domain model.
    pub fn into<D: DeserializeOwned + Serialize>(self) -> MigrationPath<D>
    where
        V: IntoDomain<D>,
    {
        let finalize: Box<
            dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError> + Send + Sync,
        > = Box::new(move |value| {
            let versioned: V = serde_json::from_value(value).map_err(|e| {
                MigrationError::DeserializationError(format!(
                    "Failed to deserialize final version: {}",
                    e
                ))
            })?;

            let domain = versioned.into_domain();

            serde_json::to_value(domain).map_err(|e| MigrationError::MigrationStepFailed {
                from: V::VERSION.to_string(),
                to: "domain".to_string(),
                error: e.to_string(),
            })
        });

        MigrationPath {
            entity: self.entity,
            inner: EntityMigrationPath {
                steps: self.steps,
                finalize,
                versions: self.versions.clone(),
                version_key: self.version_key,
                data_key: self.data_key,
            },
            versions: self.versions,
            custom_version_key: self.custom_version_key,
            custom_data_key: self.custom_data_key,
            save_fn: None,
            save_flat_fn: None,
            _phantom: PhantomData,
        }
    }

    /// Finalizes the migration path with conversion to domain model and enables domain entity saving.
    ///
    /// This variant registers save functions that allow saving domain entities directly by entity name,
    /// without needing the `VersionMigrate` macro on the entity type.
    ///
    /// # Requirements
    ///
    /// The latest versioned type (V) must implement `FromDomain<D>` to convert domain entities
    /// back to the versioned format for saving.
    ///
    /// # Example
    ///
    /// ```ignore
    /// impl FromDomain<TaskEntity> for TaskV1_1_0 {
    ///     fn from_domain(entity: TaskEntity) -> Self {
    ///         TaskV1_1_0 {
    ///             id: entity.id,
    ///             title: entity.title,
    ///             description: entity.description,
    ///         }
    ///     }
    /// }
    ///
    /// let path = Migrator::define("task")
    ///     .from::<TaskV1_0_0>()
    ///     .step::<TaskV1_1_0>()
    ///     .into_with_save::<TaskEntity>();
    ///
    /// migrator.register(path)?;
    ///
    /// // Now you can save by entity name
    /// let entity = TaskEntity { ... };
    /// let json = migrator.save_domain("task", entity)?;
    /// ```
    pub fn into_with_save<D: DeserializeOwned + Serialize>(self) -> MigrationPath<D>
    where
        V: IntoDomain<D> + crate::FromDomain<D>,
    {
        let finalize: Box<
            dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError> + Send + Sync,
        > = Box::new(move |value| {
            let versioned: V = serde_json::from_value(value).map_err(|e| {
                MigrationError::DeserializationError(format!(
                    "Failed to deserialize final version: {}",
                    e
                ))
            })?;

            let domain = versioned.into_domain();

            serde_json::to_value(domain).map_err(|e| MigrationError::MigrationStepFailed {
                from: V::VERSION.to_string(),
                to: "domain".to_string(),
                error: e.to_string(),
            })
        });

        // Create save function for domain entities
        let version = V::VERSION;

        let save_fn: DomainSaveFn = Box::new(move |domain_value, vkey, dkey| {
            let domain: D = serde_json::from_value(domain_value).map_err(|e| {
                MigrationError::DeserializationError(format!("Failed to deserialize domain: {}", e))
            })?;

            let latest = V::from_domain(domain);
            let data_value = serde_json::to_value(&latest).map_err(|e| {
                MigrationError::SerializationError(format!("Failed to serialize latest: {}", e))
            })?;

            let mut map = serde_json::Map::new();
            map.insert(
                vkey.to_string(),
                serde_json::Value::String(version.to_string()),
            );
            map.insert(dkey.to_string(), data_value);

            serde_json::to_string(&map).map_err(|e| {
                MigrationError::SerializationError(format!("Failed to serialize wrapper: {}", e))
            })
        });

        let save_flat_fn: DomainSaveFlatFn = Box::new(move |domain_value, vkey| {
            let domain: D = serde_json::from_value(domain_value).map_err(|e| {
                MigrationError::DeserializationError(format!("Failed to deserialize domain: {}", e))
            })?;

            let latest = V::from_domain(domain);
            let mut data_value = serde_json::to_value(&latest).map_err(|e| {
                MigrationError::SerializationError(format!("Failed to serialize latest: {}", e))
            })?;

            let obj = data_value.as_object_mut().ok_or_else(|| {
                MigrationError::SerializationError(
                    "Data must serialize to a JSON object for flat format".to_string(),
                )
            })?;

            obj.insert(
                vkey.to_string(),
                serde_json::Value::String(version.to_string()),
            );

            serde_json::to_string(&obj).map_err(|e| {
                MigrationError::SerializationError(format!(
                    "Failed to serialize flat format: {}",
                    e
                ))
            })
        });

        MigrationPath {
            entity: self.entity,
            inner: EntityMigrationPath {
                steps: self.steps,
                finalize,
                versions: self.versions.clone(),
                version_key: self.version_key,
                data_key: self.data_key,
            },
            versions: self.versions,
            custom_version_key: self.custom_version_key,
            custom_data_key: self.custom_data_key,
            save_fn: Some(save_fn),
            save_flat_fn: Some(save_flat_fn),
            _phantom: PhantomData,
        }
    }
}

impl<V> MigrationPathBuilder<HasSteps<V>>
where
    V: Versioned + DeserializeOwned,
{
    /// Adds another migration step.
    pub fn step<Next>(mut self) -> MigrationPathBuilder<HasSteps<Next>>
    where
        V: MigratesTo<Next>,
        Next: Versioned + DeserializeOwned + Serialize,
    {
        let from_version = V::VERSION.to_string();
        let migration_fn: MigrationFn = Box::new(move |value| {
            let from_value: V = serde_json::from_value(value).map_err(|e| {
                MigrationError::DeserializationError(format!(
                    "Failed to deserialize version {}: {}",
                    V::VERSION,
                    e
                ))
            })?;

            let to_value = from_value.migrate();

            // Return the raw migrated value without wrapping
            serde_json::to_value(&to_value).map_err(|e| MigrationError::MigrationStepFailed {
                from: V::VERSION.to_string(),
                to: Next::VERSION.to_string(),
                error: e.to_string(),
            })
        });

        self.steps.insert(from_version, migration_fn);
        self.versions.push(Next::VERSION.to_string());

        MigrationPathBuilder {
            entity: self.entity,
            steps: self.steps,
            versions: self.versions,
            version_key: self.version_key,
            data_key: self.data_key,
            custom_version_key: self.custom_version_key,
            custom_data_key: self.custom_data_key,
            _state: PhantomData,
        }
    }

    /// Finalizes the migration path with conversion to domain model.
    pub fn into<D: DeserializeOwned + Serialize>(self) -> MigrationPath<D>
    where
        V: IntoDomain<D>,
    {
        let finalize: Box<
            dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError> + Send + Sync,
        > = Box::new(move |value| {
            let versioned: V = serde_json::from_value(value).map_err(|e| {
                MigrationError::DeserializationError(format!(
                    "Failed to deserialize final version: {}",
                    e
                ))
            })?;

            let domain = versioned.into_domain();

            serde_json::to_value(domain).map_err(|e| MigrationError::MigrationStepFailed {
                from: V::VERSION.to_string(),
                to: "domain".to_string(),
                error: e.to_string(),
            })
        });

        MigrationPath {
            entity: self.entity,
            inner: EntityMigrationPath {
                steps: self.steps,
                finalize,
                versions: self.versions.clone(),
                version_key: self.version_key,
                data_key: self.data_key,
            },
            versions: self.versions,
            custom_version_key: self.custom_version_key,
            custom_data_key: self.custom_data_key,
            save_fn: None,
            save_flat_fn: None,
            _phantom: PhantomData,
        }
    }

    /// Finalizes the migration path with conversion to domain model and enables domain entity saving.
    ///
    /// See `MigrationPathBuilder<HasFrom<V>>::into_with_save` for details.
    pub fn into_with_save<D: DeserializeOwned + Serialize>(self) -> MigrationPath<D>
    where
        V: IntoDomain<D> + crate::FromDomain<D>,
    {
        let finalize: Box<
            dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError> + Send + Sync,
        > = Box::new(move |value| {
            let versioned: V = serde_json::from_value(value).map_err(|e| {
                MigrationError::DeserializationError(format!(
                    "Failed to deserialize final version: {}",
                    e
                ))
            })?;

            let domain = versioned.into_domain();

            serde_json::to_value(domain).map_err(|e| MigrationError::MigrationStepFailed {
                from: V::VERSION.to_string(),
                to: "domain".to_string(),
                error: e.to_string(),
            })
        });

        // Create save function for domain entities
        let version = V::VERSION;

        let save_fn: DomainSaveFn = Box::new(move |domain_value, vkey, dkey| {
            let domain: D = serde_json::from_value(domain_value).map_err(|e| {
                MigrationError::DeserializationError(format!("Failed to deserialize domain: {}", e))
            })?;

            let latest = V::from_domain(domain);
            let data_value = serde_json::to_value(&latest).map_err(|e| {
                MigrationError::SerializationError(format!("Failed to serialize latest: {}", e))
            })?;

            let mut map = serde_json::Map::new();
            map.insert(
                vkey.to_string(),
                serde_json::Value::String(version.to_string()),
            );
            map.insert(dkey.to_string(), data_value);

            serde_json::to_string(&map).map_err(|e| {
                MigrationError::SerializationError(format!("Failed to serialize wrapper: {}", e))
            })
        });

        let save_flat_fn: DomainSaveFlatFn = Box::new(move |domain_value, vkey| {
            let domain: D = serde_json::from_value(domain_value).map_err(|e| {
                MigrationError::DeserializationError(format!("Failed to deserialize domain: {}", e))
            })?;

            let latest = V::from_domain(domain);
            let mut data_value = serde_json::to_value(&latest).map_err(|e| {
                MigrationError::SerializationError(format!("Failed to serialize latest: {}", e))
            })?;

            let obj = data_value.as_object_mut().ok_or_else(|| {
                MigrationError::SerializationError(
                    "Data must serialize to a JSON object for flat format".to_string(),
                )
            })?;

            obj.insert(
                vkey.to_string(),
                serde_json::Value::String(version.to_string()),
            );

            serde_json::to_string(&obj).map_err(|e| {
                MigrationError::SerializationError(format!(
                    "Failed to serialize flat format: {}",
                    e
                ))
            })
        });

        MigrationPath {
            entity: self.entity,
            inner: EntityMigrationPath {
                steps: self.steps,
                finalize,
                versions: self.versions.clone(),
                version_key: self.version_key,
                data_key: self.data_key,
            },
            versions: self.versions,
            custom_version_key: self.custom_version_key,
            custom_data_key: self.custom_data_key,
            save_fn: Some(save_fn),
            save_flat_fn: Some(save_flat_fn),
            _phantom: PhantomData,
        }
    }
}

/// A complete migration path from versioned DTOs to a domain model.
pub struct MigrationPath<D> {
    entity: String,
    inner: EntityMigrationPath,
    /// List of versions in the migration path for validation
    versions: Vec<String>,
    /// Custom version key override (takes precedence over Migrator defaults)
    custom_version_key: Option<String>,
    /// Custom data key override (takes precedence over Migrator defaults)
    custom_data_key: Option<String>,
    /// Function to save domain entities (if FromDomain is implemented)
    save_fn: Option<DomainSaveFn>,
    /// Function to save domain entities in flat format (if FromDomain is implemented)
    save_flat_fn: Option<DomainSaveFlatFn>,
    _phantom: PhantomData<D>,
}

/// A wrapper around JSON data that provides convenient query and update methods
/// for partial updates with automatic migration.
///
/// `ConfigMigrator` holds a JSON object and allows you to query specific keys,
/// automatically migrating versioned data to domain entities, and update them
/// with the latest version.
///
/// # Example
///
/// ```ignore
/// // config.json:
/// // {
/// //   "app_name": "MyApp",
/// //   "tasks": [
/// //     {"version": "1.0.0", "id": "1", "title": "Task 1"},
/// //     {"version": "2.0.0", "id": "2", "title": "Task 2", "description": "New"}
/// //   ]
/// // }
///
/// let config_json = fs::read_to_string("config.json")?;
/// let mut config = ConfigMigrator::from(&config_json, migrator)?;
///
/// // Query tasks (automatically migrates all versions to TaskEntity)
/// let mut tasks: Vec<TaskEntity> = config.query("tasks")?;
///
/// // Update tasks
/// tasks[0].title = "Updated Task".to_string();
///
/// // Save back with latest version
/// config.update("tasks", tasks)?;
///
/// // Write to file
/// fs::write("config.json", config.to_string()?)?;
/// ```
pub struct ConfigMigrator {
    root: serde_json::Value,
    migrator: Migrator,
}

impl ConfigMigrator {
    /// Creates a new `ConfigMigrator` from a JSON string and a `Migrator`.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::DeserializationError` if the JSON is invalid.
    pub fn from(json: &str, migrator: Migrator) -> Result<Self, MigrationError> {
        let root = serde_json::from_str(json)
            .map_err(|e| MigrationError::DeserializationError(e.to_string()))?;
        Ok(Self { root, migrator })
    }

    /// Queries a specific key from the JSON object and returns the data as domain entities.
    ///
    /// This method automatically migrates all versioned data to the latest version
    /// and converts them to domain entities.
    ///
    /// # Type Parameters
    ///
    /// - `T`: Must implement `Queryable` to provide the entity name, and `Deserialize` for deserialization.
    ///
    /// # Errors
    ///
    /// - Returns `MigrationError::DeserializationError` if the key doesn't contain a valid array.
    /// - Returns migration errors if the data cannot be migrated.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tasks: Vec<TaskEntity> = config.query("tasks")?;
    /// ```
    pub fn query<T>(&self, key: &str) -> Result<Vec<T>, MigrationError>
    where
        T: crate::Queryable + for<'de> serde::Deserialize<'de>,
    {
        let value = &self.root[key];
        if value.is_null() {
            return Ok(Vec::new());
        }

        if !value.is_array() {
            return Err(MigrationError::DeserializationError(format!(
                "Key '{}' does not contain an array",
                key
            )));
        }

        let array = value.as_array().unwrap(); // Safe because we checked is_array()
        self.migrator
            .load_vec_flat_from(T::ENTITY_NAME, array.to_vec())
    }

    /// Updates a specific key in the JSON object with new domain entities.
    ///
    /// This method serializes the entities with the latest version (automatically
    /// determined from the `Queryable` trait) and updates the JSON object in place.
    ///
    /// # Type Parameters
    ///
    /// - `T`: Must implement `Serialize` and `Queryable`.
    ///
    /// # Errors
    ///
    /// - Returns `MigrationError::EntityNotFound` if the entity is not registered.
    /// - Returns serialization errors if the data cannot be serialized.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Version is automatically determined from the entity's migration path
    /// config.update("tasks", updated_tasks)?;
    /// ```
    pub fn update<T>(&mut self, key: &str, data: Vec<T>) -> Result<(), MigrationError>
    where
        T: serde::Serialize + crate::Queryable,
    {
        let entity_name = T::ENTITY_NAME;
        let latest_version = self
            .migrator
            .get_latest_version(entity_name)
            .ok_or_else(|| MigrationError::EntityNotFound(entity_name.to_string()))?;

        // Serialize each item with version field
        let items: Vec<serde_json::Value> = data
            .into_iter()
            .map(|item| {
                let mut obj = serde_json::to_value(&item)
                    .map_err(|e| MigrationError::SerializationError(e.to_string()))?;

                if let Some(obj_map) = obj.as_object_mut() {
                    obj_map.insert(
                        "version".to_string(),
                        serde_json::Value::String(latest_version.to_string()),
                    );
                }

                Ok(obj)
            })
            .collect::<Result<Vec<_>, MigrationError>>()?;

        self.root[key] = serde_json::Value::Array(items);
        Ok(())
    }

    /// Converts the entire JSON object back to a pretty-printed string.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::SerializationError` if serialization fails.
    pub fn to_string(&self) -> Result<String, MigrationError> {
        serde_json::to_string_pretty(&self.root)
            .map_err(|e| MigrationError::SerializationError(e.to_string()))
    }

    /// Converts the entire JSON object to a compact string.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::SerializationError` if serialization fails.
    pub fn to_string_compact(&self) -> Result<String, MigrationError> {
        serde_json::to_string(&self.root)
            .map_err(|e| MigrationError::SerializationError(e.to_string()))
    }

    /// Returns a reference to the underlying JSON value.
    pub fn as_value(&self) -> &serde_json::Value {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntoDomain, MigratesTo, Versioned, VersionedWrapper};
    use serde::{Deserialize, Serialize};

    // Test data structures
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct V1 {
        value: String,
    }

    impl Versioned for V1 {
        const VERSION: &'static str = "1.0.0";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct V2 {
        value: String,
        count: u32,
    }

    impl Versioned for V2 {
        const VERSION: &'static str = "2.0.0";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct V3 {
        value: String,
        count: u32,
        enabled: bool,
    }

    impl Versioned for V3 {
        const VERSION: &'static str = "3.0.0";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Domain {
        value: String,
        count: u32,
        enabled: bool,
    }

    impl MigratesTo<V2> for V1 {
        fn migrate(self) -> V2 {
            V2 {
                value: self.value,
                count: 0,
            }
        }
    }

    impl MigratesTo<V3> for V2 {
        fn migrate(self) -> V3 {
            V3 {
                value: self.value,
                count: self.count,
                enabled: true,
            }
        }
    }

    impl IntoDomain<Domain> for V3 {
        fn into_domain(self) -> Domain {
            Domain {
                value: self.value,
                count: self.count,
                enabled: self.enabled,
            }
        }
    }

    #[test]
    fn test_migrator_new() {
        let migrator = Migrator::new();
        assert_eq!(migrator.paths.len(), 0);
    }

    #[test]
    fn test_migrator_default() {
        let migrator = Migrator::default();
        assert_eq!(migrator.paths.len(), 0);
    }

    #[test]
    fn test_single_step_migration() {
        let path = Migrator::define("test")
            .from::<V2>()
            .step::<V3>()
            .into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let v2 = V2 {
            value: "test".to_string(),
            count: 42,
        };
        let wrapper = VersionedWrapper::from_versioned(v2);
        let json = serde_json::to_string(&wrapper).unwrap();

        let result: Domain = migrator.load("test", &json).unwrap();
        assert_eq!(result.value, "test");
        assert_eq!(result.count, 42);
        assert!(result.enabled);
    }

    #[test]
    fn test_multi_step_migration() {
        let path = Migrator::define("test")
            .from::<V1>()
            .step::<V2>()
            .step::<V3>()
            .into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let v1 = V1 {
            value: "multi_step".to_string(),
        };
        let wrapper = VersionedWrapper::from_versioned(v1);
        let json = serde_json::to_string(&wrapper).unwrap();

        let result: Domain = migrator.load("test", &json).unwrap();
        assert_eq!(result.value, "multi_step");
        assert_eq!(result.count, 0);
        assert!(result.enabled);
    }

    #[test]
    fn test_no_migration_needed() {
        let path = Migrator::define("test").from::<V3>().into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let v3 = V3 {
            value: "latest".to_string(),
            count: 100,
            enabled: false,
        };
        let wrapper = VersionedWrapper::from_versioned(v3);
        let json = serde_json::to_string(&wrapper).unwrap();

        let result: Domain = migrator.load("test", &json).unwrap();
        assert_eq!(result.value, "latest");
        assert_eq!(result.count, 100);
        assert!(!result.enabled);
    }

    #[test]
    fn test_entity_not_found() {
        let migrator = Migrator::new();

        let v1 = V1 {
            value: "test".to_string(),
        };
        let wrapper = VersionedWrapper::from_versioned(v1);
        let json = serde_json::to_string(&wrapper).unwrap();

        let result: Result<Domain, MigrationError> = migrator.load("unknown", &json);
        assert!(matches!(result, Err(MigrationError::EntityNotFound(_))));

        if let Err(MigrationError::EntityNotFound(entity)) = result {
            assert_eq!(entity, "unknown");
        }
    }

    #[test]
    fn test_invalid_json() {
        let path = Migrator::define("test").from::<V3>().into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let invalid_json = "{ invalid json }";
        let result: Result<Domain, MigrationError> = migrator.load("test", invalid_json);

        assert!(matches!(
            result,
            Err(MigrationError::DeserializationError(_))
        ));
    }

    #[test]
    fn test_multiple_entities() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct OtherDomain {
            value: String,
        }

        impl IntoDomain<OtherDomain> for V1 {
            fn into_domain(self) -> OtherDomain {
                OtherDomain { value: self.value }
            }
        }

        let path1 = Migrator::define("entity1")
            .from::<V1>()
            .step::<V2>()
            .step::<V3>()
            .into::<Domain>();

        let path2 = Migrator::define("entity2")
            .from::<V1>()
            .into::<OtherDomain>();

        let mut migrator = Migrator::new();
        migrator.register(path1).unwrap();
        migrator.register(path2).unwrap();

        // Test entity1
        let v1 = V1 {
            value: "entity1".to_string(),
        };
        let wrapper = VersionedWrapper::from_versioned(v1);
        let json = serde_json::to_string(&wrapper).unwrap();
        let result: Domain = migrator.load("entity1", &json).unwrap();
        assert_eq!(result.value, "entity1");

        // Test entity2
        let v1 = V1 {
            value: "entity2".to_string(),
        };
        let wrapper = VersionedWrapper::from_versioned(v1);
        let json = serde_json::to_string(&wrapper).unwrap();
        let result: OtherDomain = migrator.load("entity2", &json).unwrap();
        assert_eq!(result.value, "entity2");
    }

    #[test]
    fn test_save() {
        let migrator = Migrator::new();

        let v1 = V1 {
            value: "test_save".to_string(),
        };

        let json = migrator.save(v1).unwrap();

        // Verify JSON contains version and data
        assert!(json.contains("\"version\""));
        assert!(json.contains("\"1.0.0\""));
        assert!(json.contains("\"data\""));
        assert!(json.contains("\"test_save\""));

        // Verify it can be parsed back
        let parsed: VersionedWrapper<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, "1.0.0");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let path = Migrator::define("test")
            .from::<V1>()
            .step::<V2>()
            .step::<V3>()
            .into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        // Save V1 data
        let v1 = V1 {
            value: "roundtrip".to_string(),
        };
        let json = migrator.save(v1).unwrap();

        // Load and migrate to Domain
        let domain: Domain = migrator.load("test", &json).unwrap();

        assert_eq!(domain.value, "roundtrip");
        assert_eq!(domain.count, 0); // Default from V1->V2 migration
        assert!(domain.enabled); // Default from V2->V3 migration
    }

    #[test]
    fn test_save_latest_version() {
        let migrator = Migrator::new();

        let v3 = V3 {
            value: "latest".to_string(),
            count: 42,
            enabled: false,
        };

        let json = migrator.save(v3).unwrap();

        // Verify the JSON structure
        assert!(json.contains("\"version\":\"3.0.0\""));
        assert!(json.contains("\"value\":\"latest\""));
        assert!(json.contains("\"count\":42"));
        assert!(json.contains("\"enabled\":false"));
    }

    #[test]
    fn test_save_pretty() {
        let migrator = Migrator::new();

        let v2 = V2 {
            value: "pretty".to_string(),
            count: 10,
        };

        let json = migrator.save(v2).unwrap();

        // Should be compact JSON (not pretty-printed)
        assert!(!json.contains('\n'));
        assert!(json.contains("\"version\":\"2.0.0\""));
    }

    #[test]
    fn test_validation_invalid_version_order() {
        // Manually construct a path with invalid version ordering
        let entity = "test".to_string();
        let versions = vec!["2.0.0".to_string(), "1.0.0".to_string()]; // Wrong order

        let result = Migrator::validate_migration_path(&entity, &versions);
        assert!(matches!(
            result,
            Err(MigrationError::InvalidVersionOrder { .. })
        ));

        if let Err(MigrationError::InvalidVersionOrder {
            entity: e,
            from,
            to,
        }) = result
        {
            assert_eq!(e, "test");
            assert_eq!(from, "2.0.0");
            assert_eq!(to, "1.0.0");
        }
    }

    #[test]
    fn test_validation_circular_path() {
        // Manually construct a path with circular reference
        let entity = "test".to_string();
        let versions = vec![
            "1.0.0".to_string(),
            "2.0.0".to_string(),
            "1.0.0".to_string(), // Circular!
        ];

        let result = Migrator::validate_migration_path(&entity, &versions);
        assert!(matches!(
            result,
            Err(MigrationError::CircularMigrationPath { .. })
        ));

        if let Err(MigrationError::CircularMigrationPath { entity: e, path }) = result {
            assert_eq!(e, "test");
            assert!(path.contains("1.0.0"));
            assert!(path.contains("2.0.0"));
        }
    }

    #[test]
    fn test_validation_valid_path() {
        // Valid migration path
        let entity = "test".to_string();
        let versions = vec![
            "1.0.0".to_string(),
            "1.1.0".to_string(),
            "2.0.0".to_string(),
        ];

        let result = Migrator::validate_migration_path(&entity, &versions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validation_empty_path() {
        // Empty path should be valid
        let entity = "test".to_string();
        let versions = vec![];

        let result = Migrator::validate_migration_path(&entity, &versions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validation_single_version() {
        // Single version path should be valid (no steps, just final conversion)
        let entity = "test".to_string();
        let versions = vec!["1.0.0".to_string()];

        let result = Migrator::validate_migration_path(&entity, &versions);
        assert!(result.is_ok());
    }

    // Tests for Vec operations
    #[test]
    fn test_save_vec_and_load_vec() {
        let migrator = Migrator::new();

        // Save multiple V1 items
        let items = vec![
            V1 {
                value: "item1".to_string(),
            },
            V1 {
                value: "item2".to_string(),
            },
            V1 {
                value: "item3".to_string(),
            },
        ];

        let json = migrator.save_vec(items).unwrap();

        // Verify JSON array format
        assert!(json.starts_with('['));
        assert!(json.ends_with(']'));
        assert!(json.contains("\"version\":\"1.0.0\""));
        assert!(json.contains("item1"));
        assert!(json.contains("item2"));
        assert!(json.contains("item3"));

        // Setup migration path
        let path = Migrator::define("test")
            .from::<V1>()
            .step::<V2>()
            .step::<V3>()
            .into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        // Load and migrate the array
        let domains: Vec<Domain> = migrator.load_vec("test", &json).unwrap();

        assert_eq!(domains.len(), 3);
        assert_eq!(domains[0].value, "item1");
        assert_eq!(domains[1].value, "item2");
        assert_eq!(domains[2].value, "item3");

        // All should have default values from migration
        for domain in &domains {
            assert_eq!(domain.count, 0);
            assert!(domain.enabled);
        }
    }

    #[test]
    fn test_load_vec_empty_array() {
        let path = Migrator::define("test")
            .from::<V1>()
            .step::<V2>()
            .step::<V3>()
            .into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let json = "[]";
        let domains: Vec<Domain> = migrator.load_vec("test", json).unwrap();

        assert_eq!(domains.len(), 0);
    }

    #[test]
    fn test_load_vec_mixed_versions() {
        // Setup migration path
        let path = Migrator::define("test")
            .from::<V1>()
            .step::<V2>()
            .step::<V3>()
            .into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        // JSON with mixed versions
        let json = r#"[
            {"version":"1.0.0","data":{"value":"v1-item"}},
            {"version":"2.0.0","data":{"value":"v2-item","count":42}},
            {"version":"3.0.0","data":{"value":"v3-item","count":99,"enabled":false}}
        ]"#;

        let domains: Vec<Domain> = migrator.load_vec("test", json).unwrap();

        assert_eq!(domains.len(), 3);

        // V1 item migrated to domain
        assert_eq!(domains[0].value, "v1-item");
        assert_eq!(domains[0].count, 0);
        assert!(domains[0].enabled);

        // V2 item migrated to domain
        assert_eq!(domains[1].value, "v2-item");
        assert_eq!(domains[1].count, 42);
        assert!(domains[1].enabled);

        // V3 item converted to domain
        assert_eq!(domains[2].value, "v3-item");
        assert_eq!(domains[2].count, 99);
        assert!(!domains[2].enabled);
    }

    #[test]
    fn test_load_vec_from_json_values() {
        let path = Migrator::define("test")
            .from::<V1>()
            .step::<V2>()
            .step::<V3>()
            .into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        // Create Vec<serde_json::Value> directly
        let values: Vec<serde_json::Value> = vec![
            serde_json::json!({"version":"1.0.0","data":{"value":"direct1"}}),
            serde_json::json!({"version":"1.0.0","data":{"value":"direct2"}}),
        ];

        let domains: Vec<Domain> = migrator.load_vec_from("test", values).unwrap();

        assert_eq!(domains.len(), 2);
        assert_eq!(domains[0].value, "direct1");
        assert_eq!(domains[1].value, "direct2");
    }

    #[test]
    fn test_save_vec_empty() {
        let migrator = Migrator::new();
        let empty: Vec<V1> = vec![];

        let json = migrator.save_vec(empty).unwrap();

        assert_eq!(json, "[]");
    }

    #[test]
    fn test_load_vec_invalid_json() {
        let path = Migrator::define("test")
            .from::<V1>()
            .step::<V2>()
            .step::<V3>()
            .into::<Domain>();

        let mut migrator = Migrator::new();
        migrator.register(path).unwrap();

        let invalid_json = "{ not an array }";
        let result: Result<Vec<Domain>, MigrationError> = migrator.load_vec("test", invalid_json);

        assert!(matches!(
            result,
            Err(MigrationError::DeserializationError(_))
        ));
    }

    #[test]
    fn test_load_vec_entity_not_found() {
        let migrator = Migrator::new();

        let json = r#"[{"version":"1.0.0","data":{"value":"test"}}]"#;
        let result: Result<Vec<Domain>, MigrationError> = migrator.load_vec("unknown", json);

        assert!(matches!(result, Err(MigrationError::EntityNotFound(_))));
    }

    #[test]
    fn test_save_vec_latest_version() {
        let migrator = Migrator::new();

        let items = vec![
            V3 {
                value: "latest1".to_string(),
                count: 10,
                enabled: true,
            },
            V3 {
                value: "latest2".to_string(),
                count: 20,
                enabled: false,
            },
        ];

        let json = migrator.save_vec(items).unwrap();

        // Verify structure
        assert!(json.contains("\"version\":\"3.0.0\""));
        assert!(json.contains("latest1"));
        assert!(json.contains("latest2"));
        assert!(json.contains("\"count\":10"));
        assert!(json.contains("\"count\":20"));
    }
}
