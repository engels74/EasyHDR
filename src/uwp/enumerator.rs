//! UWP package enumeration via Windows Runtime APIs
//!
//! This module provides enumeration of installed UWP applications using the WinRT
//! `PackageManager` API. It discovers all user-accessible UWP packages and extracts
//! metadata including display names, package identifiers, and icon paths.
//!
//! # Package Discovery
//!
//! The enumeration process:
//! 1. Create `PackageManager` instance (WinRT `Management.Deployment` namespace)
//! 2. Call `FindPackagesForUser()` for the current user
//! 3. Iterate through packages and extract metadata:
//!    - `Package.Id.FamilyName` - Stable identifier
//!    - `Package.DisplayName` - User-visible name
//!    - `Package.PublisherDisplayName` - Publisher/vendor
//!    - `Package.Logo` - Path to icon asset
//!
//! # Filtering
//!
//! System packages and framework packages may be excluded from results to show only
//! user-installable applications.

use crate::Result;
use std::path::PathBuf;

/// Metadata for an installed UWP package
///
/// Contains information needed to display and monitor a UWP application.
/// The `package_family_name` serves as the stable identifier for process detection.
#[derive(Debug, Clone)]
pub struct UwpPackageInfo {
    /// User-visible display name (e.g., "Calculator")
    pub display_name: String,

    /// Stable package identifier (e.g., "Microsoft.WindowsCalculator_8wekyb3d8bbwe")
    pub package_family_name: String,

    /// Application ID within the package (typically "App" for main executable)
    pub app_id: String,

    /// Publisher display name (e.g., "Microsoft Corporation")
    pub publisher_display_name: String,

    /// Optional path to logo/icon file in package directory
    pub logo_path: Option<PathBuf>,
}

/// Enumerate all installed UWP packages for the current user
///
/// Discovers UWP applications installed via Microsoft Store or sideloading.
/// System packages and framework packages are typically excluded from results.
///
/// # Returns
///
/// Vector of package metadata for all discovered UWP applications
///
/// # Errors
///
/// Returns error if:
/// - `PackageManager` initialization fails
/// - Package iteration fails
/// - Metadata extraction fails for critical fields
///
/// # Platform
///
/// Windows 10 21H2+ or Windows 11 required. Returns empty vector on non-Windows platforms.
///
/// # Example
///
/// ```no_run
/// # #[cfg(windows)]
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let packages = easyhdr::uwp::enumerate_packages()?;
/// for pkg in packages {
///     println!("{} ({})", pkg.display_name, pkg.package_family_name);
///     println!("  Publisher: {}", pkg.publisher_display_name);
/// }
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn enumerate_packages() -> Result<Vec<UwpPackageInfo>> {
    // TODO: Implement in task 8.2
    // For now, return empty vector
    Ok(Vec::new())
}

#[cfg(not(windows))]
pub fn enumerate_packages() -> Result<Vec<UwpPackageInfo>> {
    Ok(Vec::new())
}
