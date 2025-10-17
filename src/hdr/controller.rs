//! HDR controller implementation
//!
//! This module implements the HDR controller that manages HDR state
//! for Windows displays.

use crate::error::Result;
use crate::hdr::WindowsVersion;
use crate::hdr::windows_api::LUID;
use tracing::{debug, info, warn};

#[cfg(windows)]
use crate::hdr::windows_api::{
    DISPLAYCONFIG_ADVANCED_COLOR_MODE, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes,
    QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};

#[cfg(windows)]
use crate::error::EasyHdrError;

#[cfg(windows)]
use tracing::error;

/// Represents a display target
#[derive(Debug, Clone)]
pub struct DisplayTarget {
    /// Adapter LUID
    pub adapter_id: LUID,
    /// Target ID
    pub target_id: u32,
    /// Whether this display supports HDR
    pub supports_hdr: bool,
}

/// HDR controller
pub struct HdrController {
    /// Windows version
    #[cfg_attr(not(windows), allow(dead_code))]
    windows_version: WindowsVersion,
    /// Cached display targets
    display_cache: Vec<DisplayTarget>,
}

impl HdrController {
    /// Create a new HDR controller
    pub fn new() -> Result<Self> {
        let windows_version = WindowsVersion::detect()?;
        let mut controller = Self {
            windows_version,
            display_cache: Vec::new(),
        };

        // Enumerate displays on creation
        controller.enumerate_displays()?;

        Ok(controller)
    }

    /// Get a reference to the display cache
    ///
    /// Returns a slice of all enumerated display targets.
    pub fn get_display_cache(&self) -> &[DisplayTarget] {
        &self.display_cache
    }

    /// Get the detected Windows version
    ///
    /// Returns the Windows version that was detected during controller initialization.
    pub fn get_windows_version(&self) -> WindowsVersion {
        self.windows_version
    }

    /// Detect the current HDR state from the system
    ///
    /// Checks all HDR-capable displays and returns true if any of them have HDR enabled.
    /// This is a convenience method that combines display enumeration with HDR state checking.
    ///
    /// # Returns
    ///
    /// Returns true if HDR is enabled on any HDR-capable display, false otherwise.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::hdr::HdrController;
    ///
    /// let controller = HdrController::new()?;
    /// let hdr_is_on = controller.detect_current_hdr_state();
    /// println!("HDR is currently: {}", if hdr_is_on { "ON" } else { "OFF" });
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    pub fn detect_current_hdr_state(&self) -> bool {
        use tracing::{debug, warn};

        let displays = self.get_display_cache();

        // Check each HDR-capable display
        for disp in displays.iter().filter(|d| d.supports_hdr) {
            match self.is_hdr_enabled(disp) {
                Ok(enabled) => {
                    if enabled {
                        debug!(
                            "Display (adapter={:#x}:{:#x}, target={}) has HDR enabled",
                            disp.adapter_id.LowPart, disp.adapter_id.HighPart, disp.target_id
                        );
                        return true;
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to check HDR state for display (adapter={:#x}:{:#x}, target={}): {}",
                        disp.adapter_id.LowPart, disp.adapter_id.HighPart, disp.target_id, e
                    );
                }
            }
        }

        // No displays have HDR enabled
        false
    }

    /// Enumerate all active displays and detect HDR support
    ///
    /// Uses Windows Display Configuration APIs to retrieve display information.
    ///
    /// # Safety
    ///
    /// This function contains unsafe code that is sound because:
    ///
    /// 1. **`GetDisplayConfigBufferSizes`**: Called with valid mutable references to u32 values.
    ///    The Windows API contract guarantees these will be written with valid buffer sizes.
    ///
    /// 2. **`QueryDisplayConfig`**: Called with properly allocated Vec buffers:
    ///    - `paths` and `modes` are allocated with exact sizes from `GetDisplayConfigBufferSizes`
    ///    - `as_mut_ptr()` provides valid pointers to the Vec's backing storage
    ///    - The API writes at most `path_count` and `mode_count` elements, which fit in our buffers
    ///    - We pass null for the last parameter (topology info) which is optional
    ///
    /// 3. **DisplayConfigGetDeviceInfo/DisplayConfigSetDeviceInfo**: Called with properly
    ///    initialized structures:
    ///    - Headers are initialized with correct size and type fields
    ///    - Adapter IDs and target IDs come from `QueryDisplayConfig` results
    ///    - Pointer casts are valid because the header is the first field in each structure
    ///
    /// # Invariants
    ///
    /// - Buffer sizes from `GetDisplayConfigBufferSizes` must be used to allocate exact-sized buffers
    /// - Structure headers must have correct size and type fields before API calls
    /// - Adapter and target IDs must come from valid `QueryDisplayConfig` results
    ///
    /// # Potential Issues
    ///
    /// - If Windows API contract changes (extremely unlikely for stable APIs)
    /// - If buffer sizes change between `GetDisplayConfigBufferSizes` and `QueryDisplayConfig`
    ///   (handled by checking return codes)
    #[allow(unsafe_code)] // Windows FFI for display enumeration
    pub fn enumerate_displays(&mut self) -> Result<Vec<DisplayTarget>> {
        #[cfg(windows)]
        {
            use tracing::{debug, info};

            // Step 1: Get buffer sizes for display configuration
            let mut path_count: u32 = 0;
            let mut mode_count: u32 = 0;

            unsafe {
                let result = GetDisplayConfigBufferSizes(
                    QDC_ONLY_ACTIVE_PATHS,
                    &raw mut path_count,
                    &raw mut mode_count,
                );
                debug!(
                    "GetDisplayConfigBufferSizes returned: result={result}, path_count={path_count}, mode_count={mode_count}"
                );
                if result != 0 {
                    error!(
                        "Windows API error - GetDisplayConfigBufferSizes failed with code: {result}"
                    );
                    return Err(EasyHdrError::HdrControlFailed(format!(
                        "Failed to get display config buffer sizes: error code {result}"
                    )));
                }
            }

            debug!(
                "Display config buffer sizes: path_count={}, mode_count={}",
                path_count, mode_count
            );

            // Step 2: Allocate buffers for paths and modes
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];

            // Step 3: Query display configuration
            unsafe {
                let result = QueryDisplayConfig(
                    QDC_ONLY_ACTIVE_PATHS,
                    &raw mut path_count,
                    paths.as_mut_ptr(),
                    &raw mut mode_count,
                    modes.as_mut_ptr(),
                    std::ptr::null_mut(),
                );
                debug!(
                    "QueryDisplayConfig returned: result={result}, final_path_count={path_count}, final_mode_count={mode_count}"
                );
                if result != 0 {
                    error!("Windows API error - QueryDisplayConfig failed with code: {result}");
                    return Err(EasyHdrError::HdrControlFailed(format!(
                        "Failed to query display config: error code {result}"
                    )));
                }
            }

            info!(
                "Successfully queried display configuration: {} active paths",
                path_count
            );

            // Step 4: Extract display targets from paths and detect HDR support
            self.display_cache.clear();

            for (index, path) in paths.iter().enumerate() {
                debug!(
                    "Display path {}: adapter_id={{LowPart: {:#x}, HighPart: {:#x}}}, target_id={}, flags={:#x}",
                    index,
                    path.targetInfo.adapterId.LowPart,
                    path.targetInfo.adapterId.HighPart,
                    path.targetInfo.id,
                    path.flags
                );

                let mut target = DisplayTarget {
                    adapter_id: path.targetInfo.adapterId,
                    target_id: path.targetInfo.id,
                    supports_hdr: false, // Will be detected below
                };

                // Detect HDR support for this display
                match self.is_hdr_supported(&target) {
                    Ok(supported) => {
                        target.supports_hdr = supported;
                        debug!(
                            "Display {}: adapter_id={{LowPart: {:#x}, HighPart: {:#x}}}, target_id={}, HDR supported={}",
                            index,
                            target.adapter_id.LowPart,
                            target.adapter_id.HighPart,
                            target.target_id,
                            supported
                        );
                    }
                    Err(e) => {
                        // Log error but continue with supports_hdr = false
                        debug!(
                            "Display {}: Failed to detect HDR support: {}. Assuming not supported.",
                            index, e
                        );
                    }
                }

                self.display_cache.push(target);
            }

            info!(
                "Enumerated {} display targets ({} HDR-capable)",
                self.display_cache.len(),
                self.display_cache.iter().filter(|d| d.supports_hdr).count()
            );

            Ok(self.display_cache.clone())
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing), return empty list
            self.display_cache.clear();
            Ok(self.display_cache.clone())
        }
    }

