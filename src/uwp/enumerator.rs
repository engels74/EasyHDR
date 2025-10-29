//! UWP package enumeration via Windows Runtime APIs
//!
//! This module provides enumeration of installed UWP applications using the `WinRT`
//! `PackageManager` API. It discovers UWP packages installed for the current user
//! and extracts metadata including display names, package identifiers, and icon streams.
//!
//! # Package Discovery
//!
//! The enumeration process:
//! 1. Create `PackageManager` instance (`WinRT` `Management.Deployment` namespace)
//! 2. Call `FindPackagesByUserSecurityId("")` to retrieve current user's packages
//! 3. Iterate through packages and extract metadata:
//!    - `Package.Id.FamilyName` - Stable identifier
//!    - `Package.DisplayName` - User-visible name
//!    - `Package.PublisherDisplayName` - Publisher/vendor
//!    - `AppListEntry.DisplayInfo.GetLogo()` - Icon stream reference
//!
//! # Icon Loading Strategy
//!
//! Icons are loaded via the `AppListEntry.DisplayInfo.GetLogo()` API rather than
//! direct filesystem access. This ensures Windows Runtime automatically selects
//! the appropriate scale variant (100%, 125%, 150%, 200%, etc.) based on system
//! DPI settings, and handles all edge cases (missing files, permissions, etc.).
//!
//! # Scope and Permissions
//!
//! Only packages installed for the currently logged-in user are enumerated. This includes
//! Microsoft Store apps and sideloaded applications for the current user, but excludes
//! packages installed for other users or system-wide protected packages.
//!
//! **No administrator privileges are required** for enumeration. Using an empty string
//! as the user security ID parameter (`""`) retrieves packages for the current user only.
//!
//! # Filtering
//!
//! Framework packages and system packages are excluded from results to show only
//! user-installable applications.

use crate::Result;

#[cfg(windows)]
use windows::Storage::Streams::RandomAccessStreamReference;

/// Metadata for an installed UWP package
///
/// Contains information needed to display and monitor a UWP application.
/// The `package_family_name` serves as the stable identifier for process detection.
///
/// # Icon Handling
///
/// Icons are represented as `RandomAccessStreamReference` rather than filesystem paths.
/// This allows the Windows Runtime to automatically select the correct scale variant
/// (e.g., `Square44x44Logo.scale-200.png`) based on system DPI settings.
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

    /// Optional stream reference to logo/icon (Windows Runtime API)
    #[cfg(windows)]
    pub logo_stream: Option<RandomAccessStreamReference>,

    /// Placeholder for non-Windows platforms
    #[cfg(not(windows))]
    pub logo_stream: Option<()>,
}

/// Enumerate all installed UWP packages for the current user
///
/// Discovers UWP applications installed via Microsoft Store or sideloading for the
/// currently logged-in user. Packages installed for other users or requiring system-level
/// access are not included. Framework packages and system packages are filtered from results.
///
/// **No administrator privileges are required.** The function uses `FindPackagesByUserSecurityId("")`
/// which retrieves packages for the current user only.
///
/// # Returns
///
/// Vector of package metadata for all discovered UWP applications accessible to the current user
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
    use windows::core::HSTRING;

    // Create PackageManager instance
    let package_manager =
        PackageManager::new().map_err(|e| EasyHdrError::UwpEnumerationError(Box::new(e)))?;

    // FindPackagesByUserSecurityId("") retrieves packages for the current user only
    // This does not require administrator privileges (unlike FindPackages which
    // enumerates all users' packages and requires elevation)
    let packages = package_manager
        .FindPackagesByUserSecurityId(&HSTRING::from(""))
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

    // Get logo stream reference using AppListEntry API
    // This is the recommended approach that automatically handles scale variants
    let logo_stream = get_app_logo_stream(package);

    // App ID is typically "App" for the main application
    // This could be extracted from the package manifest, but "App" is the standard
    let app_id = String::from("App");

    Ok(Some(UwpPackageInfo {
        display_name,
        package_family_name,
        app_id,
        publisher_display_name,
        logo_stream,
    }))
}

/// Get app logo stream using Windows Runtime `AppListEntry` API
///
/// Uses the recommended `AppListEntry.DisplayInfo.GetLogo()` API to retrieve
/// a logo stream reference. This approach automatically handles:
/// - Scale variant selection (100%, 125%, 150%, 200%, etc.) based on system DPI
/// - Missing icon files (returns None instead of failing)
/// - Permission issues (gracefully degrades to None)
///
/// # Arguments
///
/// * `package` - Reference to the Windows Runtime Package object
///
/// # Returns
///
/// `Option<RandomAccessStreamReference>` - Stream reference if logo is available
///
/// # Implementation Note
///
/// Uses blocking `.get()` on async operations rather than requiring an async runtime.
/// This is acceptable because icon loading happens during enumeration, not in
/// performance-critical paths.
#[cfg(windows)]
fn get_app_logo_stream(
    package: &windows::ApplicationModel::Package,
) -> Option<RandomAccessStreamReference> {
    use tracing::{debug, warn};
    use windows::Foundation::Size;

    // Try to get the first AppListEntry for this package
    let entries_async = match package.GetAppListEntriesAsync() {
        Ok(async_op) => async_op,
        Err(e) => {
            debug!(
                "Failed to call GetAppListEntriesAsync for package '{}': {}",
                package.DisplayName().unwrap_or_default().to_string_lossy(),
                e
            );
            return None;
        }
    };

    // Block on the async operation using .join()
    let entries = match entries_async.join() {
        Ok(entries) => entries,
        Err(e) => {
            debug!(
                "Failed to await GetAppListEntriesAsync for package '{}': {}",
                package.DisplayName().unwrap_or_default().to_string_lossy(),
                e
            );
            return None;
        }
    };

    // Check if we have at least one entry
    let entry_count = match entries.Size() {
        Ok(count) => count,
        Err(e) => {
            debug!("Failed to get entry count: {}", e);
            return None;
        }
    };

    if entry_count == 0 {
        debug!(
            "Package '{}' has no app list entries",
            package.DisplayName().unwrap_or_default().to_string_lossy()
        );
        return None;
    }

    // Get the first entry
    let entry = match entries.GetAt(0) {
        Ok(entry) => entry,
        Err(e) => {
            warn!(
                "Failed to get first entry for package '{}': {}",
                package.DisplayName().unwrap_or_default().to_string_lossy(),
                e
            );
            return None;
        }
    };

    // Get DisplayInfo
    let display_info = match entry.DisplayInfo() {
        Ok(info) => info,
        Err(e) => {
            debug!(
                "Failed to get DisplayInfo for package '{}': {}",
                package.DisplayName().unwrap_or_default().to_string_lossy(),
                e
            );
            return None;
        }
    };

    // Request logo at 32x32 size (standard icon size)
    let size = Size {
        Width: 32.0,
        Height: 32.0,
    };

    match display_info.GetLogo(size) {
        Ok(logo_stream) => {
            debug!(
                "Successfully retrieved logo stream for package '{}'",
                package.DisplayName().unwrap_or_default().to_string_lossy()
            );
            Some(logo_stream)
        }
        Err(e) => {
            debug!(
                "Failed to get logo for package '{}': {}. This is normal for some system packages.",
                package.DisplayName().unwrap_or_default().to_string_lossy(),
                e
            );
            None
        }
    }
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
