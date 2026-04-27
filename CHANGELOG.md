# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - ReleaseDate

### Added
- New `local-store` crate extracted from `version-migrate` as a standalone path and storage primitive
  - `AppPaths`: platform-agnostic path resolver (Linux/macOS/Windows) with configurable strategies
  - `PathStrategy`: enum for path resolution mode (`System`, `Xdg`, `CustomBase`)
  - `PrefPath`: preference-path helper built on `AppPaths`
  - `StoreError`: dedicated error type for path/IO operations (variants: `HomeDirNotFound`, `IoError { operation, path, context, error }`)
  - `IoOperationKind`: enum classifying IO operation kinds (8 variants with `Display` impl)
- `version-migrate` re-exports `AppPaths`, `PathStrategy`, `PrefPath`, `StoreError`, and `IoOperationKind` from `local-store` for backward-compatible import paths

### Removed
- `pub mod paths` module removed from `version-migrate`; callers using `version_migrate::paths::AppPaths` must migrate to `version_migrate::AppPaths` (pre-1.0 breaking change, SemVer minor bump 0.19 → 0.20)

### Changed
- **BREAKING**: `MigrationError::IoError` and `MigrationError::HomeDirNotFound` variants removed. Replaced by `MigrationError::Store(StoreError)` via `#[from] StoreError` wiring. Pattern matches on these variants must be updated to `MigrationError::Store(StoreError::IoError { .. })` and `MigrationError::Store(StoreError::HomeDirNotFound)` respectively.

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
- Async support for migrations requiring I/O operations
  - `AsyncMigratesTo<T>` trait for async migrations
  - `AsyncIntoDomain<D>` trait for async domain conversions
  - Support for database queries, API calls, and other async operations
- 47 tests (32 unit + 12 integration + 3 async) covering all functionality

### Changed
- `Migrator::register()` now returns `Result<(), MigrationError>` instead of `()` to support validation errors

### Deprecated

### Removed

### Fixed

### Security

[Unreleased]: https://github.com/yourusername/version-migrate/compare/...HEAD
