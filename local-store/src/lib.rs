//! Pure path management and raw file storage crate for application config/data directories.

pub mod atomic_io;
pub mod dir_storage;
pub mod errors;
pub mod format_convert;
pub mod paths;
pub mod storage;

pub use dir_storage::{DirStorage, DirStorageStrategy, FilenameEncoding};
pub use errors::{IoOperationKind, StoreError};
pub use format_convert::{json_to_toml, FormatConvertError};
pub use paths::{AppPaths, PathStrategy, PrefPath};
pub use storage::{
    AtomicWriteConfig, FileStorage, FileStorageStrategy, FormatStrategy, LoadBehavior,
};

#[cfg(feature = "async")]
pub use dir_storage::AsyncDirStorage;