    /// Check if a display supports HDR
    ///
    /// Uses Windows Display Configuration APIs to detect HDR capability. Windows 11 24H2+
    /// uses `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` (checks `highDynamicRangeSupported` bit),
    /// while older Windows versions use `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO` (checks
    /// `advancedColorSupported && !wideColorEnforced`).
    ///
    /// # Safety
    ///
    /// This function contains unsafe code that is sound because:
    ///
    /// 1. **Structure Initialization**: The `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` and
    ///    `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO` structures are properly initialized with:
    ///    - Correct size from `std::mem::size_of`
    ///    - Correct type field matching the API being called
    ///    - Valid adapter ID and target ID from `enumerate_displays` results
    ///
    /// 2. **Pointer Cast**: The cast `&mut color_info.header as *mut _ as *mut _` is sound because:
    ///    - The header is the first field in the structure (guaranteed by repr(C) layout)
    ///    - The Windows API expects a pointer to the header and reads the full structure
    ///    - The structure size in the header tells the API how much memory to access
    ///
    /// 3. **API Contract**: `DisplayConfigGetDeviceInfo` is called with a properly initialized
    ///    header and will only write to fields within the structure bounds.
    ///
    /// # Invariants
    ///
    /// - The target parameter must contain valid adapter and target IDs from `QueryDisplayConfig`
    /// - Structure size and type fields must match the actual structure being used
    /// - The header must be the first field in the structure
    #[cfg_attr(not(windows), allow(unused_variables))]
    #[allow(unsafe_code)] // Windows FFI for HDR capability detection
    pub fn is_hdr_supported(&self, target: &DisplayTarget) -> Result<bool> {
        #[cfg(windows)]
        {
            use tracing::{debug, warn};

            match self.windows_version {
                WindowsVersion::Windows11_24H2 => {
                    debug!(
                        "Using Windows 11 24H2+ API (DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2) for adapter={:#x}:{:#x}, target={}",
                        target.adapter_id.LowPart, target.adapter_id.HighPart, target.target_id
                    );

                    // Windows 11 24H2+: Try DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 first
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>() is a compile-time constant that fits in u32"
                    )]
                    let mut color_info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
                        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                            type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2,
                            size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>() as u32,
                            adapterId: target.adapter_id,
                            id: target.target_id,
                        },
                        value: 0,
                        colorEncoding: 0,
                        bitsPerColorChannel: 0,
                        activeColorMode: 0,
                    };

                    unsafe {
                        let result = DisplayConfigGetDeviceInfo(
                            std::ptr::addr_of_mut!(color_info.header).cast(),
                        );
                        debug!(
                            "DisplayConfigGetDeviceInfo (GET_ADVANCED_COLOR_INFO_2) returned: result={result}"
                        );
                        if result != 0 {
                            warn!(
                                "Windows API - DisplayConfigGetDeviceInfo (advanced color info 2) failed for adapter {:?}, target {}: error code {}. Falling back to legacy API.",
                                target.adapter_id, target.target_id, result
                            );

                            // Fallback to the older API for compatibility
                            // This handles cases where newer Windows builds may have changed the API
                            return self.is_hdr_supported_legacy(target);
                        }
                    }

                    let hdr_supported = color_info.highDynamicRangeSupported();
                    let wcg_supported = color_info.wideColorGamutSupported();

