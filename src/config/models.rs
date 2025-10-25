//! Configuration data models
//!
//! This module defines the data structures used for application configuration.

use crate::error::Result;
use crate::utils::{extract_display_name_from_exe, extract_icon_from_exe};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Win32 desktop application
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Win32App {
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

/// UWP (Universal Windows Platform) application
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UwpApp {
    /// Unique identifier for this application entry
    pub id: Uuid,
    /// Display name shown in the UI
    pub display_name: String,
    /// Package family name (stable identifier across updates)
    pub package_family_name: String,
    /// Application ID within the package
    pub app_id: String,
    /// Whether monitoring is enabled for this application
    pub enabled: bool,
    /// Cached icon data (not persisted to config file)
    #[serde(skip)]
    pub icon_data: Option<Vec<u8>>,
}

/// Represents a monitored application (Win32 or UWP)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(clippy::large_enum_variant)] // Win32App and UwpApp are similarly sized
pub enum MonitoredApp {
    /// Traditional Win32 desktop application
    Win32(Win32App),
    /// Universal Windows Platform application
    Uwp(UwpApp),
}

impl Win32App {
    /// Create a Win32 app from an executable path
    ///
    /// Extracts display name from file metadata, icon from resources, and generates
    /// a unique UUID. Process name is derived from filename (lowercase, no extension).
    ///
    /// Accepts any type that can be converted into a `PathBuf` for better ergonomics.
    pub fn from_exe_path(exe_path: impl Into<PathBuf>) -> Result<Self> {
        use crate::error::EasyHdrError;

        let exe_path = exe_path.into();

        // Validate that the path exists and is a file
        if !exe_path.exists() {
            return Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                format!("Executable path does not exist: {}", exe_path.display()),
            )));
        }

        if !exe_path.is_file() {
            return Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                format!("Path is not a file: {}", exe_path.display()),
            )));
        }

        // Extract display name from metadata (with fallback to filename)
        let display_name = extract_display_name_from_exe(&exe_path)?;

        // Extract process name from filename (lowercase, without extension)
        let process_name = exe_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                EasyHdrError::ConfigError(crate::error::StringError::new(format!(
                    "Failed to extract filename from path: {}",
                    exe_path.display()
                )))
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

impl UwpApp {
    /// Create a UWP app from package information
    ///
    /// Generates a unique UUID for the app entry.
    ///
    /// # Arguments
    ///
    /// * `display_name` - Human-readable name shown in the UI
    /// * `package_family_name` - Stable package identifier (e.g., "Microsoft.WindowsCalculator_8wekyb3d8bbwe")
    /// * `app_id` - Application ID within the package (typically "App" for main application)
    pub fn from_package_info(
        display_name: String,
        package_family_name: String,
        app_id: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            display_name,
            package_family_name,
            app_id,
            enabled: true, // Default to enabled
            icon_data: None,
        }
    }
}

impl MonitoredApp {
    /// Get the unique ID regardless of app type
    pub fn id(&self) -> &Uuid {
        match self {
            Self::Win32(app) => &app.id,
            Self::Uwp(app) => &app.id,
        }
    }

    /// Get the display name regardless of app type
    pub fn display_name(&self) -> &str {
        match self {
            Self::Win32(app) => &app.display_name,
            Self::Uwp(app) => &app.display_name,
        }
    }

    /// Check if monitoring is enabled
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Win32(app) => app.enabled,
            Self::Uwp(app) => app.enabled,
        }
    }

    /// Get mutable reference to icon data
    pub fn icon_data_mut(&mut self) -> &mut Option<Vec<u8>> {
        match self {
            Self::Win32(app) => &mut app.icon_data,
            Self::Uwp(app) => &mut app.icon_data,
        }
    }

    /// Load icon data lazily if not already loaded (for Win32 apps only)
    ///
    /// Loads icon from the executable on first access to reduce memory usage.
    /// For UWP apps, this is a no-op (icon should be loaded during enumeration).
    pub fn ensure_icon_loaded(&mut self) -> Option<&Vec<u8>> {
        match self {
            Self::Win32(app) => app.ensure_icon_loaded(),
            Self::Uwp(app) => app.icon_data.as_ref(),
        }
    }

    /// Release icon data to free memory
    ///
    /// Clears cached icon data to reduce memory usage. Can be reloaded with `ensure_icon_loaded()`.
    pub fn release_icon(&mut self) {
        match self {
            Self::Win32(app) => app.release_icon(),
            Self::Uwp(app) => {
                #[cfg_attr(not(windows), allow(unused_variables))]
                if let Some(icon_data) = app.icon_data.take() {
                    // Record icon removal in memory profiler
                    #[cfg(windows)]
                    {
                        use crate::utils::memory_profiler;
                        memory_profiler::get_profiler().record_icon_removed(icon_data.len());
                    }
                    tracing::debug!("Released icon data for {}", app.display_name);
                }
            }
        }
    }
}

