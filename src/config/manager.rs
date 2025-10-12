//! Configuration manager for loading and saving application configuration
//!
//! This module provides functionality to load and save configuration to
//! %APPDATA%\EasyHDR\config.json with atomic writes to prevent corruption.

use crate::config::models::AppConfig;
use crate::error::{EasyHdrError, Result};
use std::path::PathBuf;
use tracing::{info, warn};

/// Configuration manager
pub struct ConfigManager;

impl ConfigManager {
    /// Get the path to the configuration file
    ///
    /// Returns: %APPDATA%\EasyHDR\config.json
    pub fn get_config_path() -> PathBuf {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata).join("EasyHDR").join("config.json")
    }

    /// Ensure the configuration directory exists
    ///
    /// Creates %APPDATA%\EasyHDR if it doesn't exist
    pub fn ensure_config_dir() -> Result<PathBuf> {
        let config_path = Self::get_config_path();
        let config_dir = config_path
            .parent()
            .ok_or_else(|| EasyHdrError::ConfigError("Invalid config path".to_string()))?;
        
        std::fs::create_dir_all(config_dir)?;
        Ok(config_dir.to_path_buf())
    }

    /// Load configuration from disk
    ///
    /// If the configuration file doesn't exist or is corrupt, returns default configuration.
    pub fn load() -> Result<AppConfig> {
        let config_path = Self::get_config_path();
        
        if !config_path.exists() {
            info!("Configuration file not found, using defaults");
            return Ok(AppConfig::default());
        }
        
        let json = std::fs::read_to_string(&config_path)?;
        
        match serde_json::from_str(&json) {
            Ok(config) => {
                info!("Configuration loaded successfully");
                Ok(config)
            }
            Err(e) => {
                warn!("Failed to parse configuration, using defaults: {}", e);
                Ok(AppConfig::default())
            }
        }
    }

    /// Save configuration to disk with atomic write
    ///
    /// Uses a temporary file and rename to ensure atomic write operation.
    pub fn save(config: &AppConfig) -> Result<()> {
        let config_path = Self::get_config_path();
        Self::ensure_config_dir()?;
        
        let config_dir = config_path
            .parent()
            .ok_or_else(|| EasyHdrError::ConfigError("Invalid config path".to_string()))?;
        
        // Atomic write: write to temp file, then rename
        let temp_path = config_dir.join("config.json.tmp");
        let json = serde_json::to_string_pretty(config)?;
        std::fs::write(&temp_path, json)?;
        std::fs::rename(temp_path, config_path)?;
        
        info!("Configuration saved successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_path() {
        let path = ConfigManager::get_config_path();
        assert!(path.to_string_lossy().contains("EasyHDR"));
        assert!(path.to_string_lossy().ends_with("config.json"));
    }

    #[test]
    fn test_load_missing_config() {
        // This should return default config without error
        let config = ConfigManager::load();
        assert!(config.is_ok());
    }
}

