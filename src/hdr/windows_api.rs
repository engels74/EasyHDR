//! Windows API structures and definitions for HDR control
//!
//! This module contains Windows API structure definitions and constants
//! needed for HDR control operations.
//!
//! Many of these structures are not available in windows-rs 0.52, so they are
//! manually defined here with #[repr(C)] to match the Windows API layout.

#![expect(
    non_snake_case,
    reason = "Windows API structures use PascalCase and camelCase naming conventions"
)]
#![expect(
    non_camel_case_types,
    reason = "Windows API types use SCREAMING_SNAKE_CASE naming conventions"
)]

// Import LUID from windows-rs on Windows for internal use
#[cfg(windows)]
use windows::Win32::Foundation::LUID as WinLUID;

/// Locally Unique Identifier (LUID) structure
///
/// This is a newtype wrapper around the Windows `LUID` type that implements
/// `Eq` and `Hash` to enable use in hash-based collections like `HashSet` and `HashMap`.
///
/// The `windows-rs` `LUID` type only implements `PartialEq`, but we need `Eq` and `Hash`
/// for `DisplayTarget` to be used in hash-based collections. Rust's orphan rules prevent
/// implementing foreign traits on foreign types, so we use the newtype pattern.
///
/// On Windows, this wraps `windows::Win32::Foundation::LUID`.
/// On non-Windows platforms, this is a standalone struct for testing.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LUID {
    /// Low-order 32 bits
    pub LowPart: u32,
    /// High-order 32 bits
    pub HighPart: i32,
}

// Conversion from windows-rs LUID to our LUID (Windows only)
#[cfg(windows)]
impl From<WinLUID> for LUID {
    fn from(luid: WinLUID) -> Self {
        Self {
            LowPart: luid.LowPart,
            HighPart: luid.HighPart,
        }
    }
}

// Conversion from our LUID to windows-rs LUID (Windows only)
#[cfg(windows)]
impl From<LUID> for WinLUID {
    fn from(luid: LUID) -> Self {
        Self {
            LowPart: luid.LowPart,
            HighPart: luid.HighPart,
        }
    }
}

/// `DISPLAYCONFIG_DEVICE_INFO_TYPE` enumeration values
///
/// Specifies the type of display device info to configure or obtain through
/// `DisplayConfigSetDeviceInfo` or `DisplayConfigGetDeviceInfo`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DISPLAYCONFIG_DEVICE_INFO_TYPE {
    /// Get the source name
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME = 1,
    /// Get the target name
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME = 2,
    /// Get the target preferred mode
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_PREFERRED_MODE = 3,
    /// Get the adapter name
    DISPLAYCONFIG_DEVICE_INFO_GET_ADAPTER_NAME = 4,
    /// Set target persistence
    DISPLAYCONFIG_DEVICE_INFO_SET_TARGET_PERSISTENCE = 5,
    /// Get the target base type
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_BASE_TYPE = 6,
    /// Get support for virtual resolution
    DISPLAYCONFIG_DEVICE_INFO_GET_SUPPORT_VIRTUAL_RESOLUTION = 7,
    /// Set support for virtual resolution
    DISPLAYCONFIG_DEVICE_INFO_SET_SUPPORT_VIRTUAL_RESOLUTION = 8,
    /// Get advanced color info (Windows 10/11 legacy)
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO = 9,
    /// Set advanced color state (Windows 10/11 legacy)
    DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE = 10,
    /// Get SDR white level
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL = 11,
    /// Get monitor specialization
    DISPLAYCONFIG_DEVICE_INFO_GET_MONITOR_SPECIALIZATION = 12,
    /// Set monitor specialization
    DISPLAYCONFIG_DEVICE_INFO_SET_MONITOR_SPECIALIZATION = 13,
    /// Reserved for future use
    DISPLAYCONFIG_DEVICE_INFO_SET_RESERVED1 = 14,
    /// Get advanced color info v2 (Windows 11 24H2+)
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2 = 15,
    /// Set HDR state (Windows 11 24H2+)
    DISPLAYCONFIG_DEVICE_INFO_SET_HDR_STATE = 16,
    /// Set wide color gamut state
    DISPLAYCONFIG_DEVICE_INFO_SET_WCG_STATE = 17,
}

