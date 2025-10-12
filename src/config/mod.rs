//! Configuration management module
//!
//! This module handles loading, saving, and managing application configuration.
//! Configuration is stored in %APPDATA%\EasyHDR\config.json with atomic writes
//! to prevent corruption.

pub mod manager;
pub mod models;

pub use manager::ConfigManager;
pub use models::{AppConfig, MonitoredApp, UserPreferences, WindowState};

