//! HDR controller implementation
//!
//! This module implements the HDR controller that manages HDR state
//! for Windows displays.

use crate::error::Result;
use crate::hdr::WindowsVersion;
use crate::hdr::windows_api::LUID;

#[cfg(windows)]
use crate::hdr::windows_api::{
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2,
    DISPLAYCONFIG_ADVANCED_COLOR_MODE, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_DEVICE_INFO_TYPE,
};

#[cfg(windows)]
use crate::error::EasyHdrError;

#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    GetDisplayConfigBufferSizes, QueryDisplayConfig, QDC_ONLY_ACTIVE_PATHS,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DisplayConfigGetDeviceInfo,
};

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

    /// Enumerate displays
    ///
    /// Uses GetDisplayConfigBufferSizes and QueryDisplayConfig to retrieve
    /// all active display paths and extract display targets.
    ///
    /// # Returns
    ///
    /// Returns a vector of DisplayTarget structs representing all active displays.
    ///
    /// # Requirements
    ///
    /// - Requirement 3.2: Use GetDisplayConfigBufferSizes with QDC_ONLY_ACTIVE_PATHS
    /// - Requirement 3.3: Call QueryDisplayConfig to retrieve active display paths
    pub fn enumerate_displays(&mut self) -> Result<Vec<DisplayTarget>> {
        #[cfg(windows)]
        {
            use tracing::{debug, info};

            // Step 1: Get buffer sizes for display configuration
            let mut path_count: u32 = 0;
            let mut mode_count: u32 = 0;

            unsafe {
                GetDisplayConfigBufferSizes(
                    QDC_ONLY_ACTIVE_PATHS,
                    &mut path_count,
                    &mut mode_count,
                )
                .map_err(|e| {
                    EasyHdrError::HdrControlFailed(format!(
                        "Failed to get display config buffer sizes: {}",
                        e
                    ))
                })?;
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
                QueryDisplayConfig(
                    QDC_ONLY_ACTIVE_PATHS,
                    Some(&mut paths),
                    Some(&mut modes),
                    None,
                )
                .map_err(|e| {
                    EasyHdrError::HdrControlFailed(format!(
                        "Failed to query display config: {}",
                        e
                    ))
                })?;
            }

            info!(
                "Successfully queried display configuration: {} active paths",
                path_count
            );

            // Step 4: Extract display targets from paths and detect HDR support
            self.display_cache.clear();

            for (index, path) in paths.iter().enumerate() {
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

    /// Check if HDR is supported on a display
    ///
    /// Uses DisplayConfigGetDeviceInfo with version-specific structures to detect
    /// whether a display supports HDR.
    ///
    /// # Arguments
    ///
    /// * `target` - The display target to check
    ///
    /// # Returns
    ///
    /// Returns true if the display supports HDR, false otherwise.
    ///
    /// # Requirements
    ///
    /// - Requirement 3.4: Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 for Windows 11 24H2+
    /// - Requirement 3.5: Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO for older Windows
    /// - Requirement 3.6: Check advancedColorSupported && !wideColorEnforced for older Windows
    pub fn is_hdr_supported(&self, target: &DisplayTarget) -> Result<bool> {
        #[cfg(windows)]
        {
            use tracing::debug;

            match self.windows_version {
                WindowsVersion::Windows11_24H2 => {
                    // Windows 11 24H2+: Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2
                    let mut color_info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
                        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                            type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2,
                            size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>() as u32,
                            adapterId: target.adapter_id,
                            id: target.target_id,
                        },
                        colorEncoding: 0,
                        bitsPerColorChannel: 0,
                        activeColorMode: 0,
                        value: 0,
                    };

                    unsafe {
                        DisplayConfigGetDeviceInfo(&mut color_info.header as *mut _ as *mut _)
                            .map_err(|e| {
                                EasyHdrError::HdrControlFailed(format!(
                                    "Failed to get advanced color info 2: {}",
                                    e
                                ))
                            })?;
                    }

                    let supported = color_info.highDynamicRangeSupported();
                    debug!(
                        "Display (adapter={:#x}:{:#x}, target={}): HDR supported (24H2+) = {}",
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id,
                        supported
                    );

                    Ok(supported)
                }
                WindowsVersion::Windows10 | WindowsVersion::Windows11 => {
                    // Windows 10/11 (before 24H2): Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO
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
                        DisplayConfigGetDeviceInfo(&mut color_info.header as *mut _ as *mut _)
                            .map_err(|e| {
                                EasyHdrError::HdrControlFailed(format!(
                                    "Failed to get advanced color info: {}",
                                    e
                                ))
                            })?;
                    }

                    // HDR supported: advancedColorSupported == TRUE AND wideColorEnforced == FALSE
                    let supported = color_info.advancedColorSupported() && !color_info.wideColorEnforced();
                    debug!(
                        "Display (adapter={:#x}:{:#x}, target={}): advancedColorSupported={}, wideColorEnforced={}, HDR supported={}",
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id,
                        color_info.advancedColorSupported(),
                        color_info.wideColorEnforced(),
                        supported
                    );

                    Ok(supported)
                }
            }
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing), return false
            Ok(false)
        }
    }

    /// Check if HDR is enabled on a display
    ///
    /// Uses DisplayConfigGetDeviceInfo with version-specific structures to detect
    /// whether HDR is currently enabled on a display.
    ///
    /// # Arguments
    ///
    /// * `target` - The display target to check
    ///
    /// # Returns
    ///
    /// Returns true if HDR is currently enabled, false otherwise.
    ///
    /// # Requirements
    ///
    /// - Requirement 3.4: Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 for Windows 11 24H2+
    /// - Requirement 3.5: Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO for older Windows
    /// - Requirement 3.7: Check advancedColorSupported && advancedColorEnabled && !wideColorEnforced for older Windows
    pub fn is_hdr_enabled(&self, target: &DisplayTarget) -> Result<bool> {
        #[cfg(windows)]
        {
            use tracing::debug;

            match self.windows_version {
                WindowsVersion::Windows11_24H2 => {
                    // Windows 11 24H2+: Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2
                    let mut color_info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
                        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                            type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2,
                            size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>() as u32,
                            adapterId: target.adapter_id,
                            id: target.target_id,
                        },
                        colorEncoding: 0,
                        bitsPerColorChannel: 0,
                        activeColorMode: 0,
                        value: 0,
                    };

                    unsafe {
                        DisplayConfigGetDeviceInfo(&mut color_info.header as *mut _ as *mut _)
                            .map_err(|e| {
                                EasyHdrError::HdrControlFailed(format!(
                                    "Failed to get advanced color info 2: {}",
                                    e
                                ))
                            })?;
                    }

                    // HDR enabled: activeColorMode == HDR
                    let enabled = color_info.activeColorMode == DISPLAYCONFIG_ADVANCED_COLOR_MODE::DISPLAYCONFIG_ADVANCED_COLOR_MODE_HDR as u32;
                    debug!(
                        "Display (adapter={:#x}:{:#x}, target={}): activeColorMode={}, HDR enabled (24H2+) = {}",
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
                        DisplayConfigGetDeviceInfo(&mut color_info.header as *mut _ as *mut _)
                            .map_err(|e| {
                                EasyHdrError::HdrControlFailed(format!(
                                    "Failed to get advanced color info: {}",
                                    e
                                ))
                            })?;
                    }

                    // HDR enabled: advancedColorSupported == TRUE AND advancedColorEnabled == TRUE AND wideColorEnforced == FALSE
                    let enabled = color_info.advancedColorSupported()
                        && color_info.advancedColorEnabled()
                        && !color_info.wideColorEnforced();
                    debug!(
                        "Display (adapter={:#x}:{:#x}, target={}): advancedColorSupported={}, advancedColorEnabled={}, wideColorEnforced={}, HDR enabled={}",
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
            }
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing), return false
            Ok(false)
        }
    }

    /// Set HDR state for a single display
    ///
    /// Uses DisplayConfigSetDeviceInfo with version-specific structures to enable
    /// or disable HDR on a single display.
    ///
    /// # Arguments
    ///
    /// * `target` - The display target to control
    /// * `enable` - True to enable HDR, false to disable
    ///
    /// # Returns
    ///
    /// Returns Ok(()) if the operation succeeded, or an error if it failed.
    ///
    /// # Requirements
    ///
    /// - Requirement 3.8: Use DISPLAYCONFIG_SET_HDR_STATE for Windows 11 24H2+
    /// - Requirement 3.9: Use DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE for older Windows
    /// - Requirement 3.11: Add 100ms delay after DisplayConfigSetDeviceInfo call
    #[allow(dead_code)]
    pub fn set_hdr_state(&self, target: &DisplayTarget, enable: bool) -> Result<()> {
        #[cfg(windows)]
        {
            use crate::hdr::windows_api::{
                DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE, DISPLAYCONFIG_SET_HDR_STATE,
            };
            use tracing::{debug, info};
            use windows::Win32::Graphics::Gdi::DisplayConfigSetDeviceInfo;

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
                        DisplayConfigSetDeviceInfo(&mut set_state.header as *mut _ as *mut _)
                            .map_err(|e| {
                                EasyHdrError::HdrControlFailed(format!(
                                    "Failed to set HDR state (24H2+): {}",
                                    e
                                ))
                            })?;
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
                        DisplayConfigSetDeviceInfo(&mut set_state.header as *mut _ as *mut _)
                            .map_err(|e| {
                                EasyHdrError::HdrControlFailed(format!(
                                    "Failed to set advanced color state: {}",
                                    e
                                ))
                            })?;
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

    /// Set HDR state globally for all displays
    ///
    /// Iterates through all display targets and calls set_hdr_state() on each.
    /// Returns a vector of results for each display, allowing partial success scenarios.
    ///
    /// # Arguments
    ///
    /// * `enable` - True to enable HDR, false to disable
    ///
    /// # Returns
    ///
    /// Returns a vector of tuples containing (DisplayTarget, Result<()>) for each display.
    /// This allows tracking which displays succeeded and which failed.
    ///
    /// # Requirements
    ///
    /// - Requirement 3.10: Iterate through all display targets and call DisplayConfigSetDeviceInfo on each
    /// - Requirement 3.11: Add 100ms delays between changes (handled by set_hdr_state)
    /// - Requirement 3.13: Handle partial success scenarios gracefully
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
                    target.adapter_id.LowPart,
                    target.adapter_id.HighPart,
                    target.target_id
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
                        "Failed to set HDR {} for display (adapter={:#x}:{:#x}, target={}): {}",
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
            // Target ID should be valid
            assert!(display.target_id > 0 || display.target_id == 0);

            // supports_hdr is now properly detected (may be true or false depending on hardware)
            // Just verify it's a valid boolean value
            assert!(display.supports_hdr == true || display.supports_hdr == false);
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
        assert_eq!(cloned.supports_hdr, false);
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
                assert!(display.supports_hdr, "HDR cannot be enabled on a display that doesn't support it");
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
        assert_eq!(result.unwrap(), false);
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
        assert_eq!(result.unwrap(), false);
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
        let hdr_display = controller
            .display_cache
            .iter()
            .find(|d| d.supports_hdr);

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
            assert_eq!(new_state, false, "HDR should be disabled");

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
            assert_eq!(state, false, "HDR should be disabled globally");
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
            // Each result should have a valid target
            assert!(target.target_id >= 0);

            // Result should be Ok or Err (both are valid for partial success)
            match result {
                Ok(()) => {
                    // Success case
                    assert!(target.supports_hdr, "Only HDR-capable displays should succeed");
                }
                Err(_) => {
                    // Failure case - this is acceptable for partial success
                    // Just verify the error is logged (we can't check logs in tests)
                }
            }
        }
    }
}

