//! Windows API structures and definitions for HDR control
//!
//! This module contains Windows API structure definitions and constants
//! needed for HDR control operations.
//!
//! Many of these structures are not available in windows-rs 0.52, so they are
//! manually defined here with #[repr(C)] to match the Windows API layout.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

// Import LUID from windows-rs on Windows, or define a stub for non-Windows platforms
#[cfg(windows)]
pub use windows::Win32::Foundation::LUID;

// For non-Windows platforms (testing), define a stub LUID structure
#[cfg(not(windows))]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LUID {
    pub LowPart: u32,
    pub HighPart: i32,
}

/// DISPLAYCONFIG_DEVICE_INFO_TYPE enumeration values
///
/// Specifies the type of display device info to configure or obtain through
/// DisplayConfigSetDeviceInfo or DisplayConfigGetDeviceInfo.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DISPLAYCONFIG_DEVICE_INFO_TYPE {
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME = 1,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME = 2,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_PREFERRED_MODE = 3,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADAPTER_NAME = 4,
    DISPLAYCONFIG_DEVICE_INFO_SET_TARGET_PERSISTENCE = 5,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_BASE_TYPE = 6,
    DISPLAYCONFIG_DEVICE_INFO_GET_SUPPORT_VIRTUAL_RESOLUTION = 7,
    DISPLAYCONFIG_DEVICE_INFO_SET_SUPPORT_VIRTUAL_RESOLUTION = 8,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO = 9,
    DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE = 10,
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL = 11,
    DISPLAYCONFIG_DEVICE_INFO_GET_MONITOR_SPECIALIZATION = 12,
    DISPLAYCONFIG_DEVICE_INFO_SET_MONITOR_SPECIALIZATION = 13,
    DISPLAYCONFIG_DEVICE_INFO_SET_RESERVED1 = 14,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2 = 15,
    DISPLAYCONFIG_DEVICE_INFO_SET_HDR_STATE = 16,
    DISPLAYCONFIG_DEVICE_INFO_SET_WCG_STATE = 17,
}

/// DISPLAYCONFIG_DEVICE_INFO_HEADER structure
///
/// Contains display information about the device. This is the header for all
/// DisplayConfigGetDeviceInfo and DisplayConfigSetDeviceInfo operations.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DISPLAYCONFIG_DEVICE_INFO_HEADER {
    /// Type of device information to retrieve or set
    pub type_: DISPLAYCONFIG_DEVICE_INFO_TYPE,
    /// Size in bytes of the device information (including header)
    pub size: u32,
    /// Adapter LUID
    pub adapterId: LUID,
    /// Source or target identifier
    pub id: u32,
}

/// DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO structure (Windows 10/11)
///
/// Used to get advanced color information for a display target.
/// This is the legacy structure used on Windows 10 and Windows 11 before 24H2.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
    /// Header
    pub header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    /// Anonymous union containing bit fields
    pub value: u32,
    /// Color encoding (DISPLAYCONFIG_COLOR_ENCODING)
    pub colorEncoding: u32,
    /// Bits per color channel
    pub bitsPerColorChannel: u32,
}

impl DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
    /// Check if advanced color (HDR) is supported
    pub fn advancedColorSupported(&self) -> bool {
        (self.value & 0x1) != 0
    }

    /// Check if advanced color (HDR) is enabled
    pub fn advancedColorEnabled(&self) -> bool {
        (self.value & 0x2) != 0
    }

    /// Check if wide color gamut is enforced
    pub fn wideColorEnforced(&self) -> bool {
        (self.value & 0x4) != 0
    }

    /// Check if advanced color force disabled
    pub fn advancedColorForceDisabled(&self) -> bool {
        (self.value & 0x8) != 0
    }
}

/// DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE structure (Windows 10/11)
///
/// Used to set advanced color state for a display target.
/// This is the legacy structure used on Windows 10 and Windows 11 before 24H2.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE {
    /// Header
    pub header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    /// Anonymous union containing bit fields
    pub value: u32,
}

impl DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE {
    /// Create a new structure to enable or disable advanced color
    pub fn new(adapter_id: LUID, target_id: u32, enable: bool) -> Self {
        Self {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
                size: std::mem::size_of::<Self>() as u32,
                adapterId: adapter_id,
                id: target_id,
            },
            value: if enable { 1 } else { 0 },
        }
    }
}

/// DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 structure (Windows 11 24H2+)
///
/// Used to get advanced color information for a display target on Windows 11 24H2+.
/// This is the new structure that provides more detailed HDR information.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
    /// Header
    pub header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    /// Color encoding (DISPLAYCONFIG_COLOR_ENCODING)
    pub colorEncoding: u32,
    /// Bits per color channel
    pub bitsPerColorChannel: u32,
    /// Active color mode (DISPLAYCONFIG_ADVANCED_COLOR_MODE)
    pub activeColorMode: u32,
    /// Anonymous union containing bit fields
    pub value: u32,
}

impl DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
    /// Check if high dynamic range is supported
    pub fn highDynamicRangeSupported(&self) -> bool {
        (self.value & 0x1) != 0
    }

    /// Check if wide color gamut is supported
    pub fn wideColorGamutSupported(&self) -> bool {
        (self.value & 0x2) != 0
    }
}

/// DISPLAYCONFIG_ADVANCED_COLOR_MODE enumeration
///
/// Specifies the active color mode for a display.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DISPLAYCONFIG_ADVANCED_COLOR_MODE {
    DISPLAYCONFIG_ADVANCED_COLOR_MODE_SDR = 0,
    DISPLAYCONFIG_ADVANCED_COLOR_MODE_WCG = 1,
    DISPLAYCONFIG_ADVANCED_COLOR_MODE_HDR = 2,
}

/// DISPLAYCONFIG_SET_HDR_STATE structure (Windows 11 24H2+)
///
/// Used to set HDR state for a display target on Windows 11 24H2+.
/// This is the new structure that replaces DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DISPLAYCONFIG_SET_HDR_STATE {
    /// Header
    pub header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    /// Anonymous union containing bit fields
    pub value: u32,
}

impl DISPLAYCONFIG_SET_HDR_STATE {
    /// Create a new structure to enable or disable HDR
    pub fn new(adapter_id: LUID, target_id: u32, enable: bool) -> Self {
        Self {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_SET_HDR_STATE,
                size: std::mem::size_of::<Self>() as u32,
                adapterId: adapter_id,
                id: target_id,
            },
            value: if enable { 1 } else { 0 },
        }
    }
}

// DISPLAYCONFIG structures and constants
// These are not available in windows-rs 0.52, so we define them manually

/// QDC_ONLY_ACTIVE_PATHS flag for QueryDisplayConfig
pub const QDC_ONLY_ACTIVE_PATHS: u32 = 0x00000002;

/// DISPLAYCONFIG_PATH_ACTIVE flag
pub const DISPLAYCONFIG_PATH_ACTIVE: u32 = 0x00000001;

/// DISPLAYCONFIG_2DREGION structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_2DREGION {
    pub cx: u32,
    pub cy: u32,
}

/// DISPLAYCONFIG_RATIONAL structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_RATIONAL {
    pub Numerator: u32,
    pub Denominator: u32,
}

/// DISPLAYCONFIG_VIDEO_SIGNAL_INFO structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DISPLAYCONFIG_VIDEO_SIGNAL_INFO {
    pub pixelRate: u64,
    pub hSyncFreq: DISPLAYCONFIG_RATIONAL,
    pub vSyncFreq: DISPLAYCONFIG_RATIONAL,
    pub activeSize: DISPLAYCONFIG_2DREGION,
    pub totalSize: DISPLAYCONFIG_2DREGION,
    pub videoStandard: u32,
    pub scanLineOrdering: u32,
}

impl Default for DISPLAYCONFIG_VIDEO_SIGNAL_INFO {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

/// DISPLAYCONFIG_TARGET_MODE structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_TARGET_MODE {
    pub targetVideoSignalInfo: DISPLAYCONFIG_VIDEO_SIGNAL_INFO,
}

/// DISPLAYCONFIG_SOURCE_MODE structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_SOURCE_MODE {
    pub width: u32,
    pub height: u32,
    pub pixelFormat: u32,
    pub position: DISPLAYCONFIG_2DREGION,
}

/// DISPLAYCONFIG_MODE_INFO_TYPE enumeration
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DISPLAYCONFIG_MODE_INFO_TYPE {
    DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE = 1,
    DISPLAYCONFIG_MODE_INFO_TYPE_TARGET = 2,
}

/// DISPLAYCONFIG_MODE_INFO structure (union)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DISPLAYCONFIG_MODE_INFO {
    pub infoType: DISPLAYCONFIG_MODE_INFO_TYPE,
    pub id: u32,
    pub adapterId: LUID,
    pub modeInfo: DISPLAYCONFIG_MODE_INFO_UNION,
}

/// Union for DISPLAYCONFIG_MODE_INFO
#[repr(C)]
#[derive(Clone, Copy)]
pub union DISPLAYCONFIG_MODE_INFO_UNION {
    pub targetMode: DISPLAYCONFIG_TARGET_MODE,
    pub sourceMode: DISPLAYCONFIG_SOURCE_MODE,
}