/// `DISPLAYCONFIG_DEVICE_INFO_HEADER` structure
///
/// Contains display information about the device. This is the header for all
/// `DisplayConfigGetDeviceInfo` and `DisplayConfigSetDeviceInfo` operations.
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

/// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO` structure (Windows 10/11)
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
    /// Color encoding (`DISPLAYCONFIG_COLOR_ENCODING`)
    pub colorEncoding: u32,
    /// Bits per color channel
    pub bitsPerColorChannel: u32,
}

impl DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
    /// Create a new structure for querying HDR capabilities (legacy)
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Structure size is a compile-time constant (size_of::<Self>()) which is always less than u32::MAX"
    )]
    pub fn new(adapter_id: LUID, target_id: u32) -> Self {
        Self {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                size: std::mem::size_of::<Self>() as u32,
                adapterId: adapter_id,
                id: target_id,
            },
            value: 0,
            colorEncoding: 0,
            bitsPerColorChannel: 0,
        }
    }

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

/// `DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE` structure (Windows 10/11)
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
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Structure size is a compile-time constant (size_of::<Self>()) which is always less than u32::MAX"
    )]
    pub fn new(adapter_id: LUID, target_id: u32, enable: bool) -> Self {
        Self {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
                size: std::mem::size_of::<Self>() as u32,
                adapterId: adapter_id,
                id: target_id,
            },
            value: u32::from(enable),
        }
    }
}

/// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` structure (Windows 11 24H2+)
///
/// Used to query advanced color capabilities for a display target on Windows 11 24H2+.
///
/// # Critical: Field Order Matters!
///
/// This structure MUST match the Windows SDK layout exactly. The field order is:
/// 1. `header` - `DISPLAYCONFIG_DEVICE_INFO_HEADER` (20 bytes, offset 0)
/// 2. `value` - Bit fields for HDR/WCG capabilities (4 bytes, offset 20) ← MUST BE SECOND!
/// 3. `colorEncoding` - Current color encoding (4 bytes, offset 24)
/// 4. `bitsPerColorChannel` - Bits per color channel (4 bytes, offset 28)
/// 5. `activeColorMode` - Active color mode (SDR/WCG/HDR) (4 bytes, offset 32)
///
/// Total size: 36 bytes
///
/// # Bit Field Layout in `value`
///
/// - Bit 0: `advancedColorSupported` - Display supports advanced color
/// - Bit 1: `advancedColorActive` - Advanced color currently active
/// - Bit 2: Reserved
/// - Bit 3: `advancedColorLimitedByPolicy` - Advanced color limited by policy
/// - **Bit 4: `highDynamicRangeSupported` - Display supports HDR** (mask: 0x10)
/// - Bit 5: `highDynamicRangeUserEnabled` - User enabled HDR
/// - **Bit 6: `wideColorSupported` - Display supports wide color gamut** (mask: 0x40)
/// - Bit 7: `wideColorUserEnabled` - User enabled WCG
/// - Bits 8-31: Reserved
///
/// # References
///
/// - Windows SDK 10.0.26100.0 or later
/// - Source: XBMC/Kodi HDR implementation (tested on thousands of systems)
/// - Header: wingdi.h
/// - Verified against: <https://github.com/xbmc/xbmc/pull/26096>
///
/// # Safety
///
/// If the field order is wrong, Windows writes HDR capability data to the wrong
/// memory location (e.g., into `colorEncoding` instead of `value`), causing complete
/// failure of HDR detection.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
    /// Header
    pub header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    /// Anonymous union containing bit fields (CRITICAL: Must be second field!)
    pub value: u32,
    /// Color encoding (`DISPLAYCONFIG_COLOR_ENCODING`)
    pub colorEncoding: u32,
    /// Bits per color channel
    pub bitsPerColorChannel: u32,
    /// Active color mode (`DISPLAYCONFIG_ADVANCED_COLOR_MODE`)
    pub activeColorMode: u32,
}

