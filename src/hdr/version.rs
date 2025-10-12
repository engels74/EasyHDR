//! Windows version detection
//!
//! This module provides functionality to detect the Windows version
//! to determine which HDR APIs to use.

#[cfg(windows)]
use windows::Win32::System::SystemInformation::{GetVersionExW, OSVERSIONINFOEXW};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
#[cfg(windows)]
use windows::core::HSTRING;
#[cfg(windows)]
use std::mem::size_of;

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
    /// Uses RtlGetVersion from ntdll.dll to get accurate version information.
    /// Falls back to GetVersionExW if RtlGetVersion is unavailable.
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
                Ok(version) => {
                    return Ok(version);
                }
                Err(_e) => {
                    return Err(crate::error::EasyHdrError::WindowsApiError(
                        windows::core::Error::from_win32()
                    ));
                }
            }
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing purposes)
            Ok(WindowsVersion::Windows11)
        }
    }

    /// Detect Windows version using RtlGetVersion from ntdll.dll
    ///
    /// This is the most reliable method as it's not subject to compatibility shims.
    #[cfg(windows)]
    fn detect_with_rtl_get_version() -> crate::error::Result<Self> {
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
                    "RtlGetVersion not found in ntdll.dll".to_string()
                ));
            }

            // Define the function signature for RtlGetVersion
            type RtlGetVersionFn = unsafe extern "system" fn(*mut OSVERSIONINFOEXW) -> i32;
            let rtl_get_version: RtlGetVersionFn = transmute(rtl_get_version_ptr);

            // Prepare version info structure
            let mut version_info = OSVERSIONINFOEXW::default();
            version_info.dwOSVersionInfoSize = size_of::<OSVERSIONINFOEXW>() as u32;

            // Call RtlGetVersion
            let status = rtl_get_version(&mut version_info);

            if status != 0 {
                return Err(crate::error::EasyHdrError::HdrControlFailed(
                    format!("RtlGetVersion failed with status: {}", status)
                ));
            }

            // Parse build number to determine version
            Ok(Self::parse_build_number(version_info.dwBuildNumber))
        }
    }

    /// Detect Windows version using GetVersionExW (fallback method)
    ///
    /// This method may be affected by compatibility shims but serves as a fallback.
    #[cfg(windows)]
    fn detect_with_get_version_ex() -> crate::error::Result<Self> {
        unsafe {
            let mut version_info = OSVERSIONINFOEXW::default();
            version_info.dwOSVersionInfoSize = size_of::<OSVERSIONINFOEXW>() as u32;

            // Call GetVersionExW
            let result = GetVersionExW(&mut version_info as *mut _ as *mut _);

            if result.as_bool() {
                Ok(Self::parse_build_number(version_info.dwBuildNumber))
            } else {
                Err(crate::error::EasyHdrError::WindowsApiError(
                    windows::core::Error::from_win32()
                ))
            }
        }
    }

    /// Parse Windows build number to determine version variant
    ///
    /// # Arguments
    ///
    /// * `build_number` - The Windows build number from OSVERSIONINFOEXW
    ///
    /// # Returns
    ///
    /// The corresponding WindowsVersion enum variant
    fn parse_build_number(build_number: u32) -> Self {
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
            println!("Detected Windows version: {:?}", v);
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
    fn test_version_enum_equality() {
        assert_eq!(WindowsVersion::Windows10, WindowsVersion::Windows10);
        assert_eq!(WindowsVersion::Windows11, WindowsVersion::Windows11);
        assert_eq!(WindowsVersion::Windows11_24H2, WindowsVersion::Windows11_24H2);

        assert_ne!(WindowsVersion::Windows10, WindowsVersion::Windows11);
        assert_ne!(WindowsVersion::Windows11, WindowsVersion::Windows11_24H2);
    }

    #[test]
    fn test_version_enum_debug() {
        // Ensure Debug trait works correctly
        let v1 = WindowsVersion::Windows10;
        let v2 = WindowsVersion::Windows11;
        let v3 = WindowsVersion::Windows11_24H2;

        assert_eq!(format!("{:?}", v1), "Windows10");
        assert_eq!(format!("{:?}", v2), "Windows11");
        assert_eq!(format!("{:?}", v3), "Windows11_24H2");
    }
}

