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
//! **Phase 1** (current): PNG decoding and conversion to RGBA
//! - Read PNG file from package directory
//! - Decode PNG and convert to RGBA8 format
//! - Resize to 32x32 pixels to match Win32 icon format
//! - Return raw RGBA bytes (4096 bytes for 32x32 image)
//! - Use placeholder icon if file not found or decoding fails
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

/// Standard icon size for UI display (32x32 pixels)
const ICON_SIZE: u32 = 32;

/// Extract icon data from a UWP package logo file
///
/// Attempts to load the logo image from the package directory, decode it from PNG format,
/// and convert it to raw RGBA bytes (32x32 pixels, 4096 bytes total).
///
/// This matches the format used by Win32 icon extraction, ensuring consistent handling
/// in the GUI layer.
///
/// If the logo cannot be found, loaded, or decoded (e.g., due to permissions or invalid
/// format), returns placeholder icon data instead of failing.
///
/// # Arguments
///
/// * `logo_path` - Path to logo file from package metadata (`Package.Logo`)
///
/// # Returns
///
/// Raw RGBA bytes (32x32 pixels = 4096 bytes) for the application icon
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
/// assert_eq!(icon_data.len(), 32 * 32 * 4); // 4096 bytes (RGBA)
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn extract_icon(logo_path: &Path) -> Result<Vec<u8>> {
    use crate::EasyHdrError;
    use image::ImageReader;
    use tracing::{debug, warn};

    // Check if the logo path exists
    if !logo_path.exists() {
        debug!(
            "Logo file not found at '{}', using placeholder icon",
            logo_path.display()
        );
        return Ok(create_placeholder_rgba());
    }

    // Attempt to read and decode the icon file
    match ImageReader::open(logo_path) {
        Ok(reader) => {
            match reader.decode() {
                Ok(img) => {
                    // Convert to RGBA8 format
                    let rgba_img = img.to_rgba8();
                    let (width, height) = rgba_img.dimensions();

                    debug!(
                        "Successfully decoded icon from '{}' ({}x{} pixels)",
                        logo_path.display(),
                        width,
                        height
                    );

                    // Resize to standard icon size if needed
                    let rgba_data = if width != ICON_SIZE || height != ICON_SIZE {
                        debug!(
                            "Resizing icon from {}x{} to {}x{}",
                            width, height, ICON_SIZE, ICON_SIZE
                        );
                        let resized = image::imageops::resize(
                            &rgba_img,
                            ICON_SIZE,
                            ICON_SIZE,
                            image::imageops::FilterType::Lanczos3,
                        );
                        resized.into_raw()
                    } else {
                        rgba_img.into_raw()
                    };

                    debug!(
                        "Icon converted to RGBA: {} bytes (expected {})",
                        rgba_data.len(),
                        ICON_SIZE * ICON_SIZE * 4
                    );

                    Ok(rgba_data)
                }
                Err(e) => {
                    warn!(
                        "Failed to decode icon from '{}': {}. Using placeholder icon.",
                        logo_path.display(),
                        e
                    );
                    Ok(create_placeholder_rgba())
                }
            }
        }
        Err(e) => {
            // Log the error but return placeholder instead of failing
            warn!(
                "Failed to open icon file '{}': {}. Using placeholder icon.",
                logo_path.display(),
                e
            );

            // For logging purposes, we could create an error but we don't return it
            // since we gracefully handle failures with placeholder
            let _error = EasyHdrError::UwpIconExtractionError(format!(
                "Failed to open icon from '{}': {}",
                logo_path.display(),
                e
            ));

            Ok(create_placeholder_rgba())
        }
    }
}

#[cfg(not(windows))]
pub fn extract_icon(_logo_path: &Path) -> Result<Vec<u8>> {
    // Non-Windows platforms always use placeholder
    Ok(create_placeholder_rgba())
}

/// Create a placeholder icon as RGBA bytes (32x32 pixels, 4096 bytes)
///
/// Returns a simple gray square with a border, matching the format used by
/// Win32 icon extraction.
fn create_placeholder_rgba() -> Vec<u8> {
    let size = (ICON_SIZE * ICON_SIZE * 4) as usize;
    let mut icon = vec![0u8; size];

    // Create a simple gray square with a border
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let idx = ((y * ICON_SIZE + x) * 4) as usize;

            // Border pixels (darker gray)
            if x == 0 || x == ICON_SIZE - 1 || y == 0 || y == ICON_SIZE - 1 {
                icon[idx] = 64; // R
                icon[idx + 1] = 64; // G
                icon[idx + 2] = 64; // B
                icon[idx + 3] = 255; // A
            } else {
                // Interior pixels (lighter gray)
                icon[idx] = 128; // R
                icon[idx + 1] = 128; // G
                icon[idx + 2] = 128; // B
                icon[idx + 3] = 255; // A
            }
        }
    }

    icon
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_placeholder_icon_is_valid_rgba() {
        let placeholder = create_placeholder_rgba();

        // Verify size (32x32 RGBA = 4096 bytes)
        assert_eq!(placeholder.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert_eq!(placeholder.len(), 4096);

        // Verify it's not all zeros
        assert!(placeholder.iter().any(|&b| b != 0));

        // Verify alpha channel is fully opaque (255) for all pixels
        for i in (0..placeholder.len()).step_by(4) {
            assert_eq!(placeholder[i + 3], 255, "Alpha channel should be 255");
        }
    }

    #[test]
    fn test_extract_icon_nonexistent_file() {
        let result = extract_icon(Path::new("/nonexistent/path/icon.png"));

        // Should succeed with placeholder icon
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data.len(), 4096); // 32x32 RGBA
        assert_eq!(data, create_placeholder_rgba());
    }

    #[test]
    fn test_extract_icon_valid_png() {
        // Create a minimal valid 1x1 PNG file
        let mut temp_file = NamedTempFile::new().unwrap();
        let png_data = &[
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
        temp_file.write_all(png_data).unwrap();
        temp_file.flush().unwrap();

        let result = extract_icon(temp_file.path());

        // Should succeed and return RGBA data
        assert!(result.is_ok());
        let data = result.unwrap();
        // Should be resized to 32x32 RGBA
        assert_eq!(data.len(), 4096);
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
        assert_eq!(data.len(), 4096);
        assert_eq!(data, create_placeholder_rgba());
    }

    #[test]
    fn test_extract_icon_invalid_png() {
        // Create a file with invalid PNG data
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"not a valid PNG file").unwrap();
        temp_file.flush().unwrap();

        let result = extract_icon(temp_file.path());

        // Should succeed with placeholder icon (graceful degradation)
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data.len(), 4096);
        assert_eq!(data, create_placeholder_rgba());
    }

    #[test]
    fn test_create_placeholder_rgba_format() {
        let placeholder = create_placeholder_rgba();

        // Check size
        assert_eq!(placeholder.len(), 4096);

        // Check that border pixels are darker (64, 64, 64, 255)
        // Top-left corner
        assert_eq!(placeholder[0], 64); // R
        assert_eq!(placeholder[1], 64); // G
        assert_eq!(placeholder[2], 64); // B
        assert_eq!(placeholder[3], 255); // A

        // Check that interior pixels are lighter (128, 128, 128, 255)
        // Pixel at (1, 1) - second row, second column
        let idx = ((ICON_SIZE + 1) * 4) as usize;
        assert_eq!(placeholder[idx], 128); // R
        assert_eq!(placeholder[idx + 1], 128); // G
        assert_eq!(placeholder[idx + 2], 128); // B
        assert_eq!(placeholder[idx + 3], 255); // A
    }
}
