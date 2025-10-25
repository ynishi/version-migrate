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

    /// Loads and migrates data from a JSON string.
    pub fn load<D: DeserializeOwned>(&self, entity: &str, json: &str) -> Result<D, MigrationError> {
        // First, deserialize the wrapper to get the version
        let wrapper: VersionedWrapper<serde_json::Value> =
            serde_json::from_str(json).map_err(|e| {
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
}
