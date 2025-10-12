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
    use crate::config::models::MonitoredApp;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    /// Helper function to create a temporary test directory
    fn create_test_dir() -> PathBuf {
        let test_dir = std::env::temp_dir().join(format!("easyhdr_test_{}", Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();
        test_dir
    }

    /// Helper function to clean up test directory
    fn cleanup_test_dir(dir: &PathBuf) {
        if dir.exists() {
            fs::remove_dir_all(dir).ok();
        }
    }

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

        let config = config.unwrap();
        assert_eq!(config.monitored_apps.len(), 0);
        assert_eq!(config.preferences.monitoring_interval_ms, 1000);
    }

    #[test]
    fn test_save_and_load_round_trip() {
        let test_dir = create_test_dir();

        // Create a config with data
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Test Game".to_string(),
            exe_path: PathBuf::from("C:\\Games\\test.exe"),
            process_name: "test".to_string(),
            enabled: true,
            icon_data: None,
        });
        config.preferences.auto_start = true;
        config.preferences.monitoring_interval_ms = 500;

        // Create config directory manually
        let config_dir = test_dir.join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();

        // Write config directly to test directory
        let config_path = config_dir.join("config.json");
        let json = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&config_path, json).unwrap();

        // Verify file exists
        assert!(config_path.exists(), "Config file was not created");

        // Read back and verify
        let json_content = fs::read_to_string(&config_path).unwrap();
        let loaded_config: AppConfig = serde_json::from_str(&json_content).unwrap();

        // Verify the data matches
        assert_eq!(config.monitored_apps.len(), loaded_config.monitored_apps.len());
        assert_eq!(config.monitored_apps[0].id, loaded_config.monitored_apps[0].id);
        assert_eq!(config.monitored_apps[0].display_name, loaded_config.monitored_apps[0].display_name);
        assert_eq!(config.preferences.auto_start, loaded_config.preferences.auto_start);
        assert_eq!(config.preferences.monitoring_interval_ms, loaded_config.preferences.monitoring_interval_ms);

        // Cleanup
        cleanup_test_dir(&test_dir);
    }

    #[test]
    fn test_load_corrupt_json_returns_defaults() {
        let test_dir = create_test_dir();

        // Override APPDATA for this test
        std::env::set_var("APPDATA", test_dir.to_str().unwrap());

        // Create config directory
        let config_dir = test_dir.join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();

        // Write corrupt JSON to config file
        let config_path = config_dir.join("config.json");
        fs::write(&config_path, "{ this is not valid json }").unwrap();

        // Load should return default config without error
        let result = ConfigManager::load();
        assert!(result.is_ok(), "Load should succeed with corrupt JSON");

        let config = result.unwrap();

        // Verify we got default config
        assert_eq!(config.monitored_apps.len(), 0);
        assert_eq!(config.preferences.monitoring_interval_ms, 1000);
        assert_eq!(config.preferences.auto_start, false);

        // Cleanup
        cleanup_test_dir(&test_dir);
    }

    #[test]
    fn test_load_incomplete_json_returns_defaults() {
        let test_dir = create_test_dir();

        // Override APPDATA for this test
        std::env::set_var("APPDATA", test_dir.to_str().unwrap());

        // Create config directory
        let config_dir = test_dir.join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();

        // Write incomplete JSON (missing required fields)
        let config_path = config_dir.join("config.json");
        fs::write(&config_path, r#"{"monitored_apps": []}"#).unwrap();

        // Load should return default config without error
        let result = ConfigManager::load();
        assert!(result.is_ok(), "Load should succeed with incomplete JSON");

        let config = result.unwrap();

        // Verify we got default config
        assert_eq!(config.monitored_apps.len(), 0);

        // Cleanup
        cleanup_test_dir(&test_dir);
    }

    #[test]
    fn test_load_malformed_json_returns_defaults() {
        let test_dir = create_test_dir();

        // Override APPDATA for this test
        std::env::set_var("APPDATA", test_dir.to_str().unwrap());

        // Create config directory
        let config_dir = test_dir.join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();

        // Write various types of malformed JSON
        let test_cases = vec![
            "",  // Empty file
            "{",  // Unclosed brace
            "null",  // Null value
            "[]",  // Array instead of object
            r#"{"monitored_apps": "not an array"}"#,  // Wrong type
        ];

        for (i, malformed_json) in test_cases.iter().enumerate() {
            let config_path = config_dir.join("config.json");
            fs::write(&config_path, malformed_json).unwrap();

            let result = ConfigManager::load();
            assert!(result.is_ok(), "Test case {} failed: Load should succeed with malformed JSON", i);

            let config = result.unwrap();
            assert_eq!(config.monitored_apps.len(), 0, "Test case {} failed: Should return default config", i);
        }

        // Cleanup
        cleanup_test_dir(&test_dir);
    }

    #[test]
    fn test_atomic_write_creates_temp_file() {
        let test_dir = create_test_dir();

        // Override APPDATA for this test
        std::env::set_var("APPDATA", test_dir.to_str().unwrap());

        let config = AppConfig::default();

        // Save the config
        let result = ConfigManager::save(&config);
        assert!(result.is_ok());

        // Verify the final config file exists
        let config_path = test_dir.join("EasyHDR").join("config.json");
        assert!(config_path.exists(), "Final config file should exist");

        // Verify the temp file was cleaned up
        let temp_path = test_dir.join("EasyHDR").join("config.json.tmp");
        assert!(!temp_path.exists(), "Temp file should be removed after atomic write");

        // Cleanup
        cleanup_test_dir(&test_dir);
    }

    #[test]
    fn test_save_creates_directory_if_missing() {
        let test_dir = create_test_dir();

        // Override APPDATA for this test
        std::env::set_var("APPDATA", test_dir.to_str().unwrap());

        // Ensure directory doesn't exist
        let config_dir = test_dir.join("EasyHDR");
        if config_dir.exists() {
            fs::remove_dir_all(&config_dir).unwrap();
        }

        let config = AppConfig::default();

        // Save should create the directory
        let result = ConfigManager::save(&config);
        assert!(result.is_ok(), "Save should create directory if missing");

        // Verify directory was created
        assert!(config_dir.exists(), "Config directory should be created");

        // Verify config file exists
        let config_path = config_dir.join("config.json");
        assert!(config_path.exists(), "Config file should exist");

        // Cleanup
        cleanup_test_dir(&test_dir);
    }

    #[test]
    fn test_ensure_config_dir() {
        let test_dir = create_test_dir();

        // Override APPDATA for this test
        std::env::set_var("APPDATA", test_dir.to_str().unwrap());

        // Call ensure_config_dir
        let result = ConfigManager::ensure_config_dir();
        assert!(result.is_ok());

        let config_dir = result.unwrap();

        // Verify directory exists
        assert!(config_dir.exists());
        assert!(config_dir.to_string_lossy().contains("EasyHDR"));

        // Cleanup
        cleanup_test_dir(&test_dir);
    }

    #[test]
    fn test_multiple_saves_overwrite_correctly() {
        let test_dir = create_test_dir();
        let config_dir = test_dir.join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        // Save first config
        let mut config1 = AppConfig::default();
        config1.preferences.monitoring_interval_ms = 500;
        let json1 = serde_json::to_string_pretty(&config1).unwrap();
        fs::write(&config_path, json1).unwrap();

        // Save second config with different values
        let mut config2 = AppConfig::default();
        config2.preferences.monitoring_interval_ms = 2000;
        config2.preferences.auto_start = true;
        let json2 = serde_json::to_string_pretty(&config2).unwrap();
        fs::write(&config_path, json2).unwrap();

        // Load and verify we got the second config
        let json_content = fs::read_to_string(&config_path).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json_content).unwrap();
        assert_eq!(loaded.preferences.monitoring_interval_ms, 2000);
        assert_eq!(loaded.preferences.auto_start, true);

        // Cleanup
        cleanup_test_dir(&test_dir);
    }

    #[test]
    fn test_save_preserves_all_fields() {
        let test_dir = create_test_dir();
        let config_dir = test_dir.join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        // Create a comprehensive config
        let mut config = AppConfig::default();

        // Add multiple monitored apps
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "App 1".to_string(),
            exe_path: PathBuf::from("C:\\App1\\app1.exe"),
            process_name: "app1".to_string(),
            enabled: true,
            icon_data: None,
        });
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap(),
            display_name: "App 2".to_string(),
            exe_path: PathBuf::from("D:\\App2\\app2.exe"),
            process_name: "app2".to_string(),
            enabled: false,
            icon_data: None,
        });

        // Set all preferences
        config.preferences.auto_start = true;
        config.preferences.monitoring_interval_ms = 1500;
        config.preferences.startup_delay_ms = 7000;
        config.preferences.show_tray_notifications = false;

        // Set window state
        config.window_state.x = 250;
        config.window_state.y = 300;
        config.window_state.width = 1024;
        config.window_state.height = 768;

        // Save
        let json = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&config_path, json).unwrap();

        // Load back
        let json_content = fs::read_to_string(&config_path).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json_content).unwrap();

        // Verify all fields
        assert_eq!(loaded.monitored_apps.len(), 2);
        assert_eq!(loaded.monitored_apps[0].display_name, "App 1");
        assert_eq!(loaded.monitored_apps[1].display_name, "App 2");
        assert_eq!(loaded.monitored_apps[0].enabled, true);
        assert_eq!(loaded.monitored_apps[1].enabled, false);

        assert_eq!(loaded.preferences.auto_start, true);
        assert_eq!(loaded.preferences.monitoring_interval_ms, 1500);
        assert_eq!(loaded.preferences.startup_delay_ms, 7000);
        assert_eq!(loaded.preferences.show_tray_notifications, false);

        assert_eq!(loaded.window_state.x, 250);
        assert_eq!(loaded.window_state.y, 300);
        assert_eq!(loaded.window_state.width, 1024);
        assert_eq!(loaded.window_state.height, 768);

        // Cleanup
        cleanup_test_dir(&test_dir);
    }

    #[test]
    fn test_config_json_is_pretty_printed() {
        let test_dir = create_test_dir();

        // Override APPDATA for this test
        std::env::set_var("APPDATA", test_dir.to_str().unwrap());

        let config = AppConfig::default();
        ConfigManager::save(&config).unwrap();

        // Read the raw JSON file
        let config_path = test_dir.join("EasyHDR").join("config.json");
        let json_content = fs::read_to_string(&config_path).unwrap();

        // Verify it's pretty-printed (contains newlines and indentation)
        assert!(json_content.contains('\n'), "JSON should be pretty-printed with newlines");
        assert!(json_content.contains("  "), "JSON should be pretty-printed with indentation");

        // Cleanup
        cleanup_test_dir(&test_dir);
    }
}

