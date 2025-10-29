//! Universal Windows Platform (UWP) application support
//!
//! Provides functionality for detecting and managing UWP applications
//! (modern Windows Store apps) alongside traditional Win32 desktop applications.
//! Uses package family names for stable identification across version updates.

// Submodule declarations (Windows-only)
#[cfg(windows)]
pub mod detector;

#[cfg(windows)]
pub mod enumerator;

#[cfg(windows)]
pub mod icon;

// Public API re-exports
#[cfg(windows)]
pub use detector::{detect_uwp_process, extract_package_family_name};

#[cfg(windows)]
pub use enumerator::{UwpPackageInfo, enumerate_packages};

#[cfg(windows)]
pub use icon::{extract_icon, extract_icon_from_stream};
