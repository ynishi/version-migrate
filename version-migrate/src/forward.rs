//! Forward compatibility support for loading future/unknown versions.
//!
//! This module provides types and utilities for handling data from versions
//! that don't exist in the current codebase yet.
//!
//! # ⚠️ Requirements
//!
//! Forward compatibility assumes **additive-only schema changes**:
//!
//! - ✅ Field additions (V2 has fields V1 doesn't) → OK
//! - ❌ Field deletions (V1 has fields V2 doesn't) → Deserialization error
//! - ❌ Field type changes → Data corruption
//! - ❌ Field semantic changes (same name, different meaning) → Logic bugs
//!
//! If your schema has breaking changes, define a proper migration path instead.

use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

/// Context for forward compatibility operations.
///
/// Stores information about the original data that may be lost during
/// lossy deserialization to an older schema version.
#[derive(Debug, Clone)]
pub struct ForwardContext {
    /// The original version string from the data
    pub(crate) original_version: String,
    /// Fields that were present in the data but not in the target type
    pub(crate) unknown_fields: serde_json::Map<String, serde_json::Value>,
    /// Whether the load was lossy (unknown version)
    pub(crate) was_lossy: bool,
    /// The version key used in serialization
    pub(crate) version_key: String,
    /// The data key used in serialization (for wrapped format)
    pub(crate) data_key: String,
    /// Whether the original format was flat
    pub(crate) was_flat: bool,
}

impl ForwardContext {
    /// Creates a new ForwardContext.
    pub(crate) fn new(
        original_version: String,
        unknown_fields: serde_json::Map<String, serde_json::Value>,
        was_lossy: bool,
        version_key: String,
        data_key: String,
        was_flat: bool,
    ) -> Self {
        Self {
            original_version,
            unknown_fields,
            was_lossy,
            version_key,
            data_key,
            was_flat,
        }
    }

    /// Returns the original version of the data.
    pub fn original_version(&self) -> &str {
        &self.original_version
    }

    /// Returns true if the load was lossy (unknown version).
    pub fn was_lossy(&self) -> bool {
        self.was_lossy
    }

    /// Returns the unknown fields that were preserved.
    pub fn unknown_fields(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.unknown_fields
    }
}

/// A wrapper that holds domain data along with forward compatibility context.
///
/// This type preserves information from unknown versions so that when saved,
/// the data can be written back with minimal information loss.
///
/// # Usage
///
/// ```ignore
/// // Load with forward compatibility
/// let mut task: Forwardable<TaskEntity> = migrator.load_forward("task", json)?;
///
/// // Access inner data (Deref makes this transparent)
/// task.title = "updated".to_string();
///
/// // Check if it was a lossy load
/// if task.was_lossy() {
///     warn!("Loaded from unknown version: {}", task.original_version());
/// }
///
/// // Save preserving unknown fields and original version
/// let json = migrator.save_forward(&task)?;
/// ```
#[derive(Debug, Clone)]
pub struct Forwardable<T> {
    /// The inner domain data.
    pub inner: T,
    /// Context for preserving forward compatibility information.
    ctx: ForwardContext,
}

impl<T> Forwardable<T> {
    /// Creates a new Forwardable wrapper.
    pub(crate) fn new(inner: T, ctx: ForwardContext) -> Self {
        Self { inner, ctx }
    }

    /// Returns the original version of the data.
    pub fn original_version(&self) -> &str {
        self.ctx.original_version()
    }

    /// Returns true if the load was lossy (unknown version).
    pub fn was_lossy(&self) -> bool {
        self.ctx.was_lossy()
    }

    /// Returns the unknown fields that were preserved.
    pub fn unknown_fields(&self) -> &serde_json::Map<String, serde_json::Value> {
        self.ctx.unknown_fields()
    }

    /// Returns a reference to the forward context.
    pub fn context(&self) -> &ForwardContext {
        &self.ctx
    }

    /// Consumes the wrapper and returns the inner value.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> Deref for Forwardable<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for Forwardable<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T: Serialize> Serialize for Forwardable<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Delegate to inner for normal serialization
        // save_forward handles the special logic
        self.inner.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Forwardable<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // For normal deserialization, create with empty context
        // load_forward handles the special logic
        let inner = T::deserialize(deserializer)?;
        Ok(Self {
            inner,
            ctx: ForwardContext::new(
                String::new(),
                serde_json::Map::new(),
                false,
                "version".to_string(),
                "data".to_string(),
                false,
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestEntity {
        id: String,
        name: String,
    }

    #[test]
    fn test_forwardable_deref() {
        let entity = TestEntity {
            id: "1".to_string(),
            name: "test".to_string(),
        };
        let ctx = ForwardContext::new(
            "2.0.0".to_string(),
            serde_json::Map::new(),
            true,
            "version".to_string(),
            "data".to_string(),
            false,
        );
        let forwardable = Forwardable::new(entity, ctx);

        // Deref access
        assert_eq!(forwardable.id, "1");
        assert_eq!(forwardable.name, "test");
    }

    #[test]
    fn test_forwardable_deref_mut() {
        let entity = TestEntity {
            id: "1".to_string(),
            name: "test".to_string(),
        };
        let ctx = ForwardContext::new(
            "2.0.0".to_string(),
            serde_json::Map::new(),
            true,
            "version".to_string(),
            "data".to_string(),
            false,
        );
        let mut forwardable = Forwardable::new(entity, ctx);

        // DerefMut access
        forwardable.name = "updated".to_string();
        assert_eq!(forwardable.name, "updated");
    }

    #[test]
    fn test_forwardable_context_access() {
        let mut unknown = serde_json::Map::new();
        unknown.insert(
            "new_field".to_string(),
            serde_json::Value::String("value".to_string()),
        );

        let entity = TestEntity {
            id: "1".to_string(),
            name: "test".to_string(),
        };
        let ctx = ForwardContext::new(
            "2.0.0".to_string(),
            unknown,
            true,
            "version".to_string(),
            "data".to_string(),
            false,
        );
        let forwardable = Forwardable::new(entity, ctx);

        assert_eq!(forwardable.original_version(), "2.0.0");
        assert!(forwardable.was_lossy());
        assert_eq!(forwardable.unknown_fields().len(), 1);
        assert!(forwardable.unknown_fields().contains_key("new_field"));
    }

    #[test]
    fn test_forwardable_into_inner() {
        let entity = TestEntity {
            id: "1".to_string(),
            name: "test".to_string(),
        };
        let ctx = ForwardContext::new(
            "1.0.0".to_string(),
            serde_json::Map::new(),
            false,
            "version".to_string(),
            "data".to_string(),
            false,
        );
        let forwardable = Forwardable::new(entity.clone(), ctx);

        let inner = forwardable.into_inner();
        assert_eq!(inner, entity);
    }
}
