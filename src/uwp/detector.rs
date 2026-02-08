//! UWP process detection via Windows `AppModel` APIs
//!
//! This module provides runtime detection of UWP applications by querying process handles
//! for their associated package information. The `GetPackageFullName` API distinguishes
//! between traditional Win32 processes and UWP processes.
//!
//! # Process Detection Flow
//!
//! 1. Call `GetPackageFullName` with null buffer to get required length
//! 2. Allocate UTF-16 buffer with appropriate capacity
//! 3. Call `GetPackageFullName` again to retrieve package full name
//! 4. Handle `APPMODEL_ERROR_NO_PACKAGE` (15700) → Win32 process (return `None`)
//! 5. Extract package family name from full name string
//!
//! # Package Name Format
//!
//! - **Full Name**: `Name_Version_Architecture_ResourceId_PublisherId`
//!   - Example: `Microsoft.WindowsCalculator_10.2103.8.0_x64__8wekyb3d8bbwe`
//!
//! - **Family Name**: `Name_PublisherId`
//!   - Example: `Microsoft.WindowsCalculator_8wekyb3d8bbwe`
//!
//! The family name is stable across version updates and is used to identify monitored apps.

use crate::Result;

/// Check if a process is a UWP application and return its package family name
///
/// Returns the package family name if it's a UWP app, or `None` if it's a Win32 process.
///
/// # Safety
///
/// Caller must ensure `h_process` is a valid, open process handle with
/// `PROCESS_QUERY_LIMITED_INFORMATION` rights that remains valid for the call duration.
///
/// # Errors
///
/// Returns error if Windows API call fails (other than `APPMODEL_ERROR_NO_PACKAGE`),
/// buffer allocation fails, or UTF-16 to UTF-8 conversion fails.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows FFI for UWP package detection via GetPackageFullName"
)]
pub unsafe fn detect_uwp_process(
    h_process: windows::Win32::Foundation::HANDLE,
) -> Result<Option<String>> {
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::Win32::Storage::Packaging::Appx::GetPackageFullName;
    use windows::core::PWSTR;

    // APPMODEL_ERROR_NO_PACKAGE (15700) means this is a Win32 app, not a UWP app
    const APPMODEL_ERROR_NO_PACKAGE: WIN32_ERROR = WIN32_ERROR(15700);
    const ERROR_INSUFFICIENT_BUFFER: WIN32_ERROR = WIN32_ERROR(122);
    const ERROR_ACCESS_DENIED: WIN32_ERROR = WIN32_ERROR(5);

    // First call to get required buffer length
    let mut length: u32 = 0;
    let result = unsafe { GetPackageFullName(h_process, &raw mut length, None) };

    if result == APPMODEL_ERROR_NO_PACKAGE {
        // This is a Win32 application, not a UWP app
        return Ok(None);
    }

    if result == ERROR_ACCESS_DENIED {
        // Access denied - common for system processes (e.g., PID 4 System process)
        // Even with admin rights, some kernel-mode processes cannot be queried
        // Treat as non-UWP since we cannot determine package information
        return Ok(None);
    }

    if result != ERROR_INSUFFICIENT_BUFFER {
        // Unexpected error
        return Err(crate::EasyHdrError::UwpProcessDetectionError(
            crate::error::StringError::new(format!(
                "GetPackageFullName failed with error code {result:?}"
            )),
        ));
    }

    // Allocate buffer for package full name
    // length includes the null terminator
    let mut buffer = vec![0u16; length as usize];

    // Second call to retrieve the actual package full name
    let result =
        unsafe { GetPackageFullName(h_process, &raw mut length, Some(PWSTR(buffer.as_mut_ptr()))) };

    if result != WIN32_ERROR(0) {
        // ERROR_SUCCESS is 0
        return Err(crate::EasyHdrError::UwpProcessDetectionError(
            crate::error::StringError::new(format!(
                "GetPackageFullName (second call) failed with error code {result:?}"
            )),
        ));
    }

    // Convert UTF-16 buffer to Rust String
    // Find the null terminator
    let null_pos = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());

    let full_name = String::from_utf16(&buffer[..null_pos]).map_err(|e| {
        crate::EasyHdrError::UwpProcessDetectionError(crate::error::StringError::new(format!(
            "Failed to convert package full name from UTF-16: {e}"
        )))
    })?;

    // Extract package family name from full name
    let family_name = extract_package_family_name(&full_name)?;

    Ok(Some(family_name))
}

