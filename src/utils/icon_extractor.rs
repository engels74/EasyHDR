//! Icon extraction from executables
//!
//! This module provides functionality to extract icons from Windows executables
//! using the Windows Shell32 API.
//!
//! # Requirements
//!
//! - Requirement 1.3: Extract and cache application icons when adding applications

use crate::error::Result;
use std::path::Path;
use tracing::debug;

#[cfg(windows)]
use crate::error::EasyHdrError;

#[cfg(windows)]
use tracing::warn;

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, SelectObject, BITMAP,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
#[cfg(windows)]
use windows::Win32::UI::Shell::{
    ExtractIconExW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::HICON;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

/// Default icon size for extraction (32x32 pixels)
#[cfg(windows)]
const ICON_SIZE: usize = 32;

/// Extract icon from an executable file
///
/// This function uses Windows Shell32 ExtractIconExW to extract the application icon
/// from an executable file and converts it to raw RGBA bytes.
///
/// # Arguments
///
/// * `path` - Path to the executable file
///
/// # Returns
///
/// Returns a Vec<u8> containing RGBA pixel data (32x32 pixels = 4096 bytes)
/// Returns an empty Vec if extraction fails (graceful fallback)
///
/// # Requirements
///
/// - Requirement 1.3: Extract and cache application icons
///
/// # Implementation Notes
///
/// On Windows:
/// - Uses ExtractIconExW to get HICON handle
/// - Converts HICON to raw RGBA bitmap data
/// - Handles extraction failures gracefully with default icon fallback
///
/// On non-Windows platforms:
/// - Returns empty Vec (stub implementation for testing)
pub fn extract_icon_from_exe(#[allow(unused_variables)] path: &Path) -> Result<Vec<u8>> {
    #[cfg(windows)]
    {
        extract_icon_from_exe_windows(path)
    }

    #[cfg(not(windows))]
    {
        // Stub implementation for non-Windows platforms (testing)
        debug!("Icon extraction not supported on non-Windows platforms");
        Ok(Vec::new())
    }
}

/// Windows-specific icon extraction implementation
#[cfg(windows)]
fn extract_icon_from_exe_windows(path: &Path) -> Result<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;

    // Convert path to wide string for Windows API
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    debug!("Extracting icon from: {:?}", path);

    // Try to extract icon using ExtractIconExW
    let mut large_icon: HICON = HICON::default();

    unsafe {
        let result = ExtractIconExW(
            PCWSTR(wide_path.as_ptr()),
            0, // Extract first icon
            Some(&mut large_icon as *mut HICON),
            None, // We only need large icon
            1,    // Extract one icon
        );

        if result == 0 {
            warn!(
                "ExtractIconExW failed for {:?}, trying SHGetFileInfoW",
                path
            );
            // Fallback to SHGetFileInfoW
            return extract_icon_using_shgetfileinfo(&wide_path);
        }
    }

    // Convert HICON to RGBA bytes
    let icon_data = match hicon_to_rgba_bytes(large_icon) {
        Ok(data) => data,
        Err(e) => {
            warn!("Failed to convert HICON to RGBA: {}, using default icon", e);
            // Cleanup icon handle
            unsafe {
                let _ = DestroyIcon(large_icon);
            }
            return Ok(create_default_icon());
        }
    };

    // Cleanup icon handle
    unsafe {
        let _ = DestroyIcon(large_icon);
    }

    debug!("Successfully extracted icon: {} bytes", icon_data.len());
    Ok(icon_data)
}

/// Fallback icon extraction using SHGetFileInfoW
#[cfg(windows)]
fn extract_icon_using_shgetfileinfo(wide_path: &[u16]) -> Result<Vec<u8>> {
    use std::mem::zeroed;

    unsafe {
        let mut file_info: SHFILEINFOW = zeroed();

        let result = SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut file_info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if result == 0 {
            tracing::warn!("SHGetFileInfoW failed, using default icon");
            return Ok(create_default_icon());
        }

        let icon_data = match hicon_to_rgba_bytes(file_info.hIcon) {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!("Failed to convert HICON to RGBA: {}, using default icon", e);
                let _ = DestroyIcon(file_info.hIcon);
                return Ok(create_default_icon());
            }
        };

        // Cleanup icon handle
        let _ = DestroyIcon(file_info.hIcon);

        Ok(icon_data)
    }
}

