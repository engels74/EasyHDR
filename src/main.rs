//! EasyHDR - Automatic HDR management for Windows
//!
//! This application automatically enables and disables HDR on Windows displays
//! based on configured applications.

// Set Windows subsystem to hide console window
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Module declarations
mod config;
mod controller;
mod error;
mod gui;
mod hdr;
mod monitor;
mod utils;

use error::Result;

// Include Slint-generated code
slint::include_modules!();

fn main() -> Result<()> {
    // Initialize logging
    utils::init_logging()?;

    tracing::info!("EasyHDR starting...");

    // Load configuration
    let config = config::ConfigManager::load()?;
    tracing::info!("Configuration loaded");

    // For now, just show a basic window
    // Full initialization will be implemented in task 13
    let main_window = MainWindow::new()
        .map_err(|e| error::EasyHdrError::ConfigError(format!("Failed to create window: {}", e)))?;

    tracing::info!("Main window created");

    main_window.run()
        .map_err(|e| error::EasyHdrError::ConfigError(format!("Failed to run window: {}", e)))?;

    tracing::info!("EasyHDR shutting down");

    Ok(())
}