/// Backward-compatible deserialization for MonitoredApp enum
///
/// Supports both:
/// - Legacy format: entries without `app_type` field (migrated to Win32)
/// - New format: entries with `app_type` field ("win32" or "uwp")
impl<'de> Deserialize<'de> for MonitoredApp {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "app_type", rename_all = "lowercase")]
        enum Tagged {
            Win32(Win32App),
            Uwp(UwpApp),
        }

        #[derive(Deserialize)]
        struct Legacy {
            id: Uuid,
            display_name: String,
            exe_path: PathBuf,
            process_name: String,
            enabled: bool,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Tagged(Tagged),
            Legacy(Legacy),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Tagged(Tagged::Win32(app)) => Ok(Self::Win32(app)),
            Helper::Tagged(Tagged::Uwp(app)) => Ok(Self::Uwp(app)),
            Helper::Legacy(legacy) => {
                // Migrate legacy format to Win32App
                Ok(Self::Win32(Win32App {
                    id: legacy.id,
                    display_name: legacy.display_name,
                    exe_path: legacy.exe_path,
                    process_name: legacy.process_name,
                    enabled: legacy.enabled,
                    icon_data: None,
                }))
            }
        }
    }
}

/// Serialize MonitoredApp enum with `app_type` discriminator
impl Serialize for MonitoredApp {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        match self {
            Self::Win32(app) => {
                let mut state = serializer.serialize_struct("MonitoredApp", 6)?;
                state.serialize_field("app_type", "win32")?;
                state.serialize_field("id", &app.id)?;
                state.serialize_field("display_name", &app.display_name)?;
                state.serialize_field("exe_path", &app.exe_path)?;
                state.serialize_field("process_name", &app.process_name)?;
                state.serialize_field("enabled", &app.enabled)?;
                state.end()
            }
            Self::Uwp(app) => {
                let mut state = serializer.serialize_struct("MonitoredApp", 6)?;
                state.serialize_field("app_type", "uwp")?;
                state.serialize_field("id", &app.id)?;
                state.serialize_field("display_name", &app.display_name)?;
                state.serialize_field("package_family_name", &app.package_family_name)?;
                state.serialize_field("app_id", &app.app_id)?;
                state.serialize_field("enabled", &app.enabled)?;
                state.end()
            }
        }
    }
}

impl AsRef<std::path::Path> for Win32App {
    fn as_ref(&self) -> &std::path::Path {
        &self.exe_path
    }
}