impl Default for DISPLAYCONFIG_MODE_INFO {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

impl std::fmt::Debug for DISPLAYCONFIG_MODE_INFO {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DISPLAYCONFIG_MODE_INFO")
            .field("infoType", &self.infoType)
            .field("id", &self.id)
            .field("adapterId", &self.adapterId)
            .finish()
    }
}

/// DISPLAYCONFIG_PATH_SOURCE_INFO structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_PATH_SOURCE_INFO {
    pub adapterId: LUID,
    pub id: u32,
    pub modeInfoIdx: u32,
    pub statusFlags: u32,
}

/// DISPLAYCONFIG_PATH_TARGET_INFO structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DISPLAYCONFIG_PATH_TARGET_INFO {
    pub adapterId: LUID,
    pub id: u32,
    pub modeInfoIdx: u32,
    pub outputTechnology: u32,
    pub rotation: u32,
    pub scaling: u32,
    pub refreshRate: DISPLAYCONFIG_RATIONAL,
    pub scanLineOrdering: u32,
    pub targetAvailable: u32,
    pub statusFlags: u32,
}

impl Default for DISPLAYCONFIG_PATH_TARGET_INFO {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

/// DISPLAYCONFIG_PATH_INFO structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_PATH_INFO {
    pub sourceInfo: DISPLAYCONFIG_PATH_SOURCE_INFO,
    pub targetInfo: DISPLAYCONFIG_PATH_TARGET_INFO,
    pub flags: u32,
}

// Windows API function declarations
// These functions are not available in windows-rs 0.52, so we declare them manually

#[cfg(windows)]
extern "system" {
    /// Gets the size of the buffers needed for QueryDisplayConfig
    pub fn GetDisplayConfigBufferSizes(
        flags: u32,
        numPathArrayElements: *mut u32,
        numModeInfoArrayElements: *mut u32,
    ) -> i32;

    /// Queries the display configuration
    pub fn QueryDisplayConfig(
        flags: u32,
        numPathArrayElements: *mut u32,
        pathArray: *mut DISPLAYCONFIG_PATH_INFO,
        numModeInfoArrayElements: *mut u32,
        modeInfoArray: *mut DISPLAYCONFIG_MODE_INFO,
        currentTopologyId: *mut u32,
    ) -> i32;

    /// Gets display device information
    pub fn DisplayConfigGetDeviceInfo(requestPacket: *mut DISPLAYCONFIG_DEVICE_INFO_HEADER) -> i32;

    /// Sets display device information
    pub fn DisplayConfigSetDeviceInfo(setPacket: *const DISPLAYCONFIG_DEVICE_INFO_HEADER) -> i32;
}

// Stub implementations for non-Windows platforms
#[cfg(not(windows))]
/// Stub implementation for non-Windows platforms
///
/// # Safety
/// This is a stub function that always returns an error. It does not access any memory.
pub unsafe fn GetDisplayConfigBufferSizes(
    _flags: u32,
    _numPathArrayElements: *mut u32,
    _numModeInfoArrayElements: *mut u32,
) -> i32 {
    -1 // ERROR_NOT_SUPPORTED
}

#[cfg(not(windows))]
/// Stub implementation for non-Windows platforms
///
/// # Safety
/// This is a stub function that always returns an error. It does not access any memory.
pub unsafe fn QueryDisplayConfig(
    _flags: u32,
    _numPathArrayElements: *mut u32,
    _pathArray: *mut DISPLAYCONFIG_PATH_INFO,
    _numModeInfoArrayElements: *mut u32,
    _modeInfoArray: *mut DISPLAYCONFIG_MODE_INFO,
    _currentTopologyId: *mut u32,
) -> i32 {
    -1 // ERROR_NOT_SUPPORTED
}

#[cfg(not(windows))]
/// Stub implementation for non-Windows platforms
///
/// # Safety
/// This is a stub function that always returns an error. It does not access any memory.
pub unsafe fn DisplayConfigGetDeviceInfo(
    _requestPacket: *mut DISPLAYCONFIG_DEVICE_INFO_HEADER,
) -> i32 {
    -1 // ERROR_NOT_SUPPORTED
}

#[cfg(not(windows))]
/// Stub implementation for non-Windows platforms
///
/// # Safety
/// This is a stub function that always returns an error. It does not access any memory.
pub unsafe fn DisplayConfigSetDeviceInfo(
    _setPacket: *const DISPLAYCONFIG_DEVICE_INFO_HEADER,
) -> i32 {
    -1 // ERROR_NOT_SUPPORTED
}

