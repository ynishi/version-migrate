//! Pure path management crate for application config/data directories.

pub mod errors;
pub mod paths;

pub use errors::{IoOperationKind, StoreError};
pub use paths::{AppPaths, PathStrategy, PrefPath};
