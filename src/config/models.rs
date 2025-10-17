//! Configuration data models
//!
//! This module defines the data structures used for application configuration.

use crate::error::Result;
use crate::utils::{extract_display_name_from_exe, extract_icon_from_exe};
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

impl MonitoredApp {
    /// Create a `MonitoredApp` from an executable path
    ///
    /// Extracts display name from file metadata, icon from resources, and generates
    /// a unique UUID. Process name is derived from filename (lowercase, no extension).
    pub fn from_exe_path(exe_path: PathBuf) -> Result<Self> {
        use crate::error::EasyHdrError;

        // Validate that the path exists and is a file
        if !exe_path.exists() {
            return Err(EasyHdrError::ConfigError(format!(
                "Executable path does not exist: {}",
                exe_path.display()
            )));
        }

        if !exe_path.is_file() {
            return Err(EasyHdrError::ConfigError(format!(
                "Path is not a file: {}",
                exe_path.display()
            )));
        }

        // Extract display name from metadata (with fallback to filename)
        let display_name = extract_display_name_from_exe(&exe_path)?;

        // Extract process name from filename (lowercase, without extension)
        let process_name = exe_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                EasyHdrError::ConfigError(format!(
                    "Failed to extract filename from path: {}",
                    exe_path.display()
                ))
            })?
            .to_lowercase();

        // Extract icon from executable (gracefully handles failures)
        let icon_data = match extract_icon_from_exe(&exe_path) {
            Ok(data) if !data.is_empty() => {
                // Record icon in memory profiler
                #[cfg(windows)]
                {
                    use crate::utils::memory_profiler;
                    memory_profiler::get_profiler().record_icon_cached(data.len());
                }
                Some(data)
            }
            Ok(_) => None, // Empty data means extraction failed gracefully
            Err(e) => {
                // Log warning but don't fail - icon is optional
                tracing::warn!("Failed to extract icon from {:?}: {}", exe_path, e);
                None
            }
        };

        Ok(Self {
            id: Uuid::new_v4(),
            display_name,
            exe_path,
            process_name,
            enabled: true, // Default to enabled
            icon_data,
        })
    }

    /// Load icon data lazily if not already loaded
    ///
    /// Loads icon from the executable on first access to reduce memory usage.
    pub fn ensure_icon_loaded(&mut self) -> Option<&Vec<u8>> {
        if self.icon_data.is_none() {
            // Try to load icon
            match extract_icon_from_exe(&self.exe_path) {
                Ok(data) if !data.is_empty() => {
                    // Record icon in memory profiler
                    #[cfg(windows)]
                    {
                        use crate::utils::memory_profiler;
                        memory_profiler::get_profiler().record_icon_cached(data.len());
                    }
                    self.icon_data = Some(data);
                }
                Ok(_) => {
                    tracing::debug!(
                        "Icon extraction returned empty data for {:?}",
                        self.exe_path
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to load icon for {:?}: {}", self.exe_path, e);
                }
            }
        }
        self.icon_data.as_ref()
    }

    /// Release icon data to free memory
    ///
    /// Clears cached icon data to reduce memory usage. Can be reloaded with `ensure_icon_loaded()`.
    pub fn release_icon(&mut self) {
        #[cfg_attr(not(windows), allow(unused_variables))]
        if let Some(icon_data) = self.icon_data.take() {
            // Record icon removal in memory profiler
            #[cfg(windows)]
            {
                use crate::utils::memory_profiler;
                memory_profiler::get_profiler().record_icon_removed(icon_data.len());
            }
            tracing::debug!("Released icon data for {}", self.display_name);
        }
    }
}

/// Implement `TryFrom`<PathBuf> for `MonitoredApp` to follow Rust conversion trait conventions
impl std::convert::TryFrom<PathBuf> for MonitoredApp {
    type Error = crate::error::EasyHdrError;

    fn try_from(exe_path: PathBuf) -> Result<Self> {
        Self::from_exe_path(exe_path)
    }
}

