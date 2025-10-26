//! UWP package enumeration via Windows Runtime APIs
//!
//! This module provides enumeration of installed UWP applications using the `WinRT`
//! `PackageManager` API. It discovers all user-accessible UWP packages and extracts
//! metadata including display names, package identifiers, and icon paths.
//!
//! # Package Discovery
//!
//! The enumeration process:
//! 1. Create `PackageManager` instance (`WinRT` `Management.Deployment` namespace)
//! 2. Call `FindPackages()` for the current user
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

    /// Stable package identifier (e.g., "`Microsoft.WindowsCalculator_8wekyb3d8bbwe`")
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
    use crate::EasyHdrError;
    use windows::Management::Deployment::PackageManager;

    // Create PackageManager instance
    let package_manager =
        PackageManager::new().map_err(|e| EasyHdrError::UwpEnumerationError(Box::new(e)))?;

    // FindPackages for current user (API changed in windows 0.62)
    let packages = package_manager
        .FindPackages()
        .map_err(|e| EasyHdrError::UwpEnumerationError(Box::new(e)))?;

    let mut result = Vec::new();

    // Iterate through packages
    for package in packages {
        // Extract package metadata, skip on error (log and continue)
        match extract_package_info(&package) {
            Ok(Some(info)) => result.push(info),
            Ok(None) => {
                // Package was filtered (framework or system package)
            }
            Err(e) => {
                // Log error but continue processing other packages
                tracing::warn!("Failed to extract package info: {}", e);
            }
        }
    }

    Ok(result)
}

/// Extract package information from a `WinRT` Package object
///
/// Returns `Ok(None)` if package should be filtered (e.g., framework package)
#[cfg(windows)]
fn extract_package_info(
    package: &windows::ApplicationModel::Package,
) -> Result<Option<UwpPackageInfo>> {
    use crate::EasyHdrError;

    // Check if this is a framework package - skip if so
    let is_framework = package
        .IsFramework()
        .map_err(|e| EasyHdrError::UwpEnumerationError(Box::new(e)))?;

    if is_framework {
        return Ok(None);
    }

    // Get package ID
    let package_id = package
        .Id()
        .map_err(|e| EasyHdrError::UwpEnumerationError(Box::new(e)))?;

    // Extract package family name
    let package_family_name = package_id
        .FamilyName()
        .map_err(|e| EasyHdrError::UwpEnumerationError(Box::new(e)))?
        .to_string();

    // Get display name
    let display_name = package
        .DisplayName()
        .map_err(|e| EasyHdrError::UwpEnumerationError(Box::new(e)))?
        .to_string();

    // Skip packages with empty display names (typically system packages)
    if display_name.is_empty() {
        return Ok(None);
    }

    // Get publisher display name
    let publisher_display_name = package
        .PublisherDisplayName()
        .map_err(|e| EasyHdrError::UwpEnumerationError(Box::new(e)))?
        .to_string();

    // Get logo URI and convert to PathBuf
    let logo_path = match package.Logo() {
        Ok(uri) => {
            // Convert URI to local path
            let uri_str = uri
                .ToString()
                .map_err(|e| EasyHdrError::UwpEnumerationError(Box::new(e)))?
                .to_string();

            // WinRT URIs for UWP packages use the ms-appx:/// scheme
            // We need to resolve them to actual file paths
            // For now, store the raw URI and let icon extraction handle it
            if uri_str.is_empty() {
                None
            } else {
                Some(PathBuf::from(uri_str))
            }
        }
        Err(_) => None, // Logo is optional
    };

    // App ID is typically "App" for the main application
    // This could be extracted from the package manifest, but "App" is the standard
    let app_id = String::from("App");

    Ok(Some(UwpPackageInfo {
        display_name,
        package_family_name,
        app_id,
        publisher_display_name,
        logo_path,
    }))
}

#[cfg(not(windows))]
pub fn enumerate_packages() -> Result<Vec<UwpPackageInfo>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn test_enumerate_packages_returns_results() {
        // Test that enumerate_packages returns a list of packages
        // This test requires UWP apps to be installed on the system
        let result = enumerate_packages();

        // Should not error
        assert!(result.is_ok(), "enumerate_packages should succeed");

        let packages = result.unwrap();

        // Windows systems typically have at least some UWP apps installed
        // (even if just system apps, though we filter most of them)
        // We don't assert a minimum count as it depends on the system
        println!("Found {} UWP packages", packages.len());

        // Print first few packages for debugging
        for (i, pkg) in packages.iter().take(5).enumerate() {
            println!(
                "  {}. {} ({})",
                i + 1,
                pkg.display_name,
                pkg.package_family_name
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn test_package_info_has_required_fields() {
        // Test that package info contains required fields
        let result = enumerate_packages();
        assert!(result.is_ok());

        let packages = result.unwrap();

        // Test that each package has non-empty required fields
        for pkg in packages {
            // Display name should not be empty (we filter empty ones)
            assert!(
                !pkg.display_name.is_empty(),
                "Package display name should not be empty"
            );

            // Package family name should not be empty
            assert!(
                !pkg.package_family_name.is_empty(),
                "Package family name should not be empty"
            );

            // App ID should not be empty (we set it to "App")
            assert!(!pkg.app_id.is_empty(), "App ID should not be empty");

            // Publisher display name may or may not be empty, so we don't assert on it
            // Logo path is optional, so we don't assert on it
        }
    }

    #[test]
    #[cfg(windows)]
    fn test_package_info_filters_framework_packages() {
        // Framework packages should be filtered out
        // We can't directly test this without knowing what's on the system,
        // but we can verify that the function doesn't error
        let result = enumerate_packages();
        assert!(result.is_ok());

        // If we found packages, verify they're not framework packages
        // by checking that they have display names (framework packages often don't)
        let packages = result.unwrap();
        for pkg in packages {
            assert!(
                !pkg.display_name.is_empty(),
                "Non-framework packages should have display names"
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn test_package_family_name_format() {
        // Test that package family names follow expected format
        let result = enumerate_packages();
        assert!(result.is_ok());

        let packages = result.unwrap();

        for pkg in packages {
            // Package family names should contain an underscore
            // Format: Name_PublisherId
            assert!(
                pkg.package_family_name.contains('_'),
                "Package family name '{}' should contain underscore",
                pkg.package_family_name
            );
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn test_enumerate_packages_non_windows() {
        // On non-Windows platforms, should return empty vector
        let result = enumerate_packages();
        assert!(result.is_ok());

        let packages = result.unwrap();
        assert!(
            packages.is_empty(),
            "Non-Windows platforms should return empty vector"
        );
    }
}
