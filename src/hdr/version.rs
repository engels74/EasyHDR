//! Windows version detection
//!
//! This module provides functionality to detect the Windows version
//! to determine which HDR APIs to use.

#[cfg(windows)]
use std::mem::size_of;
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
#[cfg(windows)]
use windows::Win32::System::SystemInformation::{GetVersionExW, OSVERSIONINFOEXW};
#[cfg(windows)]
use windows::core::HSTRING;

/// Windows version enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsVersion {
    /// Windows 10
    Windows10,
    /// Windows 11 (before 24H2)
    Windows11,
    /// Windows 11 24H2 or later (build 26100+)
    Windows11_24H2,
}

impl WindowsVersion {
    /// Detect the current Windows version
    ///
    /// Uses `RtlGetVersion` from ntdll.dll to get accurate version information.
    /// Falls back to `GetVersionExW` if `RtlGetVersion` is unavailable.
    ///
    /// # Returns
    ///
    /// Returns the detected Windows version based on build number:
    /// - Build >= 26100: Windows 11 24H2 or later
    /// - Build >= 22000: Windows 11 (before 24H2)
    /// - Build < 22000: Windows 10
    ///
    /// # Errors
    ///
    /// Returns an error if version detection fails completely.
    pub fn detect() -> crate::error::Result<Self> {
        #[cfg(windows)]
        {
            // Try RtlGetVersion first (most reliable method)
            match Self::detect_with_rtl_get_version() {
                Ok(version) => {
                    return Ok(version);
                }
                Err(_e) => {
                    // Silently fall back to GetVersionExW
                }
            }

            // Fallback to GetVersionExW
            match Self::detect_with_get_version_ex() {
                Ok(version) => Ok(version),
                Err(_e) => Err(crate::error::EasyHdrError::WindowsApiError(
                    windows::core::Error::from_thread(),
                )),
            }
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing purposes)
            Ok(WindowsVersion::Windows11)
        }
    }

    /// Get the current Windows build number
    ///
    /// Uses `RtlGetVersion` from ntdll.dll to get accurate build number.
    /// Falls back to `GetVersionExW` if `RtlGetVersion` is unavailable.
    ///
    /// # Returns
    ///
    /// Returns the Windows build number (e.g., 19044, 22621, 26100)
    ///
    /// # Errors
    ///
    /// Returns an error if version detection fails completely.
    pub fn get_build_number() -> crate::error::Result<u32> {
        #[cfg(windows)]
        {
            // Try RtlGetVersion first (most reliable method)
            match Self::get_build_number_with_rtl_get_version() {
                Ok(build) => {
                    return Ok(build);
                }
                Err(_e) => {
                    // Silently fall back to GetVersionExW
                }
            }

            // Fallback to GetVersionExW
            Self::get_build_number_with_get_version_ex()
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing purposes)
            Ok(22621) // Return a typical Windows 11 build number
        }
    }

    /// Detect Windows version using `RtlGetVersion` from ntdll.dll
    ///
    /// This is the most reliable method as it's not subject to compatibility shims.
    #[cfg(windows)]
    fn detect_with_rtl_get_version() -> crate::error::Result<Self> {
        let build_number = Self::get_build_number_with_rtl_get_version()?;
        Ok(Self::parse_build_number(build_number))
    }

    /// Get Windows build number using `RtlGetVersion` from ntdll.dll
    ///
    /// This is the most reliable method as it's not subject to compatibility shims.
    ///
    /// # Safety
    ///
    /// This function contains unsafe code that is sound because:
    ///
    /// 1. **Library Loading**: `LoadLibraryW` is called with a valid, null-terminated wide string
    ///    for "ntdll.dll", which is a system DLL guaranteed to exist on all Windows systems.
    ///
    /// 2. **Function Pointer Retrieval**: `GetProcAddress` is called with a valid module handle
    ///    and a valid C string for "RtlGetVersion". We verify the returned pointer is not None
    ///    before proceeding.
    ///
    /// 3. **Transmute Soundness**: The transmute from `Option<FARPROC>` to `RtlGetVersionFn` is
    ///    sound because:
    ///    - We've verified the pointer is not None
    ///    - The function signature `unsafe extern "system" fn(*mut OSVERSIONINFOEXW) -> i32`
    ///      exactly matches the Windows API contract for `RtlGetVersion`
    ///    - The calling convention ("system") matches the Windows ABI
    ///
    /// 4. **Structure Initialization**: The `OSVERSIONINFOEXW` structure is properly initialized
    ///    with the correct size in `dwOSVersionInfoSize`, which is required by the Windows API
    ///    to prevent buffer overruns.
    ///
    /// 5. **API Contract**: We check the return status from `RtlGetVersion` before using the result,
    ///    ensuring we only read valid data.
    ///
    /// # Invariants
    ///
    /// - `ntdll.dll` must exist (guaranteed on all Windows systems)
    /// - `RtlGetVersion` must exist in ntdll.dll (guaranteed since Windows 2000)
    /// - The structure size must be set correctly before calling `RtlGetVersion`
    #[cfg(windows)]
    #[expect(
        unsafe_code,
        reason = "Windows FFI for RtlGetVersion via ntdll.dll to detect Windows build number"
    )]
    fn get_build_number_with_rtl_get_version() -> crate::error::Result<u32> {
        // Define the function signature for RtlGetVersion
        type RtlGetVersionFn = unsafe extern "system" fn(*mut OSVERSIONINFOEXW) -> i32;

        use std::mem::transmute;

        unsafe {
            // Load ntdll.dll
            let ntdll_name = HSTRING::from("ntdll.dll");
            let ntdll = LoadLibraryW(&ntdll_name)?;

            // Get RtlGetVersion function pointer
            let proc_name = windows::core::s!("RtlGetVersion");
            let rtl_get_version_ptr = GetProcAddress(ntdll, proc_name);

            if rtl_get_version_ptr.is_none() {
                return Err(crate::error::EasyHdrError::HdrControlFailed(
                    "RtlGetVersion not found in ntdll.dll".to_string(),
                ));
            }

            let rtl_get_version: RtlGetVersionFn = transmute(rtl_get_version_ptr);

            // Prepare version info structure
            #[expect(
                clippy::cast_possible_truncation,
                reason = "size_of::<OSVERSIONINFOEXW>() is a compile-time constant (284 bytes) that fits in u32"
            )]
            let mut version_info = OSVERSIONINFOEXW {
                dwOSVersionInfoSize: size_of::<OSVERSIONINFOEXW>() as u32,
                ..Default::default()
            };

            // Call RtlGetVersion
            let status = rtl_get_version(&raw mut version_info);

            if status != 0 {
                return Err(crate::error::EasyHdrError::HdrControlFailed(format!(
                    "RtlGetVersion failed with status: {status}",
                )));
            }

            Ok(version_info.dwBuildNumber)
        }
    }

    /// Detect Windows version using `GetVersionExW` (fallback method)
    ///
    /// This method may be affected by compatibility shims but serves as a fallback.
    #[cfg(windows)]
    fn detect_with_get_version_ex() -> crate::error::Result<Self> {
        let build_number = Self::get_build_number_with_get_version_ex()?;
        Ok(Self::parse_build_number(build_number))
    }

    /// Get Windows build number using `GetVersionExW` (fallback method)
    ///
    /// This method may be affected by compatibility shims but serves as a fallback.
    ///
    /// # Safety
    ///
    /// This function contains unsafe code that is sound because:
    ///
    /// 1. **Structure Initialization**: `OSVERSIONINFOEXW` is properly initialized with the
    ///    correct size in `dwOSVersionInfoSize`, which is required by the Windows API to
    ///    prevent buffer overruns.
    ///
    /// 2. **Valid Pointer**: `std::ptr::addr_of_mut!(version_info).cast()` creates a valid
    ///    mutable pointer to the stack-allocated structure with correct alignment.
    ///
    /// 3. **API Contract**: `GetVersionExW` is called with a properly initialized structure.
    ///    The return value is checked via `is_ok()` before accessing the output data.
    ///
    /// 4. **Error Handling**: If the API call fails, we retrieve the error via
    ///    `Error::from_thread()` and propagate it, ensuring we never read invalid data.
    #[cfg(windows)]
    #[expect(
        unsafe_code,
        reason = "Windows FFI for GetVersionExW fallback method to detect Windows build number"
    )]
    fn get_build_number_with_get_version_ex() -> crate::error::Result<u32> {
        unsafe {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "size_of::<OSVERSIONINFOEXW>() is a compile-time constant (284 bytes) that fits in u32"
            )]
            let mut version_info = OSVERSIONINFOEXW {
                dwOSVersionInfoSize: size_of::<OSVERSIONINFOEXW>() as u32,
                ..Default::default()
            };

            // Call GetVersionExW
            let result = GetVersionExW(std::ptr::addr_of_mut!(version_info).cast());

            if result.is_ok() {
                Ok(version_info.dwBuildNumber)
            } else {
                Err(crate::error::EasyHdrError::WindowsApiError(
                    windows::core::Error::from_thread(),
                ))
            }
        }
    }

    /// Parse Windows build number to determine version variant
    ///
    /// This method is public to allow for testing with specific build numbers.
    ///
    /// # Arguments
    ///
    /// * `build_number` - The Windows build number from OSVERSIONINFOEXW
    ///
    /// # Returns
    ///
    /// The corresponding `WindowsVersion` enum variant
    ///
    /// # Examples
    ///
    /// ```
    /// use easyhdr::hdr::version::WindowsVersion;
    ///
    /// // Windows 10 21H2
    /// let version = WindowsVersion::parse_build_number(19044);
    /// assert_eq!(version, WindowsVersion::Windows10);
    ///
    /// // Windows 11 22H2
    /// let version = WindowsVersion::parse_build_number(22621);
    /// assert_eq!(version, WindowsVersion::Windows11);
    ///
    /// // Windows 11 24H2
    /// let version = WindowsVersion::parse_build_number(26100);
    /// assert_eq!(version, WindowsVersion::Windows11_24H2);
    /// ```
    pub fn parse_build_number(build_number: u32) -> Self {
        if build_number >= 26100 {
            // Windows 11 24H2 or later
            WindowsVersion::Windows11_24H2
        } else if build_number >= 22000 {
            // Windows 11 (before 24H2)
            WindowsVersion::Windows11
        } else {
            // Windows 10
            WindowsVersion::Windows10
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_detection() {
        // This test will actually detect the current Windows version
        let version = WindowsVersion::detect();
        assert!(version.is_ok());

        // Log the detected version for debugging
        if let Ok(v) = version {
            println!("Detected Windows version: {v:?}");
        }
    }

    #[test]
    fn test_parse_build_number_windows10() {
        // Windows 10 21H2 build number
        let version = WindowsVersion::parse_build_number(19044);
        assert_eq!(version, WindowsVersion::Windows10);

        // Windows 10 22H2 build number
        let version = WindowsVersion::parse_build_number(19045);
        assert_eq!(version, WindowsVersion::Windows10);
    }

    #[test]
    fn test_parse_build_number_windows11() {
        // Windows 11 21H2 build number
        let version = WindowsVersion::parse_build_number(22000);
        assert_eq!(version, WindowsVersion::Windows11);

        // Windows 11 22H2 build number
        let version = WindowsVersion::parse_build_number(22621);
        assert_eq!(version, WindowsVersion::Windows11);

        // Windows 11 23H2 build number
        let version = WindowsVersion::parse_build_number(22631);
        assert_eq!(version, WindowsVersion::Windows11);
    }

    #[test]
    fn test_parse_build_number_windows11_24h2() {
        // Windows 11 24H2 build number
        let version = WindowsVersion::parse_build_number(26100);
        assert_eq!(version, WindowsVersion::Windows11_24H2);

        // Future build numbers
        let version = WindowsVersion::parse_build_number(26200);
        assert_eq!(version, WindowsVersion::Windows11_24H2);
    }

    #[test]
    fn test_parse_build_number_edge_cases() {
        // Just below Windows 11 threshold
        let version = WindowsVersion::parse_build_number(21999);
        assert_eq!(version, WindowsVersion::Windows10);

        // Exactly at Windows 11 threshold
        let version = WindowsVersion::parse_build_number(22000);
        assert_eq!(version, WindowsVersion::Windows11);

        // Just below Windows 11 24H2 threshold
        let version = WindowsVersion::parse_build_number(26099);
        assert_eq!(version, WindowsVersion::Windows11);

        // Exactly at Windows 11 24H2 threshold
        let version = WindowsVersion::parse_build_number(26100);
        assert_eq!(version, WindowsVersion::Windows11_24H2);
    }

    #[test]
    fn test_parse_build_number_comprehensive() {
        // Test a comprehensive range of build numbers to ensure correct classification

        // Very old Windows 10 builds
        assert_eq!(
            WindowsVersion::parse_build_number(10240),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(14393),
            WindowsVersion::Windows10
        );

        // Windows 10 1809 through 22H2
        assert_eq!(
            WindowsVersion::parse_build_number(17763),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(18362),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(18363),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(19041),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(19042),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(19043),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(19044),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(19045),
            WindowsVersion::Windows10
        );

        // Windows 11 versions
        assert_eq!(
            WindowsVersion::parse_build_number(22000),
            WindowsVersion::Windows11
        );
        assert_eq!(
            WindowsVersion::parse_build_number(22621),
            WindowsVersion::Windows11
        );
        assert_eq!(
            WindowsVersion::parse_build_number(22631),
            WindowsVersion::Windows11
        );

        // Windows 11 24H2 and beyond
        assert_eq!(
            WindowsVersion::parse_build_number(26100),
            WindowsVersion::Windows11_24H2
        );
        assert_eq!(
            WindowsVersion::parse_build_number(26200),
            WindowsVersion::Windows11_24H2
        );
        assert_eq!(
            WindowsVersion::parse_build_number(30000),
            WindowsVersion::Windows11_24H2
        );
    }

    #[test]
    fn test_parse_build_number_boundary_values() {
        // Test boundary values around version thresholds

        // Around Windows 11 threshold (22000)
        assert_eq!(
            WindowsVersion::parse_build_number(21998),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(21999),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::parse_build_number(22000),
            WindowsVersion::Windows11
        );
        assert_eq!(
            WindowsVersion::parse_build_number(22001),
            WindowsVersion::Windows11
        );

        // Around Windows 11 24H2 threshold (26100)
        assert_eq!(
            WindowsVersion::parse_build_number(26098),
            WindowsVersion::Windows11
        );
        assert_eq!(
            WindowsVersion::parse_build_number(26099),
            WindowsVersion::Windows11
        );
        assert_eq!(
            WindowsVersion::parse_build_number(26100),
            WindowsVersion::Windows11_24H2
        );
        assert_eq!(
            WindowsVersion::parse_build_number(26101),
            WindowsVersion::Windows11_24H2
        );
    }

    #[test]
    fn test_version_enum_equality() {
        assert_eq!(WindowsVersion::Windows10, WindowsVersion::Windows10);
        assert_eq!(WindowsVersion::Windows11, WindowsVersion::Windows11);
        assert_eq!(
            WindowsVersion::Windows11_24H2,
            WindowsVersion::Windows11_24H2
        );

        assert_ne!(WindowsVersion::Windows10, WindowsVersion::Windows11);
        assert_ne!(WindowsVersion::Windows11, WindowsVersion::Windows11_24H2);
    }

    #[test]
    fn test_version_enum_debug() {
        // Ensure Debug trait works correctly
        let v1 = WindowsVersion::Windows10;
        let v2 = WindowsVersion::Windows11;
        let v3 = WindowsVersion::Windows11_24H2;

        assert_eq!(format!("{v1:?}"), "Windows10");
        assert_eq!(format!("{v2:?}"), "Windows11");
        assert_eq!(format!("{v3:?}"), "Windows11_24H2");
    }

    #[test]
    fn test_version_enum_clone() {
        // Ensure Clone trait works correctly
        let v1 = WindowsVersion::Windows10;
        let v2 = v1; // Copy trait is used automatically
        assert_eq!(v1, v2);

        let v3 = WindowsVersion::Windows11_24H2;
        let v4 = v3; // Copy trait is used automatically
        assert_eq!(v3, v4);
    }

    #[test]
    fn test_version_enum_copy() {
        // Ensure Copy trait works correctly
        let v1 = WindowsVersion::Windows11;
        let v2 = v1; // Copy, not move
        assert_eq!(v1, v2);
        // v1 should still be usable after copy
        assert_eq!(v1, WindowsVersion::Windows11);
    }

    // Tests for Windows API response simulation
    // Note: These tests verify the logic flow and error handling
    // since we cannot easily mock Windows API calls in unit tests

    #[test]
    #[cfg(not(windows))]
    fn test_version_detection_non_windows() {
        // On non-Windows platforms, should return Windows11 as default
        let version = WindowsVersion::detect();
        assert!(version.is_ok());
        assert_eq!(version.unwrap(), WindowsVersion::Windows11);
    }

    #[test]
    #[cfg(windows)]
    fn test_version_detection_windows_api_success() {
        // This test verifies that version detection succeeds on Windows
        // It will use the actual Windows API, so the result depends on the test environment
        let version = WindowsVersion::detect();

        // Should succeed on any Windows system
        assert!(
            version.is_ok(),
            "Version detection should succeed on Windows"
        );

        let detected = version.unwrap();

        // Should be one of the three valid versions
        assert!(
            matches!(
                detected,
                WindowsVersion::Windows10
                    | WindowsVersion::Windows11
                    | WindowsVersion::Windows11_24H2
            ),
            "Detected version should be one of the valid Windows versions"
        );
    }

    #[test]
    fn test_parse_build_number_zero() {
        // Edge case: build number 0 (should never happen in practice)
        let version = WindowsVersion::parse_build_number(0);
        assert_eq!(version, WindowsVersion::Windows10);
    }

    #[test]
    fn test_parse_build_number_max_u32() {
        // Edge case: maximum u32 value
        let version = WindowsVersion::parse_build_number(u32::MAX);
        assert_eq!(version, WindowsVersion::Windows11_24H2);
    }

    #[test]
    fn test_parse_build_number_known_versions() {
        // Test specific known Windows versions for accuracy

        // Windows 10 versions
        assert_eq!(
            WindowsVersion::parse_build_number(10240),
            WindowsVersion::Windows10
        ); // 1507
        assert_eq!(
            WindowsVersion::parse_build_number(10586),
            WindowsVersion::Windows10
        ); // 1511
        assert_eq!(
            WindowsVersion::parse_build_number(14393),
            WindowsVersion::Windows10
        ); // 1607
        assert_eq!(
            WindowsVersion::parse_build_number(15063),
            WindowsVersion::Windows10
        ); // 1703
        assert_eq!(
            WindowsVersion::parse_build_number(16299),
            WindowsVersion::Windows10
        ); // 1709
        assert_eq!(
            WindowsVersion::parse_build_number(17134),
            WindowsVersion::Windows10
        ); // 1803
        assert_eq!(
            WindowsVersion::parse_build_number(17763),
            WindowsVersion::Windows10
        ); // 1809
        assert_eq!(
            WindowsVersion::parse_build_number(18362),
            WindowsVersion::Windows10
        ); // 1903
        assert_eq!(
            WindowsVersion::parse_build_number(18363),
            WindowsVersion::Windows10
        ); // 1909
        assert_eq!(
            WindowsVersion::parse_build_number(19041),
            WindowsVersion::Windows10
        ); // 2004
        assert_eq!(
            WindowsVersion::parse_build_number(19042),
            WindowsVersion::Windows10
        ); // 20H2
        assert_eq!(
            WindowsVersion::parse_build_number(19043),
            WindowsVersion::Windows10
        ); // 21H1
        assert_eq!(
            WindowsVersion::parse_build_number(19044),
            WindowsVersion::Windows10
        ); // 21H2
        assert_eq!(
            WindowsVersion::parse_build_number(19045),
            WindowsVersion::Windows10
        ); // 22H2

        // Windows 11 versions
        assert_eq!(
            WindowsVersion::parse_build_number(22000),
            WindowsVersion::Windows11
        ); // 21H2
        assert_eq!(
            WindowsVersion::parse_build_number(22621),
            WindowsVersion::Windows11
        ); // 22H2
        assert_eq!(
            WindowsVersion::parse_build_number(22631),
            WindowsVersion::Windows11
        ); // 23H2

        // Windows 11 24H2
        assert_eq!(
            WindowsVersion::parse_build_number(26100),
            WindowsVersion::Windows11_24H2
        ); // 24H2
    }
}