/// Convert HICON to RGBA bytes
///
/// This function converts a Windows HICON handle to raw RGBA bitmap data.
/// The output is a 32x32 pixel image with 4 bytes per pixel (RGBA).
#[cfg(windows)]
fn hicon_to_rgba_bytes(hicon: HICON) -> Result<Vec<u8>> {
    use std::mem::zeroed;

    unsafe {
        // Get icon information
        let mut icon_info: ICONINFO = zeroed();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            return Err(EasyHdrError::WindowsApiError(
                windows::core::Error::from_thread(),
            ));
        }

        // We need to cleanup these bitmaps when done
        let color_bitmap = icon_info.hbmColor;
        let mask_bitmap = icon_info.hbmMask;

        // Get bitmap information
        let mut bitmap: BITMAP = zeroed();
        if GetObjectW(
            color_bitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut BITMAP as *mut _),
        ) == 0
        {
            let _ = DeleteObject(color_bitmap.into());
            let _ = DeleteObject(mask_bitmap.into());
            return Err(EasyHdrError::WindowsApiError(
                windows::core::Error::from_thread(),
            ));
        }

        let width = bitmap.bmWidth as usize;
        let height = bitmap.bmHeight as usize;

        // Create a device context
        let hdc = CreateCompatibleDC(None);
        if hdc.is_invalid() {
            let _ = DeleteObject(color_bitmap.into());
            let _ = DeleteObject(mask_bitmap.into());
            return Err(EasyHdrError::WindowsApiError(
                windows::core::Error::from_thread(),
            ));
        }

        // Select the bitmap into the DC
        let old_bitmap = SelectObject(hdc, color_bitmap.into());

        // Prepare BITMAPINFO structure
        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32); // Negative for top-down DIB
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        // Allocate buffer for bitmap data (BGRA format)
        let mut buffer = vec![0u8; width * height * 4];

        // Get the bitmap bits
        let result = GetDIBits(
            hdc,
            color_bitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        // Cleanup
        let _ = SelectObject(hdc, old_bitmap);
        let _ = DeleteDC(hdc);
        let _ = DeleteObject(color_bitmap.into());
        let _ = DeleteObject(mask_bitmap.into());

        if result == 0 {
            return Err(EasyHdrError::WindowsApiError(
                windows::core::Error::from_thread(),
            ));
        }

        // Convert BGRA to RGBA
        for i in (0..buffer.len()).step_by(4) {
            buffer.swap(i, i + 2); // Swap B and R
        }

        // Resize to standard icon size if needed
        if width != ICON_SIZE || height != ICON_SIZE {
            buffer = resize_icon_simple(&buffer, width, height, ICON_SIZE, ICON_SIZE);
        }

        Ok(buffer)
    }
}

