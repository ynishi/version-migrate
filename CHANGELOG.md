# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - ReleaseDate

### Added
- Initial implementation of `version-migrate` core library
- `Versioned` trait for marking versioned data schemas
- `MigratesTo<T>` trait for explicit migration logic
- `IntoDomain<D>` trait for converting to domain models
- `VersionedWrapper<T>` for serializing data with version information
- `Migrator` with type-safe builder pattern for defining migration paths
  - `load()` method for deserializing and migrating JSON data
  - `load_from()` method for loading from any serde-compatible format (TOML, YAML, etc.)
  - `save()` method for serializing versioned data to JSON
- `#[derive(Versioned)]` procedural macro with semver validation
- Comprehensive error handling with `MigrationError` enum using `thiserror`
  - `DeserializationError` for JSON parsing failures
  - `SerializationError` for JSON serialization failures
  - `EntityNotFound` for unregistered entities
  - `MigrationPathNotDefined` for missing migration paths
  - `MigrationStepFailed` for migration execution failures
  - `CircularMigrationPath` for detecting circular migration paths
  - `InvalidVersionOrder` for semver ordering violations
- Separated error types into dedicated `errors` module
- Serialization format flexibility - support for TOML, YAML, and any serde-compatible format
- Migration path validation
  - Automatic validation when registering migration paths
  - Circular migration path detection
  - Semantic versioning order validation
- 44 tests (32 unit + 12 integration) covering all functionality including validation

### Changed
- `Migrator::register()` now returns `Result<(), MigrationError>` instead of `()` to support validation errors

### Deprecated

### Removed

### Fixed

### Security

[Unreleased]: https://github.com/yourusername/version-migrate/compare/...HEAD