impl DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
    /// Create a new structure for querying HDR capabilities (Windows 11 24H2+)
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Structure size is a compile-time constant (size_of::<Self>()) which is always less than u32::MAX"
    )]
    pub fn new(adapter_id: LUID, target_id: u32) -> Self {
        Self {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2,
                size: std::mem::size_of::<Self>() as u32,
                adapterId: adapter_id,
                id: target_id,
            },
            value: 0,
            colorEncoding: 0,
            bitsPerColorChannel: 0,
            activeColorMode: 0,
        }
    }

    /// Check if advanced color is supported (bit 0)
    pub fn advancedColorSupported(&self) -> bool {
        (self.value & 0x1) != 0
    }

    /// Check if advanced color is currently active (bit 1)
    pub fn advancedColorActive(&self) -> bool {
        (self.value & 0x2) != 0
    }

    /// Check if advanced color is limited by policy (bit 3)
    pub fn advancedColorLimitedByPolicy(&self) -> bool {
        (self.value & 0x8) != 0
    }

    /// Check if high dynamic range is supported (bit 4)
    pub fn highDynamicRangeSupported(&self) -> bool {
        (self.value & 0x10) != 0 // Bit 4 = 0x10 (0001 0000)
    }

    /// Check if HDR is user-enabled (bit 5)
    pub fn highDynamicRangeUserEnabled(&self) -> bool {
        (self.value & 0x20) != 0
    }

    /// Check if wide color gamut is supported (bit 6)
    pub fn wideColorGamutSupported(&self) -> bool {
        (self.value & 0x40) != 0 // Bit 6 = 0x40 (0100 0000)
    }

    /// Check if wide color is user-enabled (bit 7)
    pub fn wideColorUserEnabled(&self) -> bool {
        (self.value & 0x80) != 0
    }
}

/// `DISPLAYCONFIG_ADVANCED_COLOR_MODE` enumeration
///
/// Specifies the active color mode for a display.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DISPLAYCONFIG_ADVANCED_COLOR_MODE {
    /// Standard Dynamic Range (SDR) mode
    DISPLAYCONFIG_ADVANCED_COLOR_MODE_SDR = 0,
    /// Wide Color Gamut (WCG) mode
    DISPLAYCONFIG_ADVANCED_COLOR_MODE_WCG = 1,
    /// High Dynamic Range (HDR) mode
    DISPLAYCONFIG_ADVANCED_COLOR_MODE_HDR = 2,
}

/// `DISPLAYCONFIG_SET_HDR_STATE` structure (Windows 11 24H2+)
///
/// Used to set HDR state for a display target on Windows 11 24H2+.
/// This is the new structure that replaces `DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE`.
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
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Structure size is a compile-time constant (size_of::<Self>()) which is always less than u32::MAX"
    )]
    pub fn new(adapter_id: LUID, target_id: u32, enable: bool) -> Self {
        Self {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_SET_HDR_STATE,
                size: std::mem::size_of::<Self>() as u32,
                adapterId: adapter_id,
                id: target_id,
            },
            value: u32::from(enable),
        }
    }
}

// DISPLAYCONFIG structures and constants
// These are not available in windows-rs 0.52, so we define them manually

/// `QDC_ONLY_ACTIVE_PATHS` flag for `QueryDisplayConfig`
pub const QDC_ONLY_ACTIVE_PATHS: u32 = 0x0000_0002;

/// `DISPLAYCONFIG_PATH_ACTIVE` flag
pub const DISPLAYCONFIG_PATH_ACTIVE: u32 = 0x0000_0001;

/// `DISPLAYCONFIG_2DREGION` structure
///
/// Represents a 2D region with width and height.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_2DREGION {
    /// Width in pixels
    pub cx: u32,
    /// Height in pixels
    pub cy: u32,
}

/// `DISPLAYCONFIG_RATIONAL` structure
///
/// Represents a rational number as numerator/denominator.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_RATIONAL {
    /// Numerator of the rational number
    pub Numerator: u32,
    /// Denominator of the rational number
    pub Denominator: u32,
}