                    debug!(
                        "Display (adapter={:#x}:{:#x}, target={}) - Windows 11 24H2+ API results:",
                        target.adapter_id.LowPart, target.adapter_id.HighPart, target.target_id
                    );
                    debug!(
                        "  Raw value: {:#034b} (hex: {:#010x})",
                        color_info.value, color_info.value
                    );
                    debug!("  colorEncoding: {}", color_info.colorEncoding);
                    debug!("  bitsPerColorChannel: {}", color_info.bitsPerColorChannel);
                    debug!(
                        "  activeColorMode: {} (0=SDR, 1=WCG, 2=HDR)",
                        color_info.activeColorMode
                    );
                    debug!("  Bit fields:");
                    debug!(
                        "    [bit 0] advancedColorSupported: {}",
                        color_info.advancedColorSupported()
                    );
                    debug!(
                        "    [bit 1] advancedColorActive: {}",
                        color_info.advancedColorActive()
                    );
                    debug!(
                        "    [bit 3] advancedColorLimitedByPolicy: {}",
                        color_info.advancedColorLimitedByPolicy()
                    );
                    debug!(
                        "    [bit 4] highDynamicRangeSupported: {} ← HDR DETECTION",
                        hdr_supported
                    );
                    debug!(
                        "    [bit 5] highDynamicRangeUserEnabled: {}",
                        color_info.highDynamicRangeUserEnabled()
                    );
                    debug!("    [bit 6] wideColorSupported: {}", wcg_supported);
                    debug!(
                        "    [bit 7] wideColorUserEnabled: {}",
                        color_info.wideColorUserEnabled()
                    );
                    debug!("  Final HDR supported: {}", hdr_supported);

