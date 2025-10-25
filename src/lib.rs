//! `EasyHDR` - Automatic HDR management for Windows
//!
//! Automatically toggles HDR on Windows displays when configured applications start/stop.
//! Uses multi-threaded event-driven architecture with `ProcessMonitor` polling processes,
//! `AppController` coordinating HDR control, and `HdrController` interfacing with Windows APIs.
//!
//! # Requirements
//!
//! - Windows 10 21H2+ (Build 19044+) or Windows 11
//! - HDR-capable display with updated drivers
//!
//! # Performance
//!
//! - CPU: <1%, Memory: <50MB, Startup: <200ms, Detection: 1-2s

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
