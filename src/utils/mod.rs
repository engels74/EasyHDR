//! Utility modules
//!
//! This module contains utility functions for icon extraction,
//! auto-start management, and logging.
//!
//! # Overview
//!
//! The utilities module provides:
//! - **Auto-start management** via Windows registry
//! - **Icon extraction** from executable files
//! - **Logging system** with file rotation
//! - **Memory profiling** for performance monitoring
//! - **Startup profiling** for initialization performance
//!
//! # Modules
//!
//! ## autostart
//!
//! Manages Windows auto-start functionality via registry entries in
//! `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`.
//!
//! ## icon_extractor
//!
//! Extracts application icons and display names from executable files using
//! Windows Shell32 API. Converts icons to raw RGBA bitmap data for display in the GUI.
//!
//! ## logging
//!
//! Initializes the tracing-based logging system with file output to
//! `%APPDATA%\EasyHDR\app.log`. Implements log rotation at 5MB with 3 historical files.
//!
//! ## memory_profiler
//!
//! Provides memory usage tracking and profiling to ensure the application
//! stays within the 50MB RAM requirement.
//!
//! ## startup_profiler
//!
//! Tracks startup time from application launch to GUI display to ensure
//! the 200ms startup time requirement is met.
//!
//! # Example Usage
//!
//! ```no_run
//! use easyhdr::utils;
//!
//! // Initialize logging
//! utils::init_logging()?;
//!
//! // Check auto-start status
//! let is_enabled = utils::AutoStartManager::is_enabled()?;
//! println!("Auto-start: {}", if is_enabled { "enabled" } else { "disabled" });
//!
//! // Enable auto-start
//! utils::AutoStartManager::enable()?;
//!
//! // Extract icon and display name from executable
//! use std::path::PathBuf;
//! let exe_path = PathBuf::from(r"C:\Games\game.exe");
//! let icon_data = utils::extract_icon_from_exe(&exe_path)?;
//! let display_name = utils::extract_display_name_from_exe(&exe_path)?;
//! println!("Extracted {} bytes of icon data for '{}'", icon_data.len(), display_name);
//! # Ok::<(), easyhdr::error::EasyHdrError>(())
//! ```
//!
//! # Requirements
//!
//! - Requirement 6.6: Create registry entry when auto-start is enabled
//! - Requirement 6.7: Remove registry entry when auto-start is disabled
//! - Requirement 8.1: Log to %APPDATA%\EasyHDR\app.log
//! - Requirement 8.2: Rotate logs at 5MB with 3 historical files
//! - Requirement 9.1: Use less than 50MB RAM during monitoring
//! - Requirement 9.3: Display GUI within 200ms

pub mod autostart;
pub mod icon_extractor;
pub mod logging;
pub mod memory_profiler;
pub mod startup_profiler;

pub use autostart::AutoStartManager;
pub use icon_extractor::{extract_display_name_from_exe, extract_icon_from_exe};
pub use logging::init_logging;

