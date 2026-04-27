//! Error types for local store operations.

use std::fmt;
use thiserror::Error;

/// File I/O operation kind.
///
/// Identifies the specific type of I/O operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOperationKind {
    /// Reading from a file
    Read,
    /// Writing to a file
    Write,
    /// Creating a new file
    Create,
    /// Deleting a file
    Delete,
    /// Renaming/moving a file
    Rename,
    /// Creating a directory
    CreateDir,
    /// Reading directory contents
    ReadDir,
    /// Syncing file contents to disk
    Sync,
}

impl fmt::Display for IoOperationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Write => write!(f, "write"),
            Self::Create => write!(f, "create"),
            Self::Delete => write!(f, "delete"),
            Self::Rename => write!(f, "rename"),
            Self::CreateDir => write!(f, "create directory"),
            Self::ReadDir => write!(f, "read directory"),
            Self::Sync => write!(f, "sync"),
        }
    }
}

/// Format I/O error message with operation, path, context, and error details.
fn format_io_error(
    operation: &IoOperationKind,
    path: &str,
    context: &Option<String>,
    error: &str,
) -> String {
    if let Some(ctx) = context {
        format!("Failed to {} {} at '{}': {}", operation, ctx, path, error)
    } else {
        format!("Failed to {} file at '{}': {}", operation, path, error)
    }
}

/// Error types for path and store operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum StoreError {
    /// File I/O error with detailed operation context.
    ///
    /// Provides specific information about which I/O operation failed,
    /// along with optional context (e.g., "temporary file", "after 3 retries").
    #[error("{}", format_io_error(.operation, .path, .context, .error))]
    IoError {
        /// The I/O operation that failed.
        operation: IoOperationKind,
        /// The file path where the error occurred.
        path: String,
        /// Additional context (e.g., "temporary file", "after 3 retries").
        context: Option<String>,
        /// The underlying I/O error message.
        error: String,
    },

    /// Failed to find home directory.
    #[error("Cannot determine home directory")]
    HomeDirNotFound,

    /// Failed to encode or decode a filename for the given entity ID.
    ///
    /// Raised when a filename encoding strategy (Direct/UrlEncode/Base64) cannot
    /// encode the ID on write, or cannot decode the stored filename on read.
    ///
    /// # Arguments
    ///
    /// * `id` - The entity ID that could not be encoded/decoded.
    /// * `reason` - A human-readable explanation of the failure.
    #[error("Failed to encode filename for ID '{id}': {reason}")]
    FilenameEncoding {
        /// The entity ID involved in the encoding failure.
        id: String,
        /// Human-readable reason for the failure.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_error_io_error_display_without_context() {
        let err = StoreError::IoError {
            operation: IoOperationKind::Read,
            path: "/path/to/file.toml".to_string(),
            context: None,
            error: "Permission denied".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("Failed to read"));
        assert!(display.contains("/path/to/file.toml"));
        assert!(display.contains("Permission denied"));
    }

    #[test]
    fn test_store_error_io_error_display_with_context() {
        let err = StoreError::IoError {
            operation: IoOperationKind::Write,
            path: "/path/to/tmp.toml".to_string(),
            context: Some("temporary file".to_string()),
            error: "Disk full".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("Failed to write"));
        assert!(display.contains("temporary file"));
        assert!(display.contains("/path/to/tmp.toml"));
        assert!(display.contains("Disk full"));
    }

    #[test]
    fn test_store_error_home_dir_not_found_display() {
        let err = StoreError::HomeDirNotFound;
        let display = format!("{}", err);
        assert!(display.contains("Cannot determine home directory"));
    }

    #[test]
    fn test_store_error_is_std_error() {
        let err = StoreError::HomeDirNotFound;
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_io_operation_kind_display() {
        assert_eq!(IoOperationKind::Read.to_string(), "read");
        assert_eq!(IoOperationKind::Write.to_string(), "write");
        assert_eq!(IoOperationKind::Create.to_string(), "create");
        assert_eq!(IoOperationKind::Delete.to_string(), "delete");
        assert_eq!(IoOperationKind::Rename.to_string(), "rename");
        assert_eq!(IoOperationKind::CreateDir.to_string(), "create directory");
        assert_eq!(IoOperationKind::ReadDir.to_string(), "read directory");
        assert_eq!(IoOperationKind::Sync.to_string(), "sync");
    }

    #[test]
    fn test_store_error_filename_encoding_display() {
        let err = StoreError::FilenameEncoding {
            id: "my/id".to_string(),
            reason: "ID contains invalid characters for Direct encoding".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("my/id"), "display should contain id");
        assert!(
            display.contains("invalid characters"),
            "display should contain reason"
        );
    }

    #[test]
    fn test_store_error_io_error_rename_with_retries() {
        let err = StoreError::IoError {
            operation: IoOperationKind::Rename,
            path: "/path/to/file.toml".to_string(),
            context: Some("after 3 retries".to_string()),
            error: "Resource temporarily unavailable".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("Failed to rename"));
        assert!(display.contains("after 3 retries"));
        assert!(display.contains("/path/to/file.toml"));
        assert!(display.contains("Resource temporarily unavailable"));
    }
}