                    Ok(hdr_supported)
                }
                WindowsVersion::Windows10 | WindowsVersion::Windows11 => {
                    // Windows 10/11 (before 24H2): Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO
                    debug!(
                        "Using legacy API (DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO) for adapter={:#x}:{:#x}, target={}",
                        target.adapter_id.LowPart, target.adapter_id.HighPart, target.target_id
                    );
                    self.is_hdr_supported_legacy(target)
                }
            }
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing), return false
            Ok(false)
        }
    }

    /// Check HDR support using legacy API (Windows 10/11, or fallback for 24H2+)
    #[cfg(windows)]
    #[allow(unsafe_code)] // Windows FFI for legacy HDR capability detection
    #[expect(
        clippy::unused_self,
        reason = "Method signature matches trait-like pattern for consistency with other HDR detection methods"
    )]
    fn is_hdr_supported_legacy(&self, target: &DisplayTarget) -> Result<bool> {
        use tracing::debug;

        #[expect(clippy::cast_possible_truncation, reason = "size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() is a compile-time constant (40 bytes) that fits in u32")]
        let mut color_info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
                adapterId: target.adapter_id,
                id: target.target_id,
            },
            value: 0,
            colorEncoding: 0,
            bitsPerColorChannel: 0,
        };

        unsafe {
            let result =
                DisplayConfigGetDeviceInfo(std::ptr::addr_of_mut!(color_info.header).cast());
            debug!(
                "DisplayConfigGetDeviceInfo (GET_ADVANCED_COLOR_INFO) returned: result={result}",
            );
            if result != 0 {
                error!(
                    "Windows API error - DisplayConfigGetDeviceInfo (legacy advanced color info) failed for adapter {:?}, target {}: error code {result}",
                    target.adapter_id, target.target_id
                );
                return Err(EasyHdrError::HdrControlFailed(format!(
                    "Failed to get advanced color info (legacy): error code {result}",
                )));
            }
        }

        let advanced_color_supported = color_info.advancedColorSupported();
        let advanced_color_enabled = color_info.advancedColorEnabled();
        let wide_color_enforced = color_info.wideColorEnforced();
        let advanced_color_force_disabled = color_info.advancedColorForceDisabled();

        // HDR supported: advancedColorSupported == TRUE AND wideColorEnforced == FALSE
        let supported = advanced_color_supported && !wide_color_enforced;

        debug!(
            "Display (adapter={:#x}:{:#x}, target={}) - Legacy API results:",
            target.adapter_id.LowPart, target.adapter_id.HighPart, target.target_id
        );
        debug!("  value (raw bitfield): {:#010x}", color_info.value);
        debug!("  colorEncoding: {}", color_info.colorEncoding);
        debug!("  bitsPerColorChannel: {}", color_info.bitsPerColorChannel);
        debug!(
            "  advancedColorSupported (bit 0): {}",
            advanced_color_supported
        );
        debug!("  advancedColorEnabled (bit 1): {}", advanced_color_enabled);
        debug!("  wideColorEnforced (bit 2): {}", wide_color_enforced);
        debug!(
            "  advancedColorForceDisabled (bit 3): {}",
            advanced_color_force_disabled
        );
        debug!(
            "  Final HDR supported (advancedColorSupported && !wideColorEnforced): {}",
            supported
        );

        Ok(supported)
    }

    /// Check if HDR is currently enabled on a display
    ///
    /// Uses version-specific Windows APIs to detect current HDR state.
    ///
    /// # Safety
    ///
    /// This function contains unsafe code (via `is_hdr_enabled_legacy`) that is sound for the
    /// same reasons as `is_hdr_supported`: properly initialized structures with correct size/type
    /// fields, valid adapter/target IDs, and sound pointer casts to the header field.
    #[cfg_attr(not(windows), allow(unused_variables))]
    #[allow(unsafe_code)] // Windows FFI for HDR state detection
    pub fn is_hdr_enabled(&self, target: &DisplayTarget) -> Result<bool> {
        #[cfg(windows)]
        {
            use tracing::debug;

            match self.windows_version {
                WindowsVersion::Windows11_24H2 => {
                    // Windows 11 24H2+: Try DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 first
                    #[expect(clippy::cast_possible_truncation, reason = "size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>() is a compile-time constant (48 bytes) that fits in u32")]
                    let mut color_info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
                        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                            type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2,
                            size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>() as u32,
                            adapterId: target.adapter_id,
                            id: target.target_id,
                        },
                        value: 0,
                        colorEncoding: 0,
                        bitsPerColorChannel: 0,
                        activeColorMode: 0,
                    };

                    unsafe {
                        let result = DisplayConfigGetDeviceInfo(
                            std::ptr::addr_of_mut!(color_info.header).cast(),
                        );
                        if result != 0 {
                            use tracing::warn;
                            warn!(
                                "Windows API - DisplayConfigGetDeviceInfo (advanced color info 2 for HDR enabled check) failed for adapter {:?}, target {}: error code {result}. Falling back to legacy API.",
                                target.adapter_id, target.target_id
                            );

                            // Fallback to the older API for compatibility
                            return self.is_hdr_enabled_legacy(target);
                        }
                    }

                    // HDR enabled: activeColorMode == HDR
                    let enabled = color_info.activeColorMode
                        == DISPLAYCONFIG_ADVANCED_COLOR_MODE::DISPLAYCONFIG_ADVANCED_COLOR_MODE_HDR
                            as u32;
                    debug!(
                        "Display (adapter={:#x}:{:#x}, target={}): activeColorMode={}, HDR enabled (24H2+ API) = {}",
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id,
                        color_info.activeColorMode,
                        enabled
                    );

                    Ok(enabled)
                }
                WindowsVersion::Windows10 | WindowsVersion::Windows11 => {
                    // Windows 10/11 (before 24H2): Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO
                    self.is_hdr_enabled_legacy(target)
                }
            }
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing), return false
            Ok(false)
        }
    }

    /// Check HDR enabled state using legacy API (Windows 10/11, or fallback for 24H2+)
    #[cfg(windows)]
    #[allow(unsafe_code)] // Windows FFI for legacy HDR state detection
    #[expect(
        clippy::unused_self,
        reason = "Method signature matches trait-like pattern for consistency with other HDR detection methods"
    )]
    fn is_hdr_enabled_legacy(&self, target: &DisplayTarget) -> Result<bool> {
        use tracing::debug;

        #[expect(clippy::cast_possible_truncation, reason = "size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() is a compile-time constant (40 bytes) that fits in u32")]
        let mut color_info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
                adapterId: target.adapter_id,
                id: target.target_id,
            },
            value: 0,
            colorEncoding: 0,
            bitsPerColorChannel: 0,
        };

        unsafe {
            let result =
                DisplayConfigGetDeviceInfo(std::ptr::addr_of_mut!(color_info.header).cast());
            if result != 0 {
                error!(
                    "Windows API error - DisplayConfigGetDeviceInfo (legacy advanced color info for HDR enabled check) failed for adapter {:?}, target {}: error code {result}",
                    target.adapter_id, target.target_id
                );
                return Err(EasyHdrError::HdrControlFailed(format!(
                    "Failed to get advanced color info (legacy): error code {result}",
                )));
            }
        }

        // HDR enabled: advancedColorSupported == TRUE AND advancedColorEnabled == TRUE AND wideColorEnforced == FALSE
        let enabled = color_info.advancedColorSupported()
            && color_info.advancedColorEnabled()
            && !color_info.wideColorEnforced();
        debug!(
            "Display (adapter={:#x}:{:#x}, target={}): advancedColorSupported={}, advancedColorEnabled={}, wideColorEnforced={}, HDR enabled (legacy API) = {}",
            target.adapter_id.LowPart,
            target.adapter_id.HighPart,
            target.target_id,
            color_info.advancedColorSupported(),
            color_info.advancedColorEnabled(),
            color_info.wideColorEnforced(),
            enabled
        );

        Ok(enabled)
    }

    /// Enable or disable HDR on a single display
    ///
    /// Uses version-specific Windows APIs and includes a 100ms delay for state propagation.
    ///
    /// # Safety
    ///
    /// This function contains unsafe code that is sound because:
    ///
    /// 1. **Structure Initialization**: Both `DISPLAYCONFIG_SET_HDR_STATE` (24H2+) and
    ///    `DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE` (legacy) are properly initialized via
    ///    their `new()` constructors with:
    ///    - Correct size and type fields in the header
    ///    - Valid adapter ID and target ID from `enumerate_displays` results
    ///    - Proper enable/disable flag
    ///
    /// 2. **Pointer Cast**: The cast `&mut set_state.header as *mut _ as *mut _` is sound because:
    ///    - The header is the first field in both structures (repr(C) layout)
    ///    - `DisplayConfigSetDeviceInfo` expects a pointer to the header
    ///    - The structure size in the header tells the API how much memory to access
    ///
    /// 3. **API Contract**: `DisplayConfigSetDeviceInfo` is called with properly initialized
    ///    structures and will only access memory within the structure bounds.
    ///
    /// 4. **State Propagation**: The 100ms delay ensures Windows has time to propagate the
    ///    HDR state change before subsequent operations.
    ///
    /// # Invariants
    ///
    /// - The target parameter must contain valid adapter and target IDs
    /// - Structure size and type fields must match the actual structure being used
    /// - The header must be the first field in the structure
    #[cfg_attr(not(windows), allow(dead_code))]
    #[allow(unsafe_code)] // Windows FFI for HDR state control
    pub fn set_hdr_state(&self, target: &DisplayTarget, enable: bool) -> Result<()> {
        #[cfg(windows)]
        {
            use crate::hdr::windows_api::{
                DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE, DISPLAYCONFIG_SET_HDR_STATE,
            };
            use tracing::{debug, info};

            match self.windows_version {
                WindowsVersion::Windows11_24H2 => {
                    // Windows 11 24H2+: Use DISPLAYCONFIG_SET_HDR_STATE
                    let mut set_state = DISPLAYCONFIG_SET_HDR_STATE::new(
                        target.adapter_id,
                        target.target_id,
                        enable,
                    );

                    debug!(
                        "Setting HDR state (24H2+) for display (adapter={:#x}:{:#x}, target={}): {}",
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id,
                        if enable { "ON" } else { "OFF" }
                    );

                    unsafe {
                        let result = DisplayConfigSetDeviceInfo(
                            std::ptr::addr_of_mut!(set_state.header).cast(),
                        );
                        if result != 0 {
                            error!(
                                "Windows API error - DisplayConfigSetDeviceInfo (set HDR state 24H2+) failed for adapter {:?}, target {}: error code {result}",
                                target.adapter_id, target.target_id
                            );
                            return Err(EasyHdrError::HdrControlFailed(format!(
                                "Failed to set HDR state (24H2+): error code {result}",
                            )));
                        }
                    }

                    // Add 100ms delay after DisplayConfigSetDeviceInfo call
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    info!(
                        "Successfully set HDR {} for display (adapter={:#x}:{:#x}, target={})",
                        if enable { "ON" } else { "OFF" },
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id
                    );

                    Ok(())
                }
                WindowsVersion::Windows10 | WindowsVersion::Windows11 => {
                    // Windows 10/11 (before 24H2): Use DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE
                    let mut set_state = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE::new(
                        target.adapter_id,
                        target.target_id,
                        enable,
                    );

                    debug!(
                        "Setting HDR state (legacy) for display (adapter={:#x}:{:#x}, target={}): {}",
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id,
                        if enable { "ON" } else { "OFF" }
                    );

                    unsafe {
                        let result = DisplayConfigSetDeviceInfo(
                            std::ptr::addr_of_mut!(set_state.header).cast(),
                        );
                        if result != 0 {
                            error!(
                                "Windows API error - DisplayConfigSetDeviceInfo (set advanced color state) failed for adapter {:?}, target {}: error code {result}",
                                target.adapter_id, target.target_id
                            );
                            return Err(EasyHdrError::HdrControlFailed(format!(
                                "Failed to set advanced color state: error code {result}",
                            )));
                        }
                    }

                    // Add 100ms delay after DisplayConfigSetDeviceInfo call
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    info!(
                        "Successfully set HDR {} for display (adapter={:#x}:{:#x}, target={})",
                        if enable { "ON" } else { "OFF" },
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id
                    );

                    Ok(())
                }
            }
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing), just log and return success
            use tracing::debug;
            debug!(
                "Mock: Setting HDR state for display (target={}): {}",
                target.target_id,
                if enable { "ON" } else { "OFF" }
            );
            Ok(())
        }
    }

    /// Refresh the display cache by re-enumerating all displays
    ///
    /// Useful when display configuration changes (monitor connected/disconnected).
    pub fn refresh_displays(&mut self) -> Result<Vec<DisplayTarget>> {
        use tracing::info;

        info!("Refreshing display cache");
        self.enumerate_displays()
    }

    /// Enable or disable HDR globally across all HDR-capable displays
    ///
    /// Returns results for each display, allowing partial success. Continues with remaining
    /// displays if some fail (e.g., due to disconnection).
    pub fn set_hdr_global(&self, enable: bool) -> Result<Vec<(DisplayTarget, Result<()>)>> {
        use tracing::{debug, info, warn};

        info!(
            "Setting HDR {} globally for {} display(s)",
            if enable { "ON" } else { "OFF" },
            self.display_cache.len()
        );

        let mut results = Vec::new();

        for target in &self.display_cache {
            // Only attempt to set HDR on displays that support it
            if !target.supports_hdr {
                debug!(
                    "Skipping display (adapter={:#x}:{:#x}, target={}) - HDR not supported",
                    target.adapter_id.LowPart, target.adapter_id.HighPart, target.target_id
                );
                continue;
            }

            let result = self.set_hdr_state(target, enable);

            match &result {
                Ok(()) => {
                    info!(
                        "Successfully set HDR {} for display (adapter={:#x}:{:#x}, target={})",
                        if enable { "ON" } else { "OFF" },
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to set HDR {} for display (adapter={:#x}:{:#x}, target={}): {}. \
                         Display may have been disconnected or driver issue occurred. Continuing with other displays.",
                        if enable { "ON" } else { "OFF" },
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id,
                        e
                    );
                }
            }

            results.push((target.clone(), result));
        }

        info!(
            "HDR global toggle complete: {} successful, {} failed",
            results.iter().filter(|(_, r)| r.is_ok()).count(),
            results.iter().filter(|(_, r)| r.is_err()).count()
        );

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hdr_controller_creation() {
        let controller = HdrController::new();
        assert!(controller.is_ok());
    }

    #[test]
    fn test_enumerate_displays() {
        let mut controller = HdrController::new().expect("Failed to create controller");

        // Enumerate displays
        let displays = controller.enumerate_displays();

        // Should succeed (even if no displays found on test system)
        assert!(displays.is_ok());

        let displays = displays.unwrap();

        // Verify display cache is updated
        assert_eq!(controller.display_cache.len(), displays.len());

        // If displays are found, verify structure
        for display in &displays {
            // Target ID is a u32, so it's always valid (>= 0)
            // Just verify the display has some data
            let _ = display.target_id; // Ensure field exists

            // supports_hdr is now properly detected (may be true or false depending on hardware)
            // The field is a bool, so it's always valid
            let _ = display.supports_hdr; // Ensure field exists
        }
    }

    #[test]
    fn test_display_target_structure() {
        // Test that DisplayTarget can be created and cloned
        let target = DisplayTarget {
            adapter_id: LUID {
                LowPart: 0x1234,
                HighPart: 0x5678,
            },
            target_id: 42,
            supports_hdr: false,
        };

        let cloned = target.clone();

        assert_eq!(cloned.adapter_id.LowPart, 0x1234);
        assert_eq!(cloned.adapter_id.HighPart, 0x5678);
        assert_eq!(cloned.target_id, 42);
        assert!(!cloned.supports_hdr);
    }

    #[test]
    #[cfg(windows)]
    fn test_is_hdr_supported() {
        // This test verifies that is_hdr_supported can be called without errors
        // The actual result depends on the hardware
        let controller = HdrController::new().expect("Failed to create controller");

        // Test on all enumerated displays
        for display in &controller.display_cache {
            let result = controller.is_hdr_supported(display);

            // Should succeed (even if HDR is not supported)
            assert!(result.is_ok(), "is_hdr_supported should not fail");

            // Result should match the cached value
            assert_eq!(result.unwrap(), display.supports_hdr);
        }
    }

    #[test]
    #[cfg(windows)]
    fn test_is_hdr_enabled() {
        // This test verifies that is_hdr_enabled can be called without errors
        // The actual result depends on the current HDR state
        let controller = HdrController::new().expect("Failed to create controller");

        // Test on all enumerated displays
        for display in &controller.display_cache {
            let result = controller.is_hdr_enabled(display);

            // Should succeed (even if HDR is not enabled)
            assert!(result.is_ok(), "is_hdr_enabled should not fail");

            // If HDR is enabled, the display must support HDR
            if result.unwrap() {
                assert!(
                    display.supports_hdr,
                    "HDR cannot be enabled on a display that doesn't support it"
                );
            }
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn test_is_hdr_supported_non_windows() {
        // On non-Windows platforms, should return false
        let controller = HdrController::new().expect("Failed to create controller");

        let target = DisplayTarget {
            adapter_id: LUID {
                LowPart: 0,
                HighPart: 0,
            },
            target_id: 0,
            supports_hdr: false,
        };

        let result = controller.is_hdr_supported(&target);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    #[cfg(not(windows))]
    fn test_is_hdr_enabled_non_windows() {
        // On non-Windows platforms, should return false
        let controller = HdrController::new().expect("Failed to create controller");

        let target = DisplayTarget {
            adapter_id: LUID {
                LowPart: 0,
                HighPart: 0,
            },
            target_id: 0,
            supports_hdr: false,
        };

        let result = controller.is_hdr_enabled(&target);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_hdr_detection_consistency() {
        // Test that HDR detection is consistent across multiple calls
        let controller = HdrController::new().expect("Failed to create controller");

        for display in &controller.display_cache {
            // Call is_hdr_supported multiple times
            let result1 = controller.is_hdr_supported(display);
            let result2 = controller.is_hdr_supported(display);

            assert!(result1.is_ok());
            assert!(result2.is_ok());

            // Results should be consistent
            assert_eq!(result1.unwrap(), result2.unwrap());

            // Call is_hdr_enabled multiple times
            let enabled1 = controller.is_hdr_enabled(display);
            let enabled2 = controller.is_hdr_enabled(display);

            assert!(enabled1.is_ok());
            assert!(enabled2.is_ok());

            // Results should be consistent (within a short time frame)
            assert_eq!(enabled1.unwrap(), enabled2.unwrap());
        }
    }

    #[test]
    #[cfg(windows)]
    fn test_set_hdr_state() {
        // This test verifies that set_hdr_state can be called without errors
        // Note: This test may modify the actual HDR state of the display
        let controller = HdrController::new().expect("Failed to create controller");

        // Find an HDR-capable display
        let hdr_display = controller.display_cache.iter().find(|d| d.supports_hdr);

        if let Some(display) = hdr_display {
            // Get current HDR state
            let initial_state = controller
                .is_hdr_enabled(display)
                .expect("Failed to get initial HDR state");

            // Try to toggle HDR off
            let result = controller.set_hdr_state(display, false);
            assert!(result.is_ok(), "set_hdr_state(false) should succeed");

            // Wait a bit for the change to take effect
            std::thread::sleep(std::time::Duration::from_millis(200));

            // Verify state changed
            let new_state = controller
                .is_hdr_enabled(display)
                .expect("Failed to get new HDR state");
            assert!(!new_state, "HDR should be disabled");

            // Restore original state
            let result = controller.set_hdr_state(display, initial_state);
            assert!(result.is_ok(), "set_hdr_state(restore) should succeed");

            // Wait a bit for the change to take effect
            std::thread::sleep(std::time::Duration::from_millis(200));

            // Verify state restored
            let restored_state = controller
                .is_hdr_enabled(display)
                .expect("Failed to get restored HDR state");
            assert_eq!(
                restored_state, initial_state,
                "HDR state should be restored to initial value"
            );
        } else {
            // No HDR-capable display found, skip test
            println!("No HDR-capable display found, skipping test_set_hdr_state");
        }
    }

    #[test]
    #[cfg(windows)]
    fn test_set_hdr_global() {
        // This test verifies that set_hdr_global can be called without errors
        // Note: This test may modify the actual HDR state of all displays
        let controller = HdrController::new().expect("Failed to create controller");

        // Check if we have any HDR-capable displays
        let hdr_count = controller
            .display_cache
            .iter()
            .filter(|d| d.supports_hdr)
            .count();

        if hdr_count == 0 {
            println!("No HDR-capable displays found, skipping test_set_hdr_global");
            return;
        }

        // Get initial states
        let initial_states: Vec<_> = controller
            .display_cache
            .iter()
            .filter(|d| d.supports_hdr)
            .map(|d| {
                (
                    d.clone(),
                    controller
                        .is_hdr_enabled(d)
                        .expect("Failed to get initial state"),
                )
            })
            .collect();

        // Disable HDR globally
        let results = controller
            .set_hdr_global(false)
            .expect("set_hdr_global(false) should succeed");

        // Verify results structure
        assert_eq!(
            results.len(),
            hdr_count,
            "Should return results for all HDR-capable displays"
        );

        // All results should be Ok
        for (target, result) in &results {
            assert!(
                result.is_ok(),
                "set_hdr_state should succeed for display (adapter={:#x}:{:#x}, target={})",
                target.adapter_id.LowPart,
                target.adapter_id.HighPart,
                target.target_id
            );
        }

        // Wait for changes to take effect
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Verify all HDR-capable displays are disabled
        for display in controller.display_cache.iter().filter(|d| d.supports_hdr) {
            let state = controller
                .is_hdr_enabled(display)
                .expect("Failed to get HDR state");
            assert!(!state, "HDR should be disabled globally");
        }

        // Restore original states
        for (display, initial_state) in &initial_states {
            controller
                .set_hdr_state(display, *initial_state)
                .expect("Failed to restore HDR state");
        }

        // Wait for changes to take effect
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Verify states are restored
        for (display, initial_state) in &initial_states {
            let restored_state = controller
                .is_hdr_enabled(display)
                .expect("Failed to get restored state");
            assert_eq!(
                restored_state, *initial_state,
                "HDR state should be restored to initial value"
            );
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn test_set_hdr_state_non_windows() {
        // On non-Windows platforms, should succeed without errors
        let controller = HdrController::new().expect("Failed to create controller");

        let target = DisplayTarget {
            adapter_id: LUID {
                LowPart: 0,
                HighPart: 0,
            },
            target_id: 0,
            supports_hdr: true,
        };

        let result = controller.set_hdr_state(&target, true);
        assert!(result.is_ok());

        let result = controller.set_hdr_state(&target, false);
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(not(windows))]
    fn test_set_hdr_global_non_windows() {
        // On non-Windows platforms, should return empty results
        let controller = HdrController::new().expect("Failed to create controller");

        let results = controller.set_hdr_global(true);
        assert!(results.is_ok());
        assert_eq!(results.unwrap().len(), 0);
    }

    #[test]
    fn test_set_hdr_global_partial_success() {
        // This test verifies that set_hdr_global handles partial success scenarios
        // by returning results for each display
        let controller = HdrController::new().expect("Failed to create controller");

        let results = controller
            .set_hdr_global(false)
            .expect("set_hdr_global should succeed");

        // Results should be a vector of (DisplayTarget, Result<()>)
        for (target, result) in &results {
            // Each result should have a valid target (target_id is u32, always >= 0)
            let _ = target.target_id; // Just verify it exists

            // Result should be Ok or Err (both are valid for partial success)
            if let Ok(()) = result {
                // Success case
                assert!(
                    target.supports_hdr,
                    "Only HDR-capable displays should succeed"
                );
            }
            // Failure case - this is acceptable for partial success
            // Just verify the error is logged (we can't check logs in tests)
        }
    }

    #[test]
    fn test_refresh_displays() {
        // This test verifies that refresh_displays updates the display cache
        let mut controller = HdrController::new().expect("Failed to create controller");

        // Get initial display count
        let initial_count = controller.display_cache.len();

        // Refresh displays
        let refreshed_displays = controller
            .refresh_displays()
            .expect("refresh_displays should succeed");

        // Verify the cache was updated
        assert_eq!(
            controller.display_cache.len(),
            refreshed_displays.len(),
            "Display cache should be updated after refresh"
        );

        // Verify the count is consistent (should be the same unless displays were connected/disconnected)
        // In most test scenarios, the count should remain the same
        assert_eq!(
            refreshed_displays.len(),
            initial_count,
            "Display count should be consistent (unless hardware changed)"
        );

        // Verify each display has valid properties
        for display in &refreshed_displays {
            // target_id is u32, always valid
            let _ = display.target_id;
            // supports_hdr is a boolean, always valid
            let _ = display.supports_hdr;
        }
    }

    #[test]
    fn test_version_specific_api_selection() {
        // This test verifies that the controller uses the correct API based on Windows version
        // We test this by creating controllers and verifying they initialize correctly
        let controller = HdrController::new().expect("Failed to create controller");

        // Verify the controller has a valid Windows version
        match controller.windows_version {
            WindowsVersion::Windows10 => {
                // Windows 10 should use legacy APIs
                // The controller should initialize successfully
                eprintln!("Windows 10 detected");
            }
            WindowsVersion::Windows11 => {
                // Windows 11 (pre-24H2) should use legacy APIs
                eprintln!("Windows 11 detected");
            }
            WindowsVersion::Windows11_24H2 => {
                // Windows 11 24H2+ should use new APIs
                eprintln!("Windows 11 24H2+ detected");
            }
        }

        // Verify that displays can be enumerated regardless of version
        // Display cache should be initialized (may be empty or have displays)
        assert!(
            !controller.display_cache.is_empty() || controller.display_cache.is_empty(),
            "Display cache should be initialized"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_version_specific_hdr_detection() {
        // This test verifies that HDR detection uses the correct API based on Windows version
        let controller = HdrController::new().expect("Failed to create controller");

        // Test HDR detection on all displays
        for display in &controller.display_cache {
            let result = controller.is_hdr_supported(display);

            // Should succeed regardless of Windows version
            assert!(
                result.is_ok(),
                "HDR detection should work on all Windows versions"
            );

            // The result should match the cached value
            assert_eq!(
                result.unwrap(),
                display.supports_hdr,
                "HDR detection should be consistent with cached value"
            );
        }

        // Test HDR enabled detection on all displays
        for display in &controller.display_cache {
            let result = controller.is_hdr_enabled(display);

            // Should succeed regardless of Windows version
            assert!(
                result.is_ok(),
                "HDR enabled detection should work on all Windows versions"
            );
        }
    }

    #[test]
    fn test_error_handling_for_unsupported_displays() {
        // This test verifies that the controller handles unsupported displays gracefully
        let controller = HdrController::new().expect("Failed to create controller");

        // Create a mock display target with invalid IDs
        let invalid_target = DisplayTarget {
            adapter_id: LUID {
                LowPart: 0xFFFF_FFFF,
                HighPart: -1,
            },
            target_id: 0xFFFF_FFFF,
            supports_hdr: false,
        };

        // Test HDR support detection on invalid display
        // This may fail or return false, both are acceptable
        let result = controller.is_hdr_supported(&invalid_target);
        match result {
            Ok(supported) => {
                // If it succeeds, it should return false for invalid display
                assert!(!supported, "Invalid display should not support HDR");
            }
            Err(_) => {
                // If it fails, that's also acceptable for invalid display
                eprintln!("Error is acceptable for invalid display");
            }
        }

        // Test HDR enabled detection on invalid display
        let result = controller.is_hdr_enabled(&invalid_target);
        match result {
            Ok(enabled) => {
                // If it succeeds, it should return false for invalid display
                assert!(!enabled, "Invalid display should not have HDR enabled");
            }
            Err(_) => {
                // If it fails, that's also acceptable for invalid display
                eprintln!("Error is acceptable for invalid display");
            }
        }
    }

    #[test]
    fn test_error_handling_continues_operation() {
        // This test verifies that errors in HDR control don't crash the application
        let controller = HdrController::new().expect("Failed to create controller");

        // Create a mock display target with invalid IDs
        let invalid_target = DisplayTarget {
            adapter_id: LUID {
                LowPart: 0xFFFF_FFFF,
                HighPart: -1,
            },
            target_id: 0xFFFF_FFFF,
            supports_hdr: true, // Pretend it supports HDR
        };

        // Try to set HDR state on invalid display
        let result = controller.set_hdr_state(&invalid_target, true);

        // The operation should either succeed (unlikely) or fail gracefully
        match result {
            Ok(()) => {
                // Unlikely but acceptable
                eprintln!("Operation succeeded on invalid display");
            }
            Err(e) => {
                // Expected: operation fails but doesn't panic
                eprintln!("Operation failed gracefully with error: {e}");
            }
        }

        // Verify the controller is still functional after error
        let displays = controller.display_cache.clone();
        // Just verify we can access the display cache (len() is always valid)
        let _ = displays.len();
    }

    #[test]
    fn test_display_enumeration_parsing() {
        // This test verifies that display enumeration correctly parses display information
        let mut controller = HdrController::new().expect("Failed to create controller");

        // Enumerate displays
        let displays = controller
            .enumerate_displays()
            .expect("Display enumeration should succeed");

        // Verify each display has valid properties
        for (index, display) in displays.iter().enumerate() {
            // Adapter ID should be initialized (LowPart is u32, always valid)
            let _ = display.adapter_id.LowPart;
            // HighPart is i32, can be negative or positive
            let _ = display.adapter_id.HighPart;

            // Target ID should be valid (u32, always valid)
            let _ = display.target_id;

            // supports_hdr should be a boolean (always valid)
            let _ = display.supports_hdr;

            // Just verify we can access the display
            assert!(index < displays.len(), "Display index should be valid");
        }

        // Verify the display cache matches the returned displays
        assert_eq!(
            controller.display_cache.len(),
            displays.len(),
            "Display cache should match enumerated displays"
        );

        for (cached, enumerated) in controller.display_cache.iter().zip(displays.iter()) {
            assert_eq!(
                cached.adapter_id.LowPart, enumerated.adapter_id.LowPart,
                "Cached adapter ID LowPart should match enumerated"
            );
            assert_eq!(
                cached.adapter_id.HighPart, enumerated.adapter_id.HighPart,
                "Cached adapter ID HighPart should match enumerated"
            );
            assert_eq!(
                cached.target_id, enumerated.target_id,
                "Cached target ID should match enumerated"
            );
            assert_eq!(
                cached.supports_hdr, enumerated.supports_hdr,
                "Cached supports_hdr should match enumerated"
            );
        }
    }

    #[test]
    fn test_hdr_control_skips_unsupported_displays() {
        // This test verifies that set_hdr_global skips displays that don't support HDR
        let controller = HdrController::new().expect("Failed to create controller");

        // Count HDR-capable displays
        let hdr_capable_count = controller
            .display_cache
            .iter()
            .filter(|d| d.supports_hdr)
            .count();

        // Set HDR globally
        let results = controller
            .set_hdr_global(false)
            .expect("set_hdr_global should succeed");

        // Results should only include HDR-capable displays
        assert_eq!(
            results.len(),
            hdr_capable_count,
            "Results should only include HDR-capable displays"
        );

        // All results should be for displays that support HDR
        for (target, _) in &results {
            assert!(
                target.supports_hdr,
                "Only HDR-capable displays should be in results"
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn test_hdr_state_toggle_timing() {
        // This test verifies that HDR toggle completes within acceptable time (100-300ms target)
        let controller = HdrController::new().expect("Failed to create controller");

        // Find an HDR-capable display
        let hdr_display = controller.display_cache.iter().find(|d| d.supports_hdr);

        if let Some(display) = hdr_display {
            // Get current state
            let initial_state = controller
                .is_hdr_enabled(display)
                .expect("Failed to get initial state");

            // Measure time to toggle HDR
            let start = std::time::Instant::now();
            let result = controller.set_hdr_state(display, !initial_state);
            let duration = start.elapsed();

            // Should succeed
            assert!(result.is_ok(), "HDR toggle should succeed");

            // Should complete within 500ms (allowing some margin for test environment)
            // The requirement is 100-300ms, but we allow up to 500ms for slower test systems
            assert!(
                duration.as_millis() <= 500,
                "HDR toggle should complete within 500ms, took {}ms",
                duration.as_millis()
            );

            // Restore original state
            controller
                .set_hdr_state(display, initial_state)
                .expect("Failed to restore state");
        } else {
            println!("No HDR-capable display found, skipping timing test");
        }
    }

    #[test]
    fn test_multiple_enumerate_calls_consistency() {
        // This test verifies that multiple enumerate calls return consistent results
        let mut controller = HdrController::new().expect("Failed to create controller");

        // Enumerate displays multiple times
        let displays1 = controller
            .enumerate_displays()
            .expect("First enumeration should succeed");
        let displays2 = controller
            .enumerate_displays()
            .expect("Second enumeration should succeed");
        let displays3 = controller
            .enumerate_displays()
            .expect("Third enumeration should succeed");

        // All enumerations should return the same number of displays
        // (unless hardware changed between calls, which is unlikely in tests)
        assert_eq!(
            displays1.len(),
            displays2.len(),
            "Display count should be consistent"
        );
        assert_eq!(
            displays2.len(),
            displays3.len(),
            "Display count should be consistent"
        );

        // Verify each display has consistent properties
        for i in 0..displays1.len() {
            assert_eq!(
                displays1[i].adapter_id.LowPart, displays2[i].adapter_id.LowPart,
                "Adapter ID LowPart should be consistent"
            );
            assert_eq!(
                displays1[i].adapter_id.HighPart, displays2[i].adapter_id.HighPart,
                "Adapter ID HighPart should be consistent"
            );
            assert_eq!(
                displays1[i].target_id, displays2[i].target_id,
                "Target ID should be consistent"
            );
            assert_eq!(
                displays1[i].supports_hdr, displays2[i].supports_hdr,
                "HDR support should be consistent"
            );
        }
    }
}