/// Simple nearest-neighbor image resize
///
/// This is a basic resize implementation for icon data.
/// For production use, consider using a proper image library.
#[cfg(windows)]
fn resize_icon_simple(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> Vec<u8> {
    let mut dst = vec![0u8; dst_width * dst_height * 4];

    for y in 0..dst_height {
        for x in 0..dst_width {
            let src_x = (x * src_width) / dst_width;
            let src_y = (y * src_height) / dst_height;

            let src_idx = (src_y * src_width + src_x) * 4;
            let dst_idx = (y * dst_width + x) * 4;

            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
    }

    dst
}

/// Create a default icon (simple colored square)
///
/// This is used as a fallback when icon extraction fails.
/// Returns a 32x32 RGBA image with a simple pattern.
#[cfg(windows)]
fn create_default_icon() -> Vec<u8> {
    let size = ICON_SIZE * ICON_SIZE * 4;
    let mut icon = vec![0u8; size];

    // Create a simple gray square with a border
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let idx = (y * ICON_SIZE + x) * 4;

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

/// Extract display name from executable metadata
///
/// This function queries the FileDescription property from the executable's
/// version information resources. If the metadata is unavailable, it falls
/// back to using the filename without extension.
///
/// # Arguments
///
/// * `path` - Path to the executable file
///
/// # Returns
///
/// Returns the display name extracted from file metadata or filename
///
/// # Requirements
///
/// - Requirement 1.2: Extract display name from file metadata with fallback to filename
///
/// # Implementation Notes
///
/// On Windows:
/// - Uses GetFileVersionInfoW and VerQueryValueW to read version info
/// - Queries FileDescription from version resources
/// - Falls back to filename if metadata unavailable
///
/// On non-Windows platforms:
/// - Returns filename without extension (stub implementation)
pub fn extract_display_name_from_exe(path: &Path) -> Result<String> {
    #[cfg(windows)]
    {
        extract_display_name_windows(path)
    }

    #[cfg(not(windows))]
    {
        // Stub implementation for non-Windows platforms
        Ok(path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown Application")
            .to_string())
    }
}

/// Windows-specific display name extraction
#[cfg(windows)]
fn extract_display_name_windows(path: &Path) -> Result<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    // Convert path to wide string
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    debug!("Extracting display name from: {:?}", path);

    unsafe {
        // Get the size of version info
        let mut handle: u32 = 0;
        let size = GetFileVersionInfoSizeW(PCWSTR(wide_path.as_ptr()), Some(&mut handle));

        if size == 0 {
            debug!("No version info available, using filename");
            return Ok(get_filename_fallback(path));
        }

        // Allocate buffer for version info
        let mut buffer = vec![0u8; size as usize];

        // Get version info
        if GetFileVersionInfoW(
            PCWSTR(wide_path.as_ptr()),
            Some(handle),
            size,
            buffer.as_mut_ptr() as *mut _,
        )
        .is_err()
        {
            debug!("GetFileVersionInfoW failed, using filename");
            return Ok(get_filename_fallback(path));
        }

        // Query for FileDescription
        // Try common language/codepage combinations
        let queries = [
            "\\StringFileInfo\\040904B0\\FileDescription\0", // English (US)
            "\\StringFileInfo\\040904E4\\FileDescription\0", // English (US) Unicode
            "\\StringFileInfo\\000004B0\\FileDescription\0", // Language neutral
        ];

        for query in &queries {
            let query_wide: Vec<u16> = query.encode_utf16().collect();
            let mut value_ptr: *mut u16 = std::ptr::null_mut();
            let mut value_len: u32 = 0;

            if VerQueryValueW(
                buffer.as_ptr() as *const _,
                PCWSTR(query_wide.as_ptr()),
                &mut value_ptr as *mut *mut u16 as *mut *mut _,
                &mut value_len,
            )
            .as_bool()
                && !value_ptr.is_null()
                && value_len > 0
            {
                // Convert wide string to Rust String
                let description_slice = std::slice::from_raw_parts(value_ptr, value_len as usize);

                // Find null terminator
                let len = description_slice
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(description_slice.len());

                if let Ok(description) = String::from_utf16(&description_slice[..len]) {
                    if !description.is_empty() {
                        debug!("Extracted display name: {}", description);
                        return Ok(description);
                    }
                }
            }
        }

        // Fallback to filename if no description found
        debug!("No FileDescription found, using filename");
        Ok(get_filename_fallback(path))
    }
}

/// Get filename without extension as fallback display name
#[cfg(windows)]
fn get_filename_fallback(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown Application")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[cfg(windows)]
    #[test]
    fn test_get_filename_fallback() {
        let path = PathBuf::from("C:\\Program Files\\Test\\MyApp.exe");
        let name = get_filename_fallback(&path);
        assert_eq!(name, "MyApp");
    }

    #[cfg(windows)]
    #[test]
    fn test_get_filename_fallback_no_extension() {
        let path = PathBuf::from("C:\\Program Files\\Test\\MyApp");
        let name = get_filename_fallback(&path);
        assert_eq!(name, "MyApp");
    }

    #[test]
    fn test_extract_display_name_fallback() {
        // This should work on all platforms
        let path = PathBuf::from("C:\\Program Files\\Test\\MyApp.exe");
        let result = extract_display_name_from_exe(&path);
        assert!(result.is_ok());
        let name = result.unwrap();
        assert!(!name.is_empty());
    }

    #[test]
    fn test_extract_icon_returns_ok() {
        // This should work on all platforms (returns empty vec on non-Windows)
        let path = PathBuf::from("C:\\Windows\\System32\\notepad.exe");
        let result = extract_icon_from_exe(&path);
        assert!(result.is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn test_create_default_icon_size() {
        let icon = create_default_icon();
        // 32x32 pixels * 4 bytes per pixel (RGBA)
        assert_eq!(icon.len(), 32 * 32 * 4);
    }

    #[cfg(windows)]
    #[test]
    fn test_create_default_icon_has_alpha() {
        let icon = create_default_icon();
        // Check that alpha channel is set (every 4th byte should be 255)
        for i in (3..icon.len()).step_by(4) {
            assert_eq!(icon[i], 255);
        }
    }
}
