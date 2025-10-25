//! Migration manager and builder pattern for defining type-safe migration paths.

use crate::errors::MigrationError;
use crate::{IntoDomain, MigratesTo, Versioned, VersionedWrapper};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::marker::PhantomData;

type MigrationFn = Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError>>;

/// A registered migration path for a specific entity type.
struct EntityMigrationPath {
    /// Maps version -> migration function to next version
    steps: HashMap<String, MigrationFn>,
    /// The final conversion to domain model
    finalize: Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError>>,
}

/// The migration manager that orchestrates all migrations.
pub struct Migrator {
    paths: HashMap<String, EntityMigrationPath>,
}

impl Migrator {
    /// Creates a new, empty migrator.
    pub fn new() -> Self {
        Self {
            paths: HashMap::new(),
        }
    }

    /// Starts defining a migration path for an entity.
    pub fn define(entity: &str) -> MigrationPathBuilder<Start> {
        MigrationPathBuilder::new(entity.to_string())
    }

    /// Registers a migration path.
    pub fn register<D>(&mut self, path: MigrationPath<D>) {
        self.paths.insert(path.entity, path.inner);
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

        // First, deserialize the wrapper to get the version
        let wrapper: VersionedWrapper<serde_json::Value> =
            serde_json::from_value(value).map_err(|e| {
                MigrationError::DeserializationError(format!(
                    "Failed to parse VersionedWrapper: {}",
                    e
                ))
            })?;

        // Get the migration path for this entity
        let path = self
            .paths
            .get(entity)
            .ok_or_else(|| MigrationError::EntityNotFound(entity.to_string()))?;

        // Start migrating
        let mut current_version = wrapper.version.clone();
        let mut current_data = wrapper.data;

        // Apply migration steps until we reach a version with no further steps
        while let Some(migrate_fn) = path.steps.get(&current_version) {
            current_data = migrate_fn(current_data)?;
            // Extract the new version from the migrated data
            // This assumes the data is always wrapped in a VersionedWrapper
            if let Ok(wrapped) =
                serde_json::from_value::<VersionedWrapper<serde_json::Value>>(current_data.clone())
            {
                current_version = wrapped.version;
                current_data = wrapped.data;
            } else {
                break;
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
        let wrapper = VersionedWrapper::from_versioned(data);

        serde_json::to_string(&wrapper).map_err(|e| {
            MigrationError::SerializationError(format!("Failed to serialize data: {}", e))
        })
    }
}

impl Default for Migrator {
    fn default() -> Self {
        Self::new()
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
    _state: PhantomData<State>,
}

impl MigrationPathBuilder<Start> {
    fn new(entity: String) -> Self {
        Self {
            entity,
            steps: HashMap::new(),
            _state: PhantomData,
        }
    }

    /// Sets the starting version for migrations.
    pub fn from<V: Versioned + DeserializeOwned>(self) -> MigrationPathBuilder<HasFrom<V>> {
        MigrationPathBuilder {
            entity: self.entity,
            steps: self.steps,
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
            let wrapped = VersionedWrapper::from_versioned(to_value);

            serde_json::to_value(wrapped).map_err(|e| MigrationError::MigrationStepFailed {
                from: V::VERSION.to_string(),
                to: Next::VERSION.to_string(),
                error: e.to_string(),
            })
        });

        self.steps.insert(from_version, migration_fn);

        MigrationPathBuilder {
            entity: self.entity,
            steps: self.steps,
            _state: PhantomData,
        }
    }

    /// Finalizes the migration path with conversion to domain model.
    pub fn into<D: DeserializeOwned + Serialize>(self) -> MigrationPath<D>
    where
        V: IntoDomain<D>,
    {
        let finalize: Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError>> =
            Box::new(move |value| {
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
            },
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
            let wrapped = VersionedWrapper::from_versioned(to_value);

            serde_json::to_value(wrapped).map_err(|e| MigrationError::MigrationStepFailed {
                from: V::VERSION.to_string(),
                to: Next::VERSION.to_string(),
                error: e.to_string(),
            })
        });

        self.steps.insert(from_version, migration_fn);

        MigrationPathBuilder {
            entity: self.entity,
            steps: self.steps,
            _state: PhantomData,
        }
    }

    /// Finalizes the migration path with conversion to domain model.
    pub fn into<D: DeserializeOwned + Serialize>(self) -> MigrationPath<D>
    where
        V: IntoDomain<D>,
    {
        let finalize: Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, MigrationError>> =
            Box::new(move |value| {
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
            },
            _phantom: PhantomData,
        }
    }
}

/// A complete migration path from versioned DTOs to a domain model.
pub struct MigrationPath<D> {
    entity: String,
    inner: EntityMigrationPath,
    _phantom: PhantomData<D>,
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
        migrator.register(path);

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
        migrator.register(path);

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
        migrator.register(path);

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
        migrator.register(path);

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
        migrator.register(path1);
        migrator.register(path2);

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
        migrator.register(path);

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
}