/// `DISPLAYCONFIG_VIDEO_SIGNAL_INFO` structure
///
/// Contains information about the video signal for a display target.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_VIDEO_SIGNAL_INFO {
    /// Pixel clock rate in Hz
    pub pixelRate: u64,
    /// Horizontal sync frequency
    pub hSyncFreq: DISPLAYCONFIG_RATIONAL,
    /// Vertical sync frequency (refresh rate)
    pub vSyncFreq: DISPLAYCONFIG_RATIONAL,
    /// Active video region size
    pub activeSize: DISPLAYCONFIG_2DREGION,
    /// Total video region size (including blanking)
    pub totalSize: DISPLAYCONFIG_2DREGION,
    /// Video standard (e.g., NTSC, PAL)
    pub videoStandard: u32,
    /// Scan line ordering (progressive, interlaced)
    pub scanLineOrdering: u32,
}

/// `DISPLAYCONFIG_TARGET_MODE` structure
///
/// Contains information about a display target mode.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_TARGET_MODE {
    /// Video signal information for the target
    pub targetVideoSignalInfo: DISPLAYCONFIG_VIDEO_SIGNAL_INFO,
}

/// `DISPLAYCONFIG_SOURCE_MODE` structure
///
/// Contains information about a display source mode.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_SOURCE_MODE {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Pixel format
    pub pixelFormat: u32,
    /// Position of the source on the desktop
    pub position: DISPLAYCONFIG_2DREGION,
}

/// `DISPLAYCONFIG_MODE_INFO_TYPE` enumeration
///
/// Specifies whether the mode information is for a source or target.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DISPLAYCONFIG_MODE_INFO_TYPE {
    /// Source mode information
    DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE = 1,
    /// Target mode information
    DISPLAYCONFIG_MODE_INFO_TYPE_TARGET = 2,
}

/// `DISPLAYCONFIG_MODE_INFO` structure (union)
///
/// Contains mode information for either a source or target.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DISPLAYCONFIG_MODE_INFO {
    /// Type of mode information (source or target)
    pub infoType: DISPLAYCONFIG_MODE_INFO_TYPE,
    /// Source or target identifier
    pub id: u32,
    /// Adapter LUID
    pub adapterId: LUID,
    /// Union containing either source or target mode info
    pub modeInfo: DISPLAYCONFIG_MODE_INFO_UNION,
}

/// Union for `DISPLAYCONFIG_MODE_INFO`
///
/// Contains either target mode or source mode information.
#[repr(C)]
#[derive(Clone, Copy)]
pub union DISPLAYCONFIG_MODE_INFO_UNION {
    /// Target mode information
    pub targetMode: DISPLAYCONFIG_TARGET_MODE,
    /// Source mode information
    pub sourceMode: DISPLAYCONFIG_SOURCE_MODE,
}

impl Default for DISPLAYCONFIG_MODE_INFO {
    fn default() -> Self {
        Self {
            infoType: DISPLAYCONFIG_MODE_INFO_TYPE::DISPLAYCONFIG_MODE_INFO_TYPE_TARGET,
            id: 0,
            adapterId: LUID::default(),
            modeInfo: DISPLAYCONFIG_MODE_INFO_UNION {
                targetMode: DISPLAYCONFIG_TARGET_MODE::default(),
            },
        }
    }
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "DISPLAYCONFIG_MODE_INFO contains a union field that cannot be safely accessed without knowing the infoType discriminant"
)]
impl std::fmt::Debug for DISPLAYCONFIG_MODE_INFO {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DISPLAYCONFIG_MODE_INFO")
            .field("infoType", &self.infoType)
            .field("id", &self.id)
            .field("adapterId", &self.adapterId)
            .finish()
    }
}

/// `DISPLAYCONFIG_PATH_SOURCE_INFO` structure
///
/// Contains information about a display path source.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_PATH_SOURCE_INFO {
    /// Adapter LUID
    pub adapterId: LUID,
    /// Source identifier
    pub id: u32,
    /// Index into the mode information array
    pub modeInfoIdx: u32,
    /// Status flags
    pub statusFlags: u32,
}

