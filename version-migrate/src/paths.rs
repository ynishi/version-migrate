//! Platform-agnostic path management for application configuration and data.
//!
//! Provides unified path resolution strategies across different platforms.

use crate::{errors::IoOperationKind, MigrationError};
use std::path::PathBuf;

/// Path resolution strategy.
///
/// Determines how configuration and data directories are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStrategy {
    /// Use OS-standard directories (default).
    ///
    /// - Linux:   `~/.config/` (XDG_CONFIG_HOME)
    /// - macOS:   `~/Library/Application Support/`
    /// - Windows: `%APPDATA%`
    System,

    /// Force XDG Base Directory specification on all platforms.
    ///
    /// Uses `~/.config/` for config and `~/.local/share/` for data
    /// on all platforms (Linux, macOS, Windows).
    ///
    /// This is useful for applications that want consistent paths
    /// across platforms (e.g., VSCode, Neovim, orcs).
    Xdg,

    /// Use a custom base directory.
    ///
    /// All paths will be resolved relative to this base directory.
    CustomBase(PathBuf),
}

impl Default for PathStrategy {
    fn default() -> Self {
        Self::System
    }
}

/// Application path manager with configurable resolution strategies.
///
/// Provides platform-agnostic path resolution for configuration and data directories.
///
/// # Example
///
/// ```ignore
/// use version_migrate::{AppPaths, PathStrategy};
///
/// // Use OS-standard directories (default)
/// let paths = AppPaths::new("myapp");
/// let config_path = paths.config_file("config.toml")?;
///
/// // Force XDG on all platforms
/// let paths = AppPaths::new("myapp")
///     .config_strategy(PathStrategy::Xdg);
/// let config_path = paths.config_file("config.toml")?;
///
/// // Use custom base directory
/// let paths = AppPaths::new("myapp")
///     .config_strategy(PathStrategy::CustomBase("/opt/myapp".into()));
/// ```
#[derive(Debug, Clone)]
pub struct AppPaths {
    app_name: String,
    config_strategy: PathStrategy,
    data_strategy: PathStrategy,
}