/// Implement `TryFrom`<PathBuf> for `Win32App` to follow Rust conversion trait conventions
impl std::convert::TryFrom<PathBuf> for Win32App {
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct UserPreferences {
    /// Whether to auto-start on Windows login
    pub auto_start: bool,
    /// Process monitoring interval in milliseconds (500-2000)
    pub monitoring_interval_ms: u64,
    /// Whether to show tray notifications on HDR changes
    pub show_tray_notifications: bool,
    /// Whether to show notifications when application updates are available
    #[serde(default = "default_show_update_notifications")]
    pub show_update_notifications: bool,
    /// Whether to minimize to tray when minimize button is clicked (true) or minimize to taskbar (false)
    pub minimize_to_tray_on_minimize: bool,
    /// Whether to minimize to tray when close button is clicked (true) or close the application (false)
    pub minimize_to_tray_on_close: bool,
    /// Whether to start minimized to tray on application launch (true) or show main window (false)
    #[serde(default)]
    pub start_minimized_to_tray: bool,
    /// Timestamp of the last update check (Unix timestamp in seconds, 0 if never checked)
    #[serde(default)]
    pub last_update_check_time: u64,
    /// Cached latest version from the last update check (empty if never checked or failed)
    #[serde(default)]
    pub cached_latest_version: String,
}

/// Default value for `show_update_notifications` field (true for backwards compatibility)
fn default_show_update_notifications() -> bool {
    true
}

/// Window state for position and size persistence
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
            show_update_notifications: true,
            minimize_to_tray_on_minimize: true,
            minimize_to_tray_on_close: false,
            start_minimized_to_tray: false,
            last_update_check_time: 0,
            cached_latest_version: String::new(),
        }
    }
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: 100,
            y: 100,
            width: 660,
            height: 660,
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
    fn test_win32_app_serialization_round_trip() {
        // Create a Win32App with all fields populated
        let app = Win32App {
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
        let deserialized: Win32App = serde_json::from_str(&json).unwrap();

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
    fn test_uwp_app_serialization_round_trip() {
        // Create a UwpApp with all fields populated
        let app = UwpApp {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Calculator".to_string(),
            package_family_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
            app_id: "App".to_string(),
            enabled: true,
            icon_data: Some(vec![1, 2, 3, 4]), // Should be skipped in serialization
        };

        // Serialize to JSON
        let json = serde_json::to_string(&app).unwrap();

        // Verify icon_data is not in JSON (due to #[serde(skip)])
        assert!(!json.contains("icon_data"));

        // Deserialize back
        let deserialized: UwpApp = serde_json::from_str(&json).unwrap();

        // Verify all fields except icon_data
        assert_eq!(app.id, deserialized.id);
        assert_eq!(app.display_name, deserialized.display_name);
        assert_eq!(app.package_family_name, deserialized.package_family_name);
        assert_eq!(app.app_id, deserialized.app_id);
        assert_eq!(app.enabled, deserialized.enabled);

        // icon_data should be None after deserialization
        assert!(deserialized.icon_data.is_none());
    }

    #[test]
    fn test_monitored_app_win32_serialization() {
        // Create a Win32 MonitoredApp
        let app = MonitoredApp::Win32(Win32App {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Test Application".to_string(),
            exe_path: PathBuf::from("C:\\Program Files\\Test\\test.exe"),
            process_name: "test".to_string(),
            enabled: true,
            icon_data: None,
        });

        // Serialize to JSON
        let json = serde_json::to_string(&app).unwrap();

        // Verify app_type field is present
        assert!(json.contains("\"app_type\":\"win32\""));

        // Deserialize back
        let deserialized: MonitoredApp = serde_json::from_str(&json).unwrap();

        // Verify it's a Win32 variant
        assert!(matches!(deserialized, MonitoredApp::Win32(_)));
        assert_eq!(app.id(), deserialized.id());
        assert_eq!(app.display_name(), deserialized.display_name());
    }

    #[test]
    fn test_monitored_app_uwp_serialization() {
        // Create a UWP MonitoredApp
        let app = MonitoredApp::Uwp(UwpApp {
            id: Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap(),
            display_name: "Calculator".to_string(),
            package_family_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
            app_id: "App".to_string(),
            enabled: true,
            icon_data: None,
        });

        // Serialize to JSON
        let json = serde_json::to_string(&app).unwrap();

        // Verify app_type field is present
        assert!(json.contains("\"app_type\":\"uwp\""));

        // Deserialize back
        let deserialized: MonitoredApp = serde_json::from_str(&json).unwrap();

        // Verify it's a UWP variant
        assert!(matches!(deserialized, MonitoredApp::Uwp(_)));
        assert_eq!(app.id(), deserialized.id());
        assert_eq!(app.display_name(), deserialized.display_name());
    }

    #[test]
    fn test_backward_compatible_deserialization() {
        // Legacy JSON format without app_type field
        let legacy_json = r#"{
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "display_name": "Test Application",
            "exe_path": "C:\\Program Files\\Test\\test.exe",
            "process_name": "test",
            "enabled": true
        }"#;

        // Deserialize legacy format
        let deserialized: MonitoredApp = serde_json::from_str(legacy_json).unwrap();

        // Should be migrated to Win32 variant
        assert!(matches!(deserialized, MonitoredApp::Win32(_)));

        if let MonitoredApp::Win32(app) = deserialized {
            assert_eq!(
                app.id,
                Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
            );
            assert_eq!(app.display_name, "Test Application");
            assert_eq!(
                app.exe_path,
                PathBuf::from("C:\\Program Files\\Test\\test.exe")
            );
            assert_eq!(app.process_name, "test");
            assert!(app.enabled);
            assert!(app.icon_data.is_none());
        }
    }

    #[test]
    fn test_monitored_app_helper_methods() {
        // Test Win32 variant
        let win32_app = MonitoredApp::Win32(Win32App {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Win32 App".to_string(),
            exe_path: PathBuf::from("C:\\test.exe"),
            process_name: "test".to_string(),
            enabled: true,
            icon_data: Some(vec![1, 2, 3]),
        });

        assert_eq!(
            *win32_app.id(),
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
        );
        assert_eq!(win32_app.display_name(), "Win32 App");
        assert!(win32_app.is_enabled());

        // Test UWP variant
        let uwp_app = MonitoredApp::Uwp(UwpApp {
            id: Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap(),
            display_name: "UWP App".to_string(),
            package_family_name: "Package_Publisher".to_string(),
            app_id: "App".to_string(),
            enabled: false,
            icon_data: None,
        });

        assert_eq!(
            *uwp_app.id(),
            Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap()
        );
        assert_eq!(uwp_app.display_name(), "UWP App");
        assert!(!uwp_app.is_enabled());
    }

    #[test]
    fn test_app_config_serialization_round_trip() {
        // Create a full AppConfig with mixed Win32 and UWP apps
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Cyberpunk 2077".to_string(),
            exe_path: PathBuf::from("C:\\Games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe"),
            process_name: "cyberpunk2077".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap(),
            display_name: "Red Dead Redemption 2".to_string(),
            exe_path: PathBuf::from("D:\\Games\\RDR2\\RDR2.exe"),
            process_name: "rdr2".to_string(),
            enabled: false,
            icon_data: None,
        }));
        config.monitored_apps.push(MonitoredApp::Uwp(UwpApp {
            id: Uuid::parse_str("7c9e6679-7425-40de-944b-e07fc1f90ae7").unwrap(),
            display_name: "Calculator".to_string(),
            package_family_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
            app_id: "App".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.preferences.auto_start = true;
        config.preferences.monitoring_interval_ms = 500;
        config.preferences.show_tray_notifications = false;
        config.window_state.x = 200;
        config.window_state.y = 150;
        config.window_state.width = 800;
        config.window_state.height = 600;

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&config).unwrap();

        // Verify JSON contains app_type fields
        assert!(json.contains("\"app_type\": \"win32\""));
        assert!(json.contains("\"app_type\": \"uwp\""));

        // Deserialize back
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();

        // Verify monitored apps
        assert_eq!(
            config.monitored_apps.len(),
            deserialized.monitored_apps.len()
        );
        assert_eq!(3, deserialized.monitored_apps.len());

        // Verify first app (Win32)
        assert_eq!(
            config.monitored_apps[0].id(),
            deserialized.monitored_apps[0].id()
        );
        assert_eq!(
            config.monitored_apps[0].display_name(),
            deserialized.monitored_apps[0].display_name()
        );
        assert!(matches!(deserialized.monitored_apps[0], MonitoredApp::Win32(_)));

        // Verify second app (Win32)
        assert_eq!(
            config.monitored_apps[1].id(),
            deserialized.monitored_apps[1].id()
        );
        assert!(!deserialized.monitored_apps[1].is_enabled());

        // Verify third app (UWP)
        assert_eq!(
            config.monitored_apps[2].id(),
            deserialized.monitored_apps[2].id()
        );
        assert!(matches!(deserialized.monitored_apps[2], MonitoredApp::Uwp(_)));

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
            show_update_notifications: true,
            minimize_to_tray_on_minimize: true,
            minimize_to_tray_on_close: false,
            start_minimized_to_tray: true,
            last_update_check_time: 1_234_567_890,
            cached_latest_version: "1.2.3".to_string(),
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
        assert_eq!(
            prefs.start_minimized_to_tray,
            deserialized.start_minimized_to_tray
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
        assert_eq!(window_state.width, 660);
        assert_eq!(window_state.height, 660);
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
        let result = Win32App::from_exe_path(path);

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

        let result = Win32App::from_exe_path(current_exe.clone());

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
        let result = Win32App::from_exe_path(current_exe);

        assert!(result.is_ok());
        let app = result.unwrap();
        assert!(app.enabled);
    }

    #[test]
    fn test_from_exe_path_unique_uuid() {
        // Test that each call to from_exe_path generates a unique UUID
        let current_exe = std::env::current_exe().unwrap();

        let app1 = Win32App::from_exe_path(current_exe.clone()).unwrap();
        let app2 = Win32App::from_exe_path(current_exe).unwrap();

        // UUIDs should be different
        assert_ne!(app1.id, app2.id);
    }

    #[test]
    fn test_uwp_app_from_package_info() {
        // Test UwpApp constructor
        let app = UwpApp::from_package_info(
            "Calculator".to_string(),
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
            "App".to_string(),
        );

        assert_eq!(app.display_name, "Calculator");
        assert_eq!(
            app.package_family_name,
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe"
        );
        assert_eq!(app.app_id, "App");
        assert!(app.enabled); // Should default to enabled
        assert!(app.icon_data.is_none());

        // Test that UUIDs are unique
        let app2 = UwpApp::from_package_info(
            "Calculator".to_string(),
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
            "App".to_string(),
        );
        assert_ne!(app.id, app2.id);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_from_exe_path_stub_implementation() {
        // On non-Windows platforms, test that the stub implementation works
        let current_exe = std::env::current_exe().unwrap();
        let result = Win32App::from_exe_path(current_exe.clone());

        // Should succeed even on non-Windows
        assert!(result.is_ok());

        let app = result.unwrap();

        // Should have basic metadata
        assert!(!app.display_name.is_empty());
        assert!(!app.process_name.is_empty());
        assert_eq!(app.exe_path, current_exe);
    }
}
