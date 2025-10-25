//! Universal Windows Platform (UWP) application support
//!
//! This module provides functionality for detecting and managing UWP applications
//! (modern Windows Store apps) alongside traditional Win32 desktop applications.
//!
//! # Key Capabilities
//!
//! - **Process Detection**: Identify running UWP processes via `GetPackageFullName` API
//! - **Package Enumeration**: List installed UWP applications using WinRT `PackageManager`
//! - **Icon Extraction**: Load application icons from UWP package directories
//!
//! # Architecture
//!
//! UWP apps are identified by their **package family name** rather than executable path.
//! This stable identifier persists across version updates, unlike the version-specific
//! package full name.
//!
//! # Windows API Dependencies
//!
//! - `Win32_Storage_Packaging_Appx`: Process detection (GetPackageFullName)
//! - `Management_Deployment`: Package enumeration (PackageManager)
//! - `ApplicationModel`: Package metadata types
//! - `Storage`: Package directory access
//!
//! # Platform Support
//!
//! - Windows 10 21H2+ (Build 19044+)
//! - Windows 11 21H2+ (Build 22000+)
//!
//! # Example
//!
//! ```no_run
//! # #[cfg(windows)]
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Enumerate installed UWP applications
//! let packages = easyhdr::uwp::enumerate_packages()?;
//! for pkg in packages {
//!     println!("{}: {}", pkg.display_name, pkg.package_family_name);
//! }
//! # Ok(())
//! # }
//! ```

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
pub use icon::extract_icon;