impl AppPaths {
    /// Create a new path manager for the given application name.
    ///
    /// Uses `System` strategy by default for both config and data.
    ///
    /// # Arguments
    ///
    /// * `app_name` - Application name (used as subdirectory name)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let paths = AppPaths::new("myapp");
    /// ```
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            config_strategy: PathStrategy::default(),
            data_strategy: PathStrategy::default(),
        }
    }

    /// Set the configuration directory resolution strategy.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let paths = AppPaths::new("myapp")
    ///     .config_strategy(PathStrategy::Xdg);
    /// ```
    pub fn config_strategy(mut self, strategy: PathStrategy) -> Self {
        self.config_strategy = strategy;
        self
    }

    /// Set the data directory resolution strategy.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let paths = AppPaths::new("myapp")
    ///     .data_strategy(PathStrategy::Xdg);
    /// ```
    pub fn data_strategy(mut self, strategy: PathStrategy) -> Self {
        self.data_strategy = strategy;
        self
    }

    /// Get the configuration directory path.
    ///
    /// Creates the directory if it doesn't exist.
    ///
    /// # Returns
    ///
    /// The resolved configuration directory path.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::HomeDirNotFound` if the home directory cannot be determined.
    /// Returns `MigrationError::IoError` if directory creation fails.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let config_dir = paths.config_dir()?;
    /// // On Linux with System strategy: ~/.config/myapp
    /// // On macOS with System strategy: ~/Library/Application Support/myapp
    /// ```
    pub fn config_dir(&self) -> Result<PathBuf, MigrationError> {
        let dir = self.resolve_config_dir()?;
        self.ensure_dir_exists(&dir)?;
        Ok(dir)
    }

    /// Get the data directory path.
    ///
    /// Creates the directory if it doesn't exist.
    ///
    /// # Returns
    ///
    /// The resolved data directory path.
    ///
    /// # Errors
    ///
    /// Returns `MigrationError::HomeDirNotFound` if the home directory cannot be determined.
    /// Returns `MigrationError::IoError` if directory creation fails.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let data_dir = paths.data_dir()?;
    /// // On Linux with System strategy: ~/.local/share/myapp
    /// // On macOS with System strategy: ~/Library/Application Support/myapp
    /// ```
    pub fn data_dir(&self) -> Result<PathBuf, MigrationError> {
        let dir = self.resolve_data_dir()?;
        self.ensure_dir_exists(&dir)?;
        Ok(dir)
    }

    /// Get a configuration file path.
    ///
    /// This is a convenience method that joins the filename to the config directory.
    /// Creates the parent directory if it doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `filename` - The configuration file name
    ///
    /// # Example
    ///
    /// ```ignore
    /// let config_file = paths.config_file("config.toml")?;
    /// // On Linux with System strategy: ~/.config/myapp/config.toml
    /// ```
    pub fn config_file(&self, filename: &str) -> Result<PathBuf, MigrationError> {
        Ok(self.config_dir()?.join(filename))
    }

    /// Get a data file path.
    ///
    /// This is a convenience method that joins the filename to the data directory.
    /// Creates the parent directory if it doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `filename` - The data file name
    ///
    /// # Example
    ///
    /// ```ignore
    /// let data_file = paths.data_file("cache.db")?;
    /// // On Linux with System strategy: ~/.local/share/myapp/cache.db
    /// ```
    pub fn data_file(&self, filename: &str) -> Result<PathBuf, MigrationError> {
        Ok(self.data_dir()?.join(filename))
    }

    /// Resolve the configuration directory path based on the strategy.
    fn resolve_config_dir(&self) -> Result<PathBuf, MigrationError> {
        match &self.config_strategy {
            PathStrategy::System => {
                // Use OS-standard config directory
                let base = dirs::config_dir().ok_or(MigrationError::HomeDirNotFound)?;
                Ok(base.join(&self.app_name))
            }
            PathStrategy::Xdg => {
                // Force XDG on all platforms
                let home = dirs::home_dir().ok_or(MigrationError::HomeDirNotFound)?;
                Ok(home.join(".config").join(&self.app_name))
            }
            PathStrategy::CustomBase(base) => Ok(base.join(&self.app_name)),
        }
    }

    /// Resolve the data directory path based on the strategy.
    fn resolve_data_dir(&self) -> Result<PathBuf, MigrationError> {
        match &self.data_strategy {
            PathStrategy::System => {
                // Use OS-standard data directory
                let base = dirs::data_dir().ok_or(MigrationError::HomeDirNotFound)?;
                Ok(base.join(&self.app_name))
            }
            PathStrategy::Xdg => {
                // Force XDG on all platforms
                let home = dirs::home_dir().ok_or(MigrationError::HomeDirNotFound)?;
                Ok(home.join(".local/share").join(&self.app_name))
            }
            PathStrategy::CustomBase(base) => Ok(base.join("data").join(&self.app_name)),
        }
    }

    /// Ensure a directory exists, creating it if necessary.
    fn ensure_dir_exists(&self, path: &PathBuf) -> Result<(), MigrationError> {
        if !path.exists() {
            std::fs::create_dir_all(path).map_err(|e| MigrationError::IoError {
                operation: IoOperationKind::CreateDir,
                path: path.display().to_string(),
                context: None,
                error: e.to_string(),
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_path_strategy_default() {
        assert_eq!(PathStrategy::default(), PathStrategy::System);
    }

    #[test]
    fn test_app_paths_new() {
        let paths = AppPaths::new("testapp");
        assert_eq!(paths.app_name, "testapp");
        assert_eq!(paths.config_strategy, PathStrategy::System);
        assert_eq!(paths.data_strategy, PathStrategy::System);
    }

    #[test]
    fn test_app_paths_builder() {
        let paths = AppPaths::new("testapp")
            .config_strategy(PathStrategy::Xdg)
            .data_strategy(PathStrategy::Xdg);

        assert_eq!(paths.config_strategy, PathStrategy::Xdg);
        assert_eq!(paths.data_strategy, PathStrategy::Xdg);
    }

    #[test]
    fn test_system_strategy_config_dir() {
        let paths = AppPaths::new("testapp").config_strategy(PathStrategy::System);
        let config_dir = paths.resolve_config_dir().unwrap();

        // Should end with app name
        assert!(config_dir.ends_with("testapp"));

        // On Unix-like systems, should be under config dir
        #[cfg(unix)]
        {
            let home = dirs::home_dir().unwrap();
            // macOS uses Library/Application Support, Linux uses .config
            assert!(
                config_dir.starts_with(home.join("Library/Application Support"))
                    || config_dir.starts_with(home.join(".config"))
            );
        }
    }

    #[test]
    fn test_xdg_strategy_config_dir() {
        let paths = AppPaths::new("testapp").config_strategy(PathStrategy::Xdg);
        let config_dir = paths.resolve_config_dir().unwrap();

        // Should be ~/.config/testapp on all platforms
        let home = dirs::home_dir().unwrap();
        assert_eq!(config_dir, home.join(".config/testapp"));
    }

    #[test]
    fn test_xdg_strategy_data_dir() {
        let paths = AppPaths::new("testapp").data_strategy(PathStrategy::Xdg);
        let data_dir = paths.resolve_data_dir().unwrap();

        // Should be ~/.local/share/testapp on all platforms
        let home = dirs::home_dir().unwrap();
        assert_eq!(data_dir, home.join(".local/share/testapp"));
    }

    #[test]
    fn test_custom_base_strategy() {
        let temp_dir = TempDir::new().unwrap();
        let custom_base = temp_dir.path().to_path_buf();

        let paths = AppPaths::new("testapp")
            .config_strategy(PathStrategy::CustomBase(custom_base.clone()))
            .data_strategy(PathStrategy::CustomBase(custom_base.clone()));

        let config_dir = paths.resolve_config_dir().unwrap();
        let data_dir = paths.resolve_data_dir().unwrap();

        assert_eq!(config_dir, custom_base.join("testapp"));
        assert_eq!(data_dir, custom_base.join("data/testapp"));
    }

    #[test]
    fn test_config_file() {
        let temp_dir = TempDir::new().unwrap();
        let custom_base = temp_dir.path().to_path_buf();

        let paths =
            AppPaths::new("testapp").config_strategy(PathStrategy::CustomBase(custom_base.clone()));

        let config_file = paths.config_file("config.toml").unwrap();
        assert_eq!(config_file, custom_base.join("testapp/config.toml"));

        // Verify directory was created
        assert!(custom_base.join("testapp").exists());
    }

    #[test]
    fn test_data_file() {
        let temp_dir = TempDir::new().unwrap();
        let custom_base = temp_dir.path().to_path_buf();

        let paths =
            AppPaths::new("testapp").data_strategy(PathStrategy::CustomBase(custom_base.clone()));

        let data_file = paths.data_file("cache.db").unwrap();
        assert_eq!(data_file, custom_base.join("data/testapp/cache.db"));

        // Verify directory was created
        assert!(custom_base.join("data/testapp").exists());
    }

    #[test]
    fn test_ensure_dir_exists() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("nested/test/path");

        let paths = AppPaths::new("testapp");
        paths.ensure_dir_exists(&test_path).unwrap();

        assert!(test_path.exists());
        assert!(test_path.is_dir());
    }

    #[test]
    fn test_multiple_calls_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let custom_base = temp_dir.path().to_path_buf();

        let paths =
            AppPaths::new("testapp").config_strategy(PathStrategy::CustomBase(custom_base.clone()));

        // Call config_dir multiple times
        let dir1 = paths.config_dir().unwrap();
        let dir2 = paths.config_dir().unwrap();
        let dir3 = paths.config_dir().unwrap();

        assert_eq!(dir1, dir2);
        assert_eq!(dir2, dir3);
    }
}
