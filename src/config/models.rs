//! Configuration data models
//!
//! This module defines the data structures used for application configuration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Represents a monitored application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredApp {
    /// Unique identifier for this application entry
    pub id: Uuid,
    /// Display name shown in the UI
    pub display_name: String,
    /// Full path to the executable
    pub exe_path: PathBuf,
    /// Process name (extracted from exe filename, lowercase)
    pub process_name: String,
    /// Whether monitoring is enabled for this application
    pub enabled: bool,
    /// Cached icon data (not persisted to config file)
    #[serde(skip)]
    pub icon_data: Option<Vec<u8>>,
}

/// Top-level application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// List of monitored applications
    pub monitored_apps: Vec<MonitoredApp>,
    /// User preferences
    pub preferences: UserPreferences,
    /// Window state for persistence
    pub window_state: WindowState,
}

/// User preferences and settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    /// Whether to auto-start on Windows login
    pub auto_start: bool,
    /// Process monitoring interval in milliseconds (500-2000)
    pub monitoring_interval_ms: u64,
    /// Startup delay in milliseconds (0-10000)
    pub startup_delay_ms: u64,
    /// Whether to show tray notifications on HDR changes
    pub show_tray_notifications: bool,
}

/// Window state for position and size persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    /// X position
    pub x: i32,
    /// Y position
    pub y: i32,
    /// Window width
    pub width: u32,
    /// Window height
    pub height: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            monitored_apps: Vec::new(),
            preferences: UserPreferences::default(),
            window_state: WindowState::default(),
        }
    }
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            auto_start: false,
            monitoring_interval_ms: 1000,
            startup_delay_ms: 3000,
            show_tray_notifications: true,
        }
    }
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: 100,
            y: 100,
            width: 600,
            height: 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.monitored_apps.len(), 0);
        assert_eq!(config.preferences.monitoring_interval_ms, 1000);
    }

    #[test]
    fn test_serialization() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.preferences.auto_start, deserialized.preferences.auto_start);
    }
}

