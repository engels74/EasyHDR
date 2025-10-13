//! Configuration management module
//!
//! This module handles loading, saving, and managing application configuration.
//! Configuration is stored in %APPDATA%\EasyHDR\config.json with atomic writes
//! to prevent corruption.
//!
//! # Overview
//!
//! The configuration system provides:
//! - **Persistent storage** of monitored applications and user preferences
//! - **Atomic writes** to prevent configuration corruption
//! - **Automatic defaults** if configuration is missing or corrupt
//! - **JSON serialization** for human-readable configuration files
//!
//! # Architecture
//!
//! - `ConfigManager`: Handles loading and saving configuration files
//! - `AppConfig`: Top-level configuration structure
//! - `MonitoredApp`: Represents a configured application with metadata
//! - `UserPreferences`: User settings (auto-start, intervals, notifications)
//! - `WindowState`: GUI window position and size persistence
//!
//! # Example Usage
//!
//! ```no_run
//! use easyhdr::config::{ConfigManager, MonitoredApp};
//! use std::path::PathBuf;
//!
//! // Load configuration (creates default if missing)
//! let mut config = ConfigManager::load()?;
//!
//! // Add a monitored application
//! let game_path = PathBuf::from(r"C:\Games\Cyberpunk2077\bin\x64\Cyberpunk2077.exe");
//! let app = MonitoredApp::from_exe_path(game_path)?;
//! config.monitored_apps.push(app);
//!
//! // Update preferences
//! config.preferences.auto_start = true;
//! config.preferences.monitoring_interval_ms = 1000;
//!
//! // Save configuration (atomic write)
//! ConfigManager::save(&config)?;
//! # Ok::<(), easyhdr::error::EasyHdrError>(())
//! ```
//!
//! # Configuration File Location
//!
//! - Windows: `%APPDATA%\EasyHDR\config.json`
//! - Example: `C:\Users\YourName\AppData\Roaming\EasyHDR\config.json`
//!
//! # Configuration File Format
//!
//! ```json
//! {
//!   "monitored_apps": [
//!     {
//!       "id": "550e8400-e29b-41d4-a716-446655440000",
//!       "display_name": "Cyberpunk 2077",
//!       "exe_path": "C:\\Games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe",
//!       "process_name": "cyberpunk2077",
//!       "enabled": true
//!     }
//!   ],
//!   "preferences": {
//!     "auto_start": true,
//!     "monitoring_interval_ms": 1000,
//!     "startup_delay_ms": 3000,
//!     "show_tray_notifications": true
//!   },
//!   "window_state": {
//!     "x": 100,
//!     "y": 100,
//!     "width": 600,
//!     "height": 500
//!   }
//! }
//! ```

pub mod manager;
pub mod models;

pub use manager::ConfigManager;
pub use models::{AppConfig, MonitoredApp, UserPreferences, WindowState};
