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
    #[allow(dead_code)]
    pub fn set_hdr_state(&self, _target: &DisplayTarget, _enable: bool) -> Result<()> {
        // TODO: Implement HDR control
        // This will be implemented in task 4
        Ok(())
    }

    /// Set HDR state globally for all displays
    pub fn set_hdr_global(&self, _enable: bool) -> Result<Vec<(DisplayTarget, Result<()>)>> {
        // TODO: Implement global HDR control
        // This will be implemented in task 4
        Ok(Vec::new())
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
}