/// `DISPLAYCONFIG_PATH_TARGET_INFO` structure
///
/// Contains information about a display path target.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DISPLAYCONFIG_PATH_TARGET_INFO {
    /// Adapter LUID
    pub adapterId: LUID,
    /// Target identifier
    pub id: u32,
    /// Index into the mode information array
    pub modeInfoIdx: u32,
    /// Output technology (e.g., HDMI, `DisplayPort`)
    pub outputTechnology: u32,
    /// Rotation setting
    pub rotation: u32,
    /// Scaling setting
    pub scaling: u32,
    /// Refresh rate
    pub refreshRate: DISPLAYCONFIG_RATIONAL,
    /// Scan line ordering
    pub scanLineOrdering: u32,
    /// Whether the target is available
    pub targetAvailable: u32,
    /// Status flags
    pub statusFlags: u32,
}

impl Default for DISPLAYCONFIG_PATH_TARGET_INFO {
    /// Creates a default instance by zero-initializing the structure.
    ///
    /// # Safety
    ///
    /// `#[repr(C)]` struct with only primitive fields (`LUID`, `u32`). All-zero bit
    /// patterns valid for these types. Zero-init matches API expectations.
    fn default() -> Self {
        #[expect(
            unsafe_code,
            reason = "Zero-initialization of C struct DISPLAYCONFIG_PATH_TARGET_INFO with primitive fields"
        )]
        unsafe {
            std::mem::zeroed()
        }
    }
}

/// `DISPLAYCONFIG_PATH_INFO` structure
///
/// Contains information about a display path (source to target connection).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DISPLAYCONFIG_PATH_INFO {
    /// Source information
    pub sourceInfo: DISPLAYCONFIG_PATH_SOURCE_INFO,
    /// Target information
    pub targetInfo: DISPLAYCONFIG_PATH_TARGET_INFO,
    /// Path flags
    pub flags: u32,
}

// Windows API function declarations
// These functions are not available in windows-rs 0.52, so we declare them manually

// Windows Display Configuration API FFI declarations
//
// # Safety
//
// Raw FFI for Windows Display Config APIs. Callers must ensure: valid aligned pointers
// to initialized memory; buffer capacity ≥ sizes from `GetDisplayConfigBufferSizes`;
// structures initialized with correct size/type fields; synchronized access to shared
// buffers; return codes checked (0 = success) before using output; `currentTopologyId`
// may be null, all other pointers non-null.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows FFI declarations for Display Configuration API functions not in windows-rs 0.52"
)]
unsafe extern "system" {
    /// Gets the size of the buffers needed for `QueryDisplayConfig`
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
// These stubs are necessary for cross-platform compilation and testing.
// They match the Windows API signatures but always return errors.
#[cfg(not(windows))]
#[expect(
    unsafe_code,
    reason = "Stub FFI function for GetDisplayConfigBufferSizes on non-Windows platforms"
)]
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
#[expect(
    unsafe_code,
    reason = "Stub FFI function for QueryDisplayConfig on non-Windows platforms"
)]
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
#[expect(
    unsafe_code,
    reason = "Stub FFI function for DisplayConfigGetDeviceInfo on non-Windows platforms"
)]
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
#[expect(
    unsafe_code,
    reason = "Stub FFI function for DisplayConfigSetDeviceInfo on non-Windows platforms"
)]
/// Stub implementation for non-Windows platforms
///
/// # Safety
/// This is a stub function that always returns an error. It does not access any memory.
pub unsafe fn DisplayConfigSetDeviceInfo(
    _setPacket: *const DISPLAYCONFIG_DEVICE_INFO_HEADER,
) -> i32 {
    -1 // ERROR_NOT_SUPPORTED
}

/// Default implementation for `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO`
impl Default for DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
    fn default() -> Self {
        Self::new(LUID::default(), 0)
    }
}

