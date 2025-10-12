//! HDR control module
//!
//! This module provides functionality to control HDR settings on Windows displays
//! using the Windows Display Configuration API.

pub mod controller;
pub mod version;
pub mod windows_api;

pub use controller::{DisplayTarget, HdrController};
pub use version::WindowsVersion;