/// Top-level application configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            auto_start: false,
            monitoring_interval_ms: 1000,
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
        assert_eq!(
            config.preferences.auto_start,
            deserialized.preferences.auto_start
        );
    }

    #[test]
    fn test_monitored_app_serialization_round_trip() {
        // Create a MonitoredApp with all fields populated
        let app = MonitoredApp {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Test Application".to_string(),
            exe_path: PathBuf::from("C:\\Program Files\\Test\\test.exe"),
            process_name: "test".to_string(),
            enabled: true,
            icon_data: Some(vec![1, 2, 3, 4]), // Should be skipped in serialization
        };

        // Serialize to JSON
        let json = serde_json::to_string(&app).unwrap();

        // Verify icon_data is not in JSON (due to #[serde(skip)])
        assert!(!json.contains("icon_data"));

        // Deserialize back
        let deserialized: MonitoredApp = serde_json::from_str(&json).unwrap();

        // Verify all fields except icon_data
        assert_eq!(app.id, deserialized.id);
        assert_eq!(app.display_name, deserialized.display_name);
        assert_eq!(app.exe_path, deserialized.exe_path);
        assert_eq!(app.process_name, deserialized.process_name);
        assert_eq!(app.enabled, deserialized.enabled);

        // icon_data should be None after deserialization
        assert!(deserialized.icon_data.is_none());
    }

    #[test]
    fn test_app_config_serialization_round_trip() {
        // Create a full AppConfig with monitored apps
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Cyberpunk 2077".to_string(),
            exe_path: PathBuf::from("C:\\Games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe"),
            process_name: "cyberpunk2077".to_string(),
            enabled: true,
            icon_data: None,
        });
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap(),
            display_name: "Red Dead Redemption 2".to_string(),
            exe_path: PathBuf::from("D:\\Games\\RDR2\\RDR2.exe"),
            process_name: "rdr2".to_string(),
            enabled: false,
            icon_data: None,
        });
        config.preferences.auto_start = true;
        config.preferences.monitoring_interval_ms = 500;
        config.preferences.show_tray_notifications = false;
        config.window_state.x = 200;
        config.window_state.y = 150;
        config.window_state.width = 800;
        config.window_state.height = 600;

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&config).unwrap();

        // Deserialize back
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();

        // Verify monitored apps
        assert_eq!(
            config.monitored_apps.len(),
            deserialized.monitored_apps.len()
        );
        assert_eq!(
            config.monitored_apps[0].id,
            deserialized.monitored_apps[0].id
        );
        assert_eq!(
            config.monitored_apps[0].display_name,
            deserialized.monitored_apps[0].display_name
        );
        assert_eq!(
            config.monitored_apps[0].exe_path,
            deserialized.monitored_apps[0].exe_path
        );
        assert_eq!(
            config.monitored_apps[0].process_name,
            deserialized.monitored_apps[0].process_name
        );
        assert_eq!(
            config.monitored_apps[0].enabled,
            deserialized.monitored_apps[0].enabled
        );

        assert_eq!(
            config.monitored_apps[1].id,
            deserialized.monitored_apps[1].id
        );
        assert_eq!(
            config.monitored_apps[1].enabled,
            deserialized.monitored_apps[1].enabled
        );

        // Verify preferences
        assert_eq!(
            config.preferences.auto_start,
            deserialized.preferences.auto_start
        );
        assert_eq!(
            config.preferences.monitoring_interval_ms,
            deserialized.preferences.monitoring_interval_ms
        );
        assert_eq!(
            config.preferences.show_tray_notifications,
            deserialized.preferences.show_tray_notifications
        );

        // Verify window state
        assert_eq!(config.window_state.x, deserialized.window_state.x);
        assert_eq!(config.window_state.y, deserialized.window_state.y);
        assert_eq!(config.window_state.width, deserialized.window_state.width);
        assert_eq!(config.window_state.height, deserialized.window_state.height);
    }

    #[test]
    fn test_user_preferences_serialization_round_trip() {
        let prefs = UserPreferences {
            auto_start: true,
            monitoring_interval_ms: 2000,
            show_tray_notifications: false,
        };

        let json = serde_json::to_string(&prefs).unwrap();
        let deserialized: UserPreferences = serde_json::from_str(&json).unwrap();

        assert_eq!(prefs.auto_start, deserialized.auto_start);
        assert_eq!(
            prefs.monitoring_interval_ms,
            deserialized.monitoring_interval_ms
        );
        assert_eq!(
            prefs.show_tray_notifications,
            deserialized.show_tray_notifications
        );
    }

    #[test]
    fn test_window_state_serialization_round_trip() {
        let window_state = WindowState {
            x: -100, // Test negative coordinates
            y: -50,
            width: 1920,
            height: 1080,
        };

        let json = serde_json::to_string(&window_state).unwrap();
        let deserialized: WindowState = serde_json::from_str(&json).unwrap();

        assert_eq!(window_state.x, deserialized.x);
        assert_eq!(window_state.y, deserialized.y);
        assert_eq!(window_state.width, deserialized.width);
        assert_eq!(window_state.height, deserialized.height);
    }

    #[test]
    fn test_default_user_preferences() {
        let prefs = UserPreferences::default();

        assert!(!prefs.auto_start);
        assert_eq!(prefs.monitoring_interval_ms, 1000);
        assert!(prefs.show_tray_notifications);
    }

    #[test]
    fn test_default_window_state() {
        let window_state = WindowState::default();

        assert_eq!(window_state.x, 100);
        assert_eq!(window_state.y, 100);
        assert_eq!(window_state.width, 600);
        assert_eq!(window_state.height, 500);
    }

    #[test]
    fn test_empty_config_serialization() {
        // Test that an empty config serializes and deserializes correctly
        let config = AppConfig::default();

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(
            config.monitored_apps.len(),
            deserialized.monitored_apps.len()
        );
        assert_eq!(0, deserialized.monitored_apps.len());
    }

    #[test]
    fn test_from_exe_path_nonexistent_file() {
        // Test that from_exe_path returns an error for a nonexistent file
        let path = PathBuf::from("C:\\NonExistent\\Path\\app.exe");
        let result = MonitoredApp::from_exe_path(path);

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("does not exist"));
        }
    }

    #[test]
    fn test_from_exe_path_process_name_extraction() {
        // Test process name extraction from various path formats
        // This test uses the current executable as a real file that exists
        let current_exe = std::env::current_exe().unwrap();

        let result = MonitoredApp::from_exe_path(current_exe.clone());

        // Should succeed
        assert!(result.is_ok());

        let app = result.unwrap();

        // Process name should be lowercase filename without extension
        let expected_process_name = current_exe
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap()
            .to_lowercase();

        assert_eq!(app.process_name, expected_process_name);
        assert_eq!(app.exe_path, current_exe);
        assert!(app.enabled); // Should default to enabled
        assert!(!app.display_name.is_empty()); // Should have a display name
    }

    #[test]
    fn test_from_exe_path_default_enabled() {
        // Test that newly created apps are enabled by default
        let current_exe = std::env::current_exe().unwrap();
        let result = MonitoredApp::from_exe_path(current_exe);

        assert!(result.is_ok());
        let app = result.unwrap();
        assert!(app.enabled);
    }

    #[test]
    fn test_from_exe_path_unique_uuid() {
        // Test that each call to from_exe_path generates a unique UUID
        let current_exe = std::env::current_exe().unwrap();

        let app1 = MonitoredApp::from_exe_path(current_exe.clone()).unwrap();
        let app2 = MonitoredApp::from_exe_path(current_exe).unwrap();

        // UUIDs should be different
        assert_ne!(app1.id, app2.id);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_from_exe_path_stub_implementation() {
        // On non-Windows platforms, test that the stub implementation works
        let current_exe = std::env::current_exe().unwrap();
        let result = MonitoredApp::from_exe_path(current_exe.clone());

        // Should succeed even on non-Windows
        assert!(result.is_ok());

        let app = result.unwrap();

        // Should have basic metadata
        assert!(!app.display_name.is_empty());
        assert!(!app.process_name.is_empty());
        assert_eq!(app.exe_path, current_exe);
    }
}
