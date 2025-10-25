//! UWP process detection via Windows AppModel APIs
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
/// Calls `GetPackageFullName` to determine if the given process handle corresponds to
/// a UWP application. Returns the package family name if it's a UWP app, or `None`
/// if it's a traditional Win32 process.
///
/// # Arguments
///
/// * `h_process` - Valid process handle with `PROCESS_QUERY_LIMITED_INFORMATION` rights
///
/// # Returns
///
/// - `Ok(Some(family_name))` - Process is a UWP app with given package family name
/// - `Ok(None)` - Process is a Win32 application (received `APPMODEL_ERROR_NO_PACKAGE`)
/// - `Err(_)` - Windows API error (other than `NO_PACKAGE`)
///
/// # Safety
///
/// Caller must ensure:
/// - `h_process` is a valid, open process handle with `PROCESS_QUERY_LIMITED_INFORMATION` rights
/// - The handle remains valid for the duration of this call
/// - The handle is not concurrently closed by another thread
///
/// # Errors
///
/// Returns error if:
/// - Windows API call fails with error other than `APPMODEL_ERROR_NO_PACKAGE`
/// - Buffer allocation fails
/// - UTF-16 to UTF-8 string conversion fails
///
/// # Example
///
/// ```no_run
/// # #[cfg(windows)]
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
/// use windows::Win32::Foundation::HANDLE;
///
/// let pid = 1234;
/// let handle = unsafe {
///     OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?
/// };
///
/// let family_name = unsafe { easyhdr::uwp::detect_uwp_process(handle)? };
/// match family_name {
///     Some(name) => println!("UWP app: {}", name),
///     None => println!("Win32 app"),
/// }
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows FFI for UWP package detection via GetPackageFullName"
)]
pub unsafe fn detect_uwp_process(
    _h_process: windows::Win32::Foundation::HANDLE,
) -> Result<Option<String>> {
    // TODO: Implement in task 4.1
    // For now, return None (treat all processes as Win32)
    Ok(None)
}

/// Extract package family name from package full name
///
/// Parses the package full name string to extract the stable package family name
/// identifier. The family name omits version and architecture components.
///
/// # Format
///
/// - **Input (full name)**: `Name_Version_Architecture_ResourceId_PublisherId`
/// - **Output (family name)**: `Name_PublisherId`
///
/// # Arguments
///
/// * `full_name` - Package full name string from `GetPackageFullName`
///
/// # Returns
///
/// The extracted package family name
///
/// # Errors
///
/// Returns error if the full name format is invalid or cannot be parsed
///
/// # Example
///
/// ```
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let full_name = "Microsoft.WindowsCalculator_10.2103.8.0_x64__8wekyb3d8bbwe";
/// let family_name = easyhdr::uwp::extract_package_family_name(full_name)?;
/// assert_eq!(family_name, "Microsoft.WindowsCalculator_8wekyb3d8bbwe");
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn extract_package_family_name(_full_name: &str) -> Result<String> {
    // TODO: Implement in task 4.2
    // For now, return error indicating not implemented
    Err(crate::EasyHdrError::Other(
        "extract_package_family_name not yet implemented".into(),
    ))
}

#[cfg(not(windows))]
pub unsafe fn detect_uwp_process(_h_process: ()) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(not(windows))]
pub fn extract_package_family_name(_full_name: &str) -> Result<String> {
    Err(crate::EasyHdrError::Other(
        "UWP detection only available on Windows".into(),
    ))
}
