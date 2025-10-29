//! `EasyHDR` - Automatic HDR management for Windows
//!
//! Automatically toggles HDR on Windows displays when configured applications start/stop.
//! Uses multi-threaded event-driven architecture with process monitoring and HDR control.

// Module declarations
pub mod config;
pub mod controller;
pub mod error;
pub mod hdr;
pub mod monitor;
pub mod utils;

// UWP application support (Windows only)
#[cfg(windows)]
pub mod uwp;

// Re-export commonly used types
pub use error::{EasyHdrError, Result};
