//! EasyHDR - Automatic HDR management for Windows
//!
//! This library provides functionality to automatically enable and disable HDR
//! on Windows displays based on configured applications.
//!
//! # Modules
//!
//! - `config`: Configuration management and persistence
//! - `controller`: Application logic controller
//! - `error`: Error types and handling
//! - `hdr`: HDR control and Windows version detection
//! - `monitor`: Process monitoring
//! - `utils`: Utility functions

// Module declarations
pub mod config;
pub mod controller;
pub mod error;
pub mod hdr;
pub mod monitor;
pub mod utils;

// Re-export commonly used types
pub use error::{EasyHdrError, Result};

