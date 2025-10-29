//! HDR control module
//!
//! Controls HDR settings on Windows displays using the Windows Display Configuration API.
//! Provides display enumeration, capability detection, and state control.

pub mod controller;
pub mod version;
pub mod windows_api;

pub use controller::{DisplayTarget, HdrController};
pub use version::WindowsVersion;
