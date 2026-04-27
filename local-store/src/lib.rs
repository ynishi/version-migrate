//! Pure path management and raw file storage crate for application config/data directories.

pub mod dir_storage;
pub mod errors;
pub mod paths;
pub mod storage;

pub use dir_storage::{DirStorage, DirStorageStrategy, FilenameEncoding};
pub use errors::{IoOperationKind, StoreError};
pub use paths::{AppPaths, PathStrategy, PrefPath};
pub use storage::{
    AtomicWriteConfig, FileStorage, FileStorageStrategy, FormatStrategy, LoadBehavior,
};

#[cfg(feature = "async")]
pub use dir_storage::AsyncDirStorage;
