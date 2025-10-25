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
- `#[derive(Versioned)]` procedural macro with semver validation
- Comprehensive error handling with `MigrationError`
- Integration tests demonstrating migration workflows

### Changed

### Deprecated

### Removed

### Fixed

### Security

[Unreleased]: https://github.com/yourusername/version-migrate/compare/...HEAD
