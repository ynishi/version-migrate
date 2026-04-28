//! Atomic file I/O helpers shared across storage types.
//!
//! Provides `get_temp_path`, `atomic_rename`, and `cleanup_temp_files` as
//! free functions so that `FileStorage`, `DirStorage`, and `AsyncDirStorage`
//! can all delegate to a single implementation instead of duplicating the
//! logic in each `impl` block.

use crate::errors::{IoOperationKind, StoreError};
use std::path::{Path, PathBuf};

/// Compute the path to a per-process temporary file for `target_path`.
///
/// The temporary file is placed in the same directory as `target_path` so
/// that the subsequent rename stays on the same filesystem.
///
/// Format: `<parent>/.<filename>.tmp.<pid>`
///
/// # Errors
///
/// `StoreError::IoError` if `target_path` has no parent directory or no
/// file-name component.
pub fn get_temp_path(target_path: &Path) -> Result<PathBuf, StoreError> {
    let parent = target_path.parent().ok_or_else(|| StoreError::IoError {
        operation: IoOperationKind::Create,
        path: target_path.display().to_string(),
        context: Some("path has no parent directory".to_string()),
        error: "cannot determine parent for temporary file".to_string(),
    })?;

    let file_name = target_path.file_name().ok_or_else(|| StoreError::IoError {
        operation: IoOperationKind::Create,
        path: target_path.display().to_string(),
        context: Some("path has no file name".to_string()),
        error: "cannot determine filename for temporary file".to_string(),
    })?;

    let tmp_name = format!(
        ".{}.tmp.{}",
        file_name.to_string_lossy(),
        std::process::id()
    );
    Ok(parent.join(tmp_name))
}

/// Rename `tmp_path` to `target_path` atomically, retrying up to
/// `retry_count` times with a 10 ms delay between attempts.
///
/// # Errors
///
/// `StoreError::IoError { operation: Rename, … }` after all retries are
/// exhausted.
pub fn atomic_rename(
    tmp_path: &Path,
    target_path: &Path,
    retry_count: usize,
) -> Result<(), StoreError> {
    let mut last_error = None;

    for attempt in 0..retry_count {
        match std::fs::rename(tmp_path, target_path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_error = Some(e);
                if attempt + 1 < retry_count {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    }

    Err(StoreError::IoError {
        operation: IoOperationKind::Rename,
        path: target_path.display().to_string(),
        context: Some(format!("after {} retries", retry_count)),
        error: last_error
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown error after retries".to_string()),
    })
}

/// Remove orphaned `.<filename>.tmp.*` files in the same directory as
/// `target_path`.
///
/// Errors are silently ignored (best-effort cleanup).
pub fn cleanup_temp_files(target_path: &Path) -> std::io::Result<()> {
    let parent = match target_path.parent() {
        Some(p) => p,
        None => return Ok(()),
    };

    let file_name = match target_path.file_name() {
        Some(f) => f.to_string_lossy().into_owned(),
        None => return Ok(()),
    };

    let prefix = format!(".{}.tmp.", file_name);

    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                if name.starts_with(&prefix) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Async variants
// ============================================================================

#[cfg(feature = "async")]
pub mod async_io {
    //! Async variants of the atomic I/O helpers.
    //!
    //! `get_temp_path` is synchronous and shared with the sync module;
    //! call `super::get_temp_path(target_path)` from async callers.

    use crate::errors::{IoOperationKind, StoreError};
    use std::path::Path;

    /// Rename `tmp_path` to `target_path` atomically (async), retrying up to
    /// `retry_count` times with a 10 ms `tokio::time::sleep` between attempts.
    ///
    /// # Errors
    ///
    /// `StoreError::IoError { operation: Rename, … }` after all retries are
    /// exhausted.
    pub async fn atomic_rename(
        tmp_path: &Path,
        target_path: &Path,
        retry_count: usize,
    ) -> Result<(), StoreError> {
        let mut last_error = None;

        for attempt in 0..retry_count {
            match tokio::fs::rename(tmp_path, target_path).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_error = Some(e);
                    if attempt + 1 < retry_count {
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    }
                }
            }
        }

        Err(StoreError::IoError {
            operation: IoOperationKind::Rename,
            path: target_path.display().to_string(),
            context: Some(format!("after {} retries (async)", retry_count)),
            error: last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error after retries".to_string()),
        })
    }

    /// Remove orphaned `.<filename>.tmp.*` files in the same directory as
    /// `target_path` (async).
    ///
    /// Errors are silently ignored (best-effort cleanup).
    pub async fn cleanup_temp_files(target_path: &Path) -> std::io::Result<()> {
        let parent = match target_path.parent() {
            Some(p) => p,
            None => return Ok(()),
        };

        let file_name = match target_path.file_name() {
            Some(f) => f.to_string_lossy().into_owned(),
            None => return Ok(()),
        };

        let prefix = format!(".{}.tmp.", file_name);

        let mut entries = match tokio::fs::read_dir(parent).await {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(name) = entry.file_name().into_string() {
                if name.starts_with(&prefix) {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_get_temp_path_format() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("config.toml");
        let tmp = get_temp_path(&target).unwrap();
        let name = tmp.file_name().unwrap().to_string_lossy();
        assert!(
            name.starts_with(".config.toml.tmp."),
            "tmp name should start with .<filename>.tmp., got: {}",
            name
        );
        assert_eq!(tmp.parent().unwrap(), dir.path());
    }

    #[test]
    fn test_get_temp_path_no_parent_errors() {
        // A path with no parent (root-relative bare name has parent "").
        // Use a path that genuinely has no parent component.
        let bare = std::path::PathBuf::from("/");
        // "/" has no file_name, so this should error.
        let result = get_temp_path(&bare);
        assert!(result.is_err(), "root path should produce an error");
    }

    #[test]
    fn test_atomic_rename_success() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.tmp");
        let dst = dir.path().join("dst.toml");
        fs::write(&src, "data").unwrap();
        atomic_rename(&src, &dst, 3).unwrap();
        assert!(dst.exists());
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "data");
    }

    #[test]
    fn test_atomic_rename_fails_when_src_missing() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("nonexistent.tmp");
        let dst = dir.path().join("dst.toml");
        let result = atomic_rename(&src, &dst, 1);
        assert!(result.is_err(), "should fail when src does not exist");
        if let Err(StoreError::IoError { operation, .. }) = result {
            assert_eq!(operation, IoOperationKind::Rename);
        } else {
            panic!("expected StoreError::IoError(Rename)");
        }
    }

    #[test]
    fn test_cleanup_temp_files_removes_matching() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("data.toml");

        let stale1 = dir.path().join(".data.toml.tmp.11111");
        let stale2 = dir.path().join(".data.toml.tmp.22222");
        let other = dir.path().join("other.toml");
        fs::write(&stale1, "s1").unwrap();
        fs::write(&stale2, "s2").unwrap();
        fs::write(&other, "keep").unwrap();

        cleanup_temp_files(&target).unwrap();

        assert!(!stale1.exists(), "stale1 should be removed");
        assert!(!stale2.exists(), "stale2 should be removed");
        assert!(other.exists(), "other.toml should be kept");
    }

    #[test]
    fn test_cleanup_temp_files_no_matches_is_ok() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("config.toml");
        // No .tmp files — should not error.
        cleanup_temp_files(&target).unwrap();
    }
}