/// Default implementation for `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2`
impl Default for DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
    fn default() -> Self {
        Self::new(LUID::default(), 0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_displayconfig_get_advanced_color_info_bit_fields() {
        // Test advancedColorSupported bit
        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            value: 0x1,
            ..Default::default()
        };
        assert!(info.advancedColorSupported());
        assert!(!info.advancedColorEnabled());
        assert!(!info.wideColorEnforced());

        // Test advancedColorEnabled bit
        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            value: 0x2,
            ..Default::default()
        };
        assert!(!info.advancedColorSupported());
        assert!(info.advancedColorEnabled());
        assert!(!info.wideColorEnforced());

        // Test wideColorEnforced bit
        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            value: 0x4,
            ..Default::default()
        };
        assert!(!info.advancedColorSupported());
        assert!(!info.advancedColorEnabled());
        assert!(info.wideColorEnforced());

        // Test multiple bits
        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            value: 0x3, // supported + enabled
            ..Default::default()
        };
        assert!(info.advancedColorSupported());
        assert!(info.advancedColorEnabled());
        assert!(!info.wideColorEnforced());
    }

    #[test]
    fn test_displayconfig_get_advanced_color_info_2_bit_fields() {
        // Test highDynamicRangeSupported bit (bit 4 = 0x10)
        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
            value: 0x10,
            ..Default::default()
        };
        assert!(info.highDynamicRangeSupported());
        assert!(!info.wideColorGamutSupported());

        // Test wideColorGamutSupported bit (bit 6 = 0x40)
        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
            value: 0x40,
            ..Default::default()
        };
        assert!(!info.highDynamicRangeSupported());
        assert!(info.wideColorGamutSupported());

        // Test both bits (0x10 | 0x40 = 0x50)
        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
            value: 0x50,
            ..Default::default()
        };
        assert!(info.highDynamicRangeSupported());
        assert!(info.wideColorGamutSupported());

        // Test individual bit fields
        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
            value: 0x1, // Bit 0: advancedColorSupported
            ..Default::default()
        };
        assert!(info.advancedColorSupported());
        assert!(!info.highDynamicRangeSupported());

        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
            value: 0x2, // Bit 1: advancedColorActive
            ..Default::default()
        };
        assert!(info.advancedColorActive());
        assert!(!info.highDynamicRangeSupported());

        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
            value: 0x8, // Bit 3: advancedColorLimitedByPolicy
            ..Default::default()
        };
        assert!(info.advancedColorLimitedByPolicy());

        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
            value: 0x20, // Bit 5: highDynamicRangeUserEnabled
            ..Default::default()
        };
        assert!(info.highDynamicRangeUserEnabled());
        assert!(!info.highDynamicRangeSupported());

        let info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
            value: 0x80, // Bit 7: wideColorUserEnabled
            ..Default::default()
        };
        assert!(info.wideColorUserEnabled());
        assert!(!info.wideColorGamutSupported());
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Test uses compile-time constant (size_of::<Self>()) which is always less than u32::MAX"
    )]
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
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Test uses compile-time constant (size_of::<Self>()) which is always less than u32::MAX"
    )]
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
        assert!(std::mem::size_of::<DISPLAYCONFIG_DEVICE_INFO_HEADER>().is_multiple_of(4));
        assert!(std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>().is_multiple_of(4));
        assert!(std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>().is_multiple_of(4));
        assert!(std::mem::size_of::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>().is_multiple_of(4));
        assert!(std::mem::size_of::<DISPLAYCONFIG_SET_HDR_STATE>().is_multiple_of(4));
    }

    #[test]
    fn test_displayconfig_device_info_header_exact_size() {
        // Verify header size matches Windows SDK expectations
        // Size should be: type (4 bytes) + size (4) + adapter (8) + id (4) = 20 bytes
        assert_eq!(
            std::mem::size_of::<DISPLAYCONFIG_DEVICE_INFO_HEADER>(),
            20,
            "DISPLAYCONFIG_DEVICE_INFO_HEADER size must be 20 bytes to match Windows SDK"
        );
    }

    #[test]
    fn test_displayconfig_get_advanced_color_info_2_exact_size() {
        // Verify structure size matches Windows SDK expectations
        // Size should be: header (20 bytes) + value (4) + encoding (4) + bits (4) + mode (4) = 36 bytes
        assert_eq!(
            std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>(),
            36,
            "DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 size must be 36 bytes to match Windows SDK"
        );
    }

    #[test]
    fn test_displayconfig_get_advanced_color_info_exact_size() {
        // Verify legacy structure size matches Windows SDK expectations
        // Size should be: header (20 bytes) + value (4) + encoding (4) + bits (4) = 32 bytes
        assert_eq!(
            std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(),
            32,
            "DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO size must be 32 bytes to match Windows SDK"
        );
    }
}
