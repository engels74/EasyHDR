//! UWP application icon extraction
//!
//! This module provides icon loading from UWP package directories. Icons are typically
//! stored as PNG files referenced by package metadata.
//!
//! # Icon Location
//!
//! UWP packages store icons in their installation directory, typically:
//! - `C:\Program Files\WindowsApps\<PackageFullName>\Assets\`
//!
//! The `Package.Logo` property provides the relative path within the package.
//!
//! # Implementation Strategy
//!
//! **Phase 1** (current): Simple file-based loading
//! - Read PNG file directly from package directory
//! - Return raw image bytes
//! - Use placeholder icon if file not found
//!
//! **Phase 2** (future enhancement):
//! - Parse `.pri` (Package Resource Index) files for optimal icon selection
//! - Select appropriate icon scale (100%, 200%, etc.) based on DPI
//! - Handle multiple icon sizes and choose best fit
//!
//! # Security Considerations
//!
//! UWP package directories are protected by Windows. Access denied errors should be
//! handled gracefully with fallback to placeholder icon.

use crate::Result;
use std::path::Path;

/// Extract icon data from a UWP package logo file
///
/// Attempts to load the logo image from the package directory. Returns raw image bytes
/// (typically PNG format) suitable for display in the UI.
///
/// If the logo cannot be found or loaded (e.g., due to permissions), returns placeholder
/// icon data instead of failing.
///
/// # Arguments
///
/// * `logo_path` - Path to logo file from package metadata (`Package.Logo`)
///
/// # Returns
///
/// Raw image bytes (PNG format) for the application icon
///
/// # Errors
///
/// Returns error only for critical failures. Missing/inaccessible logos return
/// placeholder data rather than errors.
///
/// # Example
///
/// ```no_run
/// # #[cfg(windows)]
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use std::path::Path;
///
/// let logo_path = Path::new(r"C:\Program Files\WindowsApps\...\Assets\Square44x44Logo.png");
/// let icon_data = easyhdr::uwp::extract_icon(logo_path)?;
/// println!("Loaded {} bytes of icon data", icon_data.len());
/// # Ok(())
/// # }
/// ```
/// Minimal valid 1x1 transparent PNG file (67 bytes)
///
/// Used as fallback when UWP package icons cannot be loaded.
/// This is a valid PNG image that can be displayed in the UI.
const PLACEHOLDER_ICON: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
    0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1 dimensions
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, // RGBA color type
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, // IDAT chunk
    0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, // Compressed data
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, // CRC
    0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, // IEND chunk
    0x42, 0x60, 0x82, // PNG end marker
];

#[cfg(windows)]
pub fn extract_icon(logo_path: &Path) -> Result<Vec<u8>> {
    use crate::EasyHdrError;
    use std::fs;
    use tracing::{debug, warn};

    // Check if the logo path exists
    if !logo_path.exists() {
        debug!(
            "Logo file not found at '{}', using placeholder icon",
            logo_path.display()
        );
        return Ok(PLACEHOLDER_ICON.to_vec());
    }

    // Attempt to read the icon file
    match fs::read(logo_path) {
        Ok(data) => {
            debug!(
                "Successfully loaded icon from '{}' ({} bytes)",
                logo_path.display(),
                data.len()
            );
            Ok(data)
        }
        Err(e) => {
            // Log the error but return placeholder instead of failing
            warn!(
                "Failed to read icon from '{}': {}. Using placeholder icon.",
                logo_path.display(),
                e
            );

            // For logging purposes, we could create an error but we don't return it
            // since the requirement is to gracefully handle failures with placeholder
            let _error = EasyHdrError::UwpIconExtractionError(format!(
                "Failed to read icon from '{}': {}",
                logo_path.display(),
                e
            ));

            Ok(PLACEHOLDER_ICON.to_vec())
        }
    }
}

#[cfg(not(windows))]
pub fn extract_icon(_logo_path: &Path) -> Result<Vec<u8>> {
    // Non-Windows platforms always use placeholder
    Ok(PLACEHOLDER_ICON.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_placeholder_icon_is_valid_png() {
        // Verify placeholder starts with PNG signature
        assert_eq!(
            &PLACEHOLDER_ICON[0..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        );

        // Verify placeholder ends with IEND chunk
        assert_eq!(
            &PLACEHOLDER_ICON[PLACEHOLDER_ICON.len() - 4..],
            &[0xAE, 0x42, 0x60, 0x82]
        );

        // Verify minimum PNG size
        assert!(PLACEHOLDER_ICON.len() >= 67);
    }

    #[test]
    fn test_extract_icon_nonexistent_file() {
        let result = extract_icon(Path::new("/nonexistent/path/icon.png"));

        // Should succeed with placeholder icon
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data, PLACEHOLDER_ICON);
    }

    #[test]
    fn test_extract_icon_valid_file() {
        // Create a temporary PNG file with test data
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = b"fake png data for testing";
        temp_file.write_all(test_data).unwrap();
        temp_file.flush().unwrap();

        let result = extract_icon(temp_file.path());

        // Should succeed with the actual file data
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data, test_data);
    }

    #[cfg(windows)]
    #[test]
    fn test_extract_icon_permission_denied() {
        // This test attempts to read from a restricted path
        // On Windows, certain system paths are protected
        let restricted_path = Path::new(r"C:\System Volume Information\test.png");

        let result = extract_icon(restricted_path);

        // Should succeed with placeholder icon rather than failing
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data, PLACEHOLDER_ICON);
    }

    #[test]
    fn test_extract_icon_empty_file() {
        // Create an empty temporary file
        let temp_file = NamedTempFile::new().unwrap();

        let result = extract_icon(temp_file.path());

        // Should succeed with the empty file (valid case - just 0 bytes)
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data.len(), 0);
    }
}