/// Default implementation for DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO
impl Default for DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
    fn default() -> Self {
        Self {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                size: std::mem::size_of::<Self>() as u32,
                adapterId: LUID { LowPart: 0, HighPart: 0 },
                id: 0,
            },
            value: 0,
            colorEncoding: 0,
            bitsPerColorChannel: 0,
        }
    }
}

/// Default implementation for DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2
impl Default for DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
    fn default() -> Self {
        Self {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2,
                size: std::mem::size_of::<Self>() as u32,
                adapterId: LUID { LowPart: 0, HighPart: 0 },
                id: 0,
            },
            colorEncoding: 0,
            bitsPerColorChannel: 0,
            activeColorMode: 0,
            value: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_displayconfig_get_advanced_color_info_bit_fields() {
        let mut info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();

        // Test advancedColorSupported bit
        info.value = 0x1;
        assert!(info.advancedColorSupported());
        assert!(!info.advancedColorEnabled());
        assert!(!info.wideColorEnforced());

        // Test advancedColorEnabled bit
        info.value = 0x2;
        assert!(!info.advancedColorSupported());
        assert!(info.advancedColorEnabled());
        assert!(!info.wideColorEnforced());

        // Test wideColorEnforced bit
        info.value = 0x4;
        assert!(!info.advancedColorSupported());
        assert!(!info.advancedColorEnabled());
        assert!(info.wideColorEnforced());

        // Test multiple bits
        info.value = 0x3; // supported + enabled
        assert!(info.advancedColorSupported());
        assert!(info.advancedColorEnabled());
        assert!(!info.wideColorEnforced());
    }

    #[test]
    fn test_displayconfig_get_advanced_color_info_2_bit_fields() {
        let mut info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2::default();

        // Test highDynamicRangeSupported bit
        info.value = 0x1;
        assert!(info.highDynamicRangeSupported());
        assert!(!info.wideColorGamutSupported());

        // Test wideColorGamutSupported bit
        info.value = 0x2;
        assert!(!info.highDynamicRangeSupported());
        assert!(info.wideColorGamutSupported());

        // Test both bits
        info.value = 0x3;
        assert!(info.highDynamicRangeSupported());
        assert!(info.wideColorGamutSupported());
    }

    #[test]
    fn test_displayconfig_set_advanced_color_state_new() {
        let luid = LUID {
            LowPart: 0x1234,
            HighPart: 0x5678,
        };
        let target_id = 42;

        // Test enable
        let state = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE::new(luid, target_id, true);
        assert_eq!(
            state.header.type_,
            DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE
        );
        assert_eq!(
            state.header.size,
            std::mem::size_of::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>() as u32
        );
        assert_eq!(state.header.adapterId.LowPart, 0x1234);
        assert_eq!(state.header.adapterId.HighPart, 0x5678);
        assert_eq!(state.header.id, 42);
        assert_eq!(state.value, 1);

        // Test disable
        let state = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE::new(luid, target_id, false);
        assert_eq!(state.value, 0);
    }

    #[test]
    fn test_displayconfig_set_hdr_state_new() {
        let luid = LUID {
            LowPart: 0xABCD,
            HighPart: 0xEF01,
        };
        let target_id = 99;

        // Test enable
        let state = DISPLAYCONFIG_SET_HDR_STATE::new(luid, target_id, true);
        assert_eq!(
            state.header.type_,
            DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_SET_HDR_STATE
        );
        assert_eq!(
            state.header.size,
            std::mem::size_of::<DISPLAYCONFIG_SET_HDR_STATE>() as u32
        );
        assert_eq!(state.header.adapterId.LowPart, 0xABCD);
        assert_eq!(state.header.adapterId.HighPart, 0xEF01);
        assert_eq!(state.header.id, 99);
        assert_eq!(state.value, 1);

        // Test disable
        let state = DISPLAYCONFIG_SET_HDR_STATE::new(luid, target_id, false);
        assert_eq!(state.value, 0);
    }

    #[test]
    fn test_structure_sizes() {
        // Verify structure sizes are reasonable (should be multiples of 4 for alignment)
        assert!(std::mem::size_of::<DISPLAYCONFIG_DEVICE_INFO_HEADER>() % 4 == 0);
        assert!(std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() % 4 == 0);
        assert!(std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>() % 4 == 0);
        assert!(std::mem::size_of::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>() % 4 == 0);
        assert!(std::mem::size_of::<DISPLAYCONFIG_SET_HDR_STATE>() % 4 == 0);
    }
}
