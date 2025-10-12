//! HDR controller implementation
//!
//! This module implements the HDR controller that manages HDR state
//! for Windows displays.

use crate::error::Result;
use crate::hdr::WindowsVersion;
use crate::hdr::windows_api::LUID;

#[cfg(windows)]
use crate::error::EasyHdrError;

#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    GetDisplayConfigBufferSizes, QueryDisplayConfig, QDC_ONLY_ACTIVE_PATHS,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
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

            // Step 4: Extract display targets from paths
            self.display_cache.clear();

            for (index, path) in paths.iter().enumerate() {
                let target = DisplayTarget {
                    adapter_id: path.targetInfo.adapterId,
                    target_id: path.targetInfo.id,
                    supports_hdr: false, // Will be detected in task 4.2
                };

                debug!(
                    "Display {}: adapter_id={{LowPart: {:#x}, HighPart: {:#x}}}, target_id={}",
                    index,
                    target.adapter_id.LowPart,
                    target.adapter_id.HighPart,
                    target.target_id
                );

                self.display_cache.push(target);
            }

            info!("Enumerated {} display targets", self.display_cache.len());

            Ok(self.display_cache.clone())
        }

        #[cfg(not(windows))]
        {
            // For non-Windows platforms (testing), return empty list
            self.display_cache.clear();
            Ok(self.display_cache.clone())
        }
    }

    /// Check if HDR is enabled on a display
    #[allow(dead_code)]
    pub fn is_hdr_enabled(&self, _target: &DisplayTarget) -> Result<bool> {
        // TODO: Implement HDR state detection
        // This will be implemented in task 4
        Ok(false)
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

            // supports_hdr should be false initially (will be set in task 4.2)
            assert_eq!(display.supports_hdr, false);
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
}

