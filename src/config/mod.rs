//! Configuration management module
//!
//! Handles loading, saving, and managing application configuration.
//! Provides persistent storage with atomic writes to prevent corruption.

pub mod manager;
pub mod models;

pub use manager::ConfigManager;
pub use models::{AppConfig, MonitoredApp, UserPreferences, UwpApp, Win32App, WindowState};
