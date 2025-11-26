//! Configuration management for FlakeCache CLI
//!
//! Handles loading, validating, and persisting CLI configuration including
//! credentials, cache settings, and user preferences.

use crate::error::{CliError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub mod auth;
pub mod defaults;

pub use auth::AuthConfig;
pub use defaults::*;

/// Main CLI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Authentication configuration
    pub auth: AuthConfig,

    /// Default cache name
    pub default_cache: Option<String>,

    /// API server URL
    #[serde(default = "defaults::default_api_url")]
    pub api_url: String,

    /// Enable verbose logging
    #[serde(default)]
    pub verbose: bool,

    /// Connection timeout in seconds
    #[serde(default = "defaults::default_timeout")]
    pub timeout_secs: u64,

    /// Maximum parallel uploads/downloads
    #[serde(default = "defaults::default_parallelism")]
    pub parallelism: usize,
}

impl Config {
    /// Load configuration from default location
    ///
    /// Tries in order:
    /// 1. XDG_CONFIG_HOME/flakecache/config.toml
    /// 2. ~/.config/flakecache/config.toml
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Err(CliError::NoConfig);
        }
        Self::load_from(&path)
    }

    /// Load configuration from a specific path
    pub fn load_from(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path).map_err(|e| CliError::ConfigRead {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        toml::from_str(&contents).map_err(|e| CliError::InvalidConfig(e.to_string()))
    }

    /// Save configuration to default location
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        self.save_to(&path)
    }

    /// Save configuration to a specific path
    pub fn save_to(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| CliError::DirError {
                path: parent.to_path_buf(),
                reason: e.to_string(),
            })?;
        }

        let contents =
            toml::to_string_pretty(self).map_err(|e| CliError::SerializationError(e.to_string()))?;

        fs::write(path, contents).map_err(|e| CliError::ConfigWrite {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        // Set restrictive permissions on config file (contains credentials)
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, Permissions::from_mode(0o600)).map_err(|e| {
                CliError::ConfigWrite {
                    path: path.to_path_buf(),
                    reason: format!("Failed to set permissions: {e}"),
                }
            })?;
        }

        Ok(())
    }

    /// Get the path to the config file
    pub fn config_path() -> Result<PathBuf> {
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .and_then(|path| if path.is_empty() { None } else { Some(path) })
            .or_else(|| {
                dirs::home_dir().map(|home| {
                    home.join(".config").to_string_lossy().to_string()
                })
            });

        config_home
            .ok_or_else(|| {
                CliError::Internal(
                    "Could not determine config directory: XDG_CONFIG_HOME not set and no home directory found"
                        .to_string(),
                )
            })
            .map(|path| PathBuf::from(path).join("flakecache").join("config.toml"))
    }

    /// Get cache directory path
    pub fn cache_dir() -> Result<PathBuf> {
        let cache_home = std::env::var("XDG_CACHE_HOME")
            .ok()
            .and_then(|path| if path.is_empty() { None } else { Some(path) })
            .or_else(|| {
                dirs::home_dir().map(|home| {
                    home.join(".cache").to_string_lossy().to_string()
                })
            });

        cache_home
            .ok_or_else(|| {
                CliError::Internal(
                    "Could not determine cache directory: XDG_CACHE_HOME not set and no home directory found"
                        .to_string(),
                )
            })
            .map(|path| PathBuf::from(path).join("flakecache"))
    }

    /// Merge another config into this one, with other taking precedence
    pub fn merge(&mut self, other: &Config) {
        if !other.auth.token.is_empty() {
            self.auth.token.clone_from(&other.auth.token);
        }
        if !other.auth.refresh_token.is_empty() {
            self.auth.refresh_token.clone_from(&other.auth.refresh_token);
        }
        if let Some(cache) = &other.default_cache {
            self.default_cache = Some(cache.clone());
        }
        if other.api_url != default_api_url() {
            self.api_url.clone_from(&other.api_url);
        }
        if other.verbose {
            self.verbose = true;
        }
        if other.timeout_secs != default_timeout() {
            self.timeout_secs = other.timeout_secs;
        }
        if other.parallelism != default_parallelism() {
            self.parallelism = other.parallelism;
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Ensure API URL is not empty
        if self.api_url.is_empty() {
            return Err(CliError::InvalidConfig("api_url cannot be empty".to_string()));
        }

        // Validate timeout
        if self.timeout_secs == 0 {
            return Err(CliError::InvalidConfig(
                "timeout_secs must be greater than 0".to_string(),
            ));
        }

        // Validate parallelism
        if self.parallelism == 0 {
            return Err(CliError::InvalidConfig(
                "parallelism must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auth: AuthConfig::default(),
            default_cache: None,
            api_url: default_api_url(),
            verbose: false,
            timeout_secs: default_timeout(),
            parallelism: default_parallelism(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.api_url, "https://c.flakecache.com");
        assert_eq!(config.timeout_secs, 300);
        assert!(config.timeout_secs > 0);
    }

    #[test]
    fn test_config_validation() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }
}