/// Extract package family name from package full name
///
/// Extracts `Name_PublisherId` from `Name_Version_Architecture_ResourceId_PublisherId`.
///
/// # Errors
///
/// Returns error if the full name format is invalid or cannot be parsed.
#[cfg(windows)]
pub fn extract_package_family_name(full_name: &str) -> Result<String> {
    // Package full name format: Name_Version_Architecture_ResourceId_PublisherId
    // Package family name format: Name_PublisherId
    //
    // Example:
    // Full:   Microsoft.WindowsCalculator_10.2103.8.0_x64__8wekyb3d8bbwe
    // Family: Microsoft.WindowsCalculator_8wekyb3d8bbwe
    //
    // Strategy: Split by '_' and take first component (Name) and last component (PublisherId)

    let parts: Vec<&str> = full_name.split('_').collect();

    // We expect at least 5 parts: Name, Version, Architecture, ResourceId, PublisherId
    // ResourceId can be empty (represented by two consecutive underscores)
    if parts.len() < 5 {
        return Err(crate::EasyHdrError::PackageFamilyNameExtractionError(
            full_name.to_string(),
        ));
    }

    // First part is the Name
    let name = parts[0];

    // Last part is the PublisherId
    let publisher_id = parts[parts.len() - 1];

    // Validate that both parts are non-empty
    if name.is_empty() || publisher_id.is_empty() {
        return Err(crate::EasyHdrError::InvalidPackageFamilyName(
            full_name.to_string(),
        ));
    }

    // Construct family name: Name_PublisherId
    Ok(format!("{name}_{publisher_id}"))
}

#[cfg(not(windows))]
pub unsafe fn detect_uwp_process(_h_process: ()) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(not(windows))]
pub fn extract_package_family_name(_full_name: &str) -> Result<String> {
    Err(crate::EasyHdrError::UwpProcessDetectionError(
        crate::error::StringError::new("UWP detection only available on Windows"),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_valid() {
        // Test with a typical Windows Calculator package name
        let full_name = "Microsoft.WindowsCalculator_10.2103.8.0_x64__8wekyb3d8bbwe";
        let result = extract_package_family_name(full_name);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Microsoft.WindowsCalculator_8wekyb3d8bbwe");
    }

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_with_empty_resource_id() {
        // Test with empty ResourceId (two consecutive underscores)
        let full_name = "Microsoft.WindowsStore_12011.1001.1.0_x64__8wekyb3d8bbwe";
        let result = extract_package_family_name(full_name);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Microsoft.WindowsStore_8wekyb3d8bbwe");
    }

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_different_architecture() {
        // Test with ARM64 architecture
        let full_name = "Microsoft.Photos_2023.11110.8002.0_arm64__8wekyb3d8bbwe";
        let result = extract_package_family_name(full_name);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Microsoft.Photos_8wekyb3d8bbwe");
    }

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_neutral_architecture() {
        // Test with neutral architecture
        let full_name = "Microsoft.DesktopAppInstaller_1.21.3133.0_neutral__8wekyb3d8bbwe";
        let result = extract_package_family_name(full_name);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "Microsoft.DesktopAppInstaller_8wekyb3d8bbwe"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_too_few_parts() {
        // Test with malformed package name (too few parts)
        let full_name = "Microsoft.WindowsCalculator_10.2103.8.0";
        let result = extract_package_family_name(full_name);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::EasyHdrError::PackageFamilyNameExtractionError(_)
        ));
    }

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_empty_name() {
        // Test with empty name component
        let full_name = "_10.2103.8.0_x64__8wekyb3d8bbwe";
        let result = extract_package_family_name(full_name);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::EasyHdrError::InvalidPackageFamilyName(_)
        ));
    }

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_empty_publisher_id() {
        // Test with empty publisher ID
        let full_name = "Microsoft.WindowsCalculator_10.2103.8.0_x64__";
        let result = extract_package_family_name(full_name);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::EasyHdrError::InvalidPackageFamilyName(_)
        ));
    }

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_single_underscore() {
        // Test with only one underscore (invalid format)
        let full_name = "Microsoft.WindowsCalculator";
        let result = extract_package_family_name(full_name);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::EasyHdrError::PackageFamilyNameExtractionError(_)
        ));
    }

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_extra_underscores() {
        // Test with more than 5 parts (should still work - take first and last)
        let full_name = "Microsoft.WindowsCalculator_10.2103.8.0_x64_extra_part__8wekyb3d8bbwe";
        let result = extract_package_family_name(full_name);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Microsoft.WindowsCalculator_8wekyb3d8bbwe");
    }

    #[test]
    #[cfg(windows)]
    fn test_extract_package_family_name_real_world_examples() {
        // Test with real-world package names
        let test_cases = vec![
            (
                "Microsoft.MicrosoftEdge_44.19041.1266.0_neutral__8wekyb3d8bbwe",
                "Microsoft.MicrosoftEdge_8wekyb3d8bbwe",
            ),
            (
                "Microsoft.Xbox.TCUI_1.24.10001.0_x64__8wekyb3d8bbwe",
                "Microsoft.Xbox.TCUI_8wekyb3d8bbwe",
            ),
            (
                "Microsoft.WindowsTerminal_1.18.3181.0_x64__8wekyb3d8bbwe",
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
            ),
        ];

        for (full_name, expected_family_name) in test_cases {
            let result = extract_package_family_name(full_name);
            assert!(result.is_ok(), "Failed for: {full_name}");
            assert_eq!(
                result.unwrap(),
                expected_family_name,
                "Mismatch for: {full_name}"
            );
        }
    }
}
