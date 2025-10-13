//! HDR control module
//!
//! This module provides functionality to control HDR settings on Windows displays
//! using the Windows Display Configuration API.
//!
//! # Overview
//!
//! The HDR control system provides:
//! - **Display enumeration** using Windows Display Configuration API
//! - **HDR capability detection** with version-specific APIs
//! - **HDR state control** (enable/disable) for individual displays or globally
//! - **Windows version detection** to select appropriate APIs
//!
//! # Architecture
//!
//! - `HdrController`: Main controller for HDR operations
//! - `DisplayTarget`: Represents a physical display with adapter and target IDs
//! - `WindowsVersion`: Enum for Windows version detection
//! - `windows_api`: Low-level Windows API structures and constants
//!
//! # Windows API Integration
//!
//! This module uses different Windows APIs depending on the Windows version:
//!
//! ## Windows 11 24H2+ (Build 26100+)
//!
//! - Uses `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` for HDR detection
//! - Uses `DISPLAYCONFIG_SET_HDR_STATE` for HDR control
//! - Provides `highDynamicRangeSupported` and `highDynamicRangeEnabled` flags
//!
//! ## Windows 10/11 (Before 24H2)
//!
//! - Uses `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO` for HDR detection
//! - Uses `DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE` for HDR control
//! - Checks `advancedColorSupported`, `advancedColorEnabled`, and `wideColorEnforced` flags
//!
//! # Example Usage
//!
//! ```no_run
//! use easyhdr::hdr::HdrController;
//!
//! // Create HDR controller (detects Windows version automatically)
//! let controller = HdrController::new()?;
//!
//! // Get cached display list
//! let displays = controller.get_display_cache();
//! println!("Found {} displays", displays.len());
//!
//! // Check HDR support and state for each display
//! for display in displays {
//!     let supported = controller.is_hdr_supported(display)?;
//!     let enabled = controller.is_hdr_enabled(display)?;
//!     println!("Display: HDR supported={}, enabled={}", supported, enabled);
//! }
//!
//! // Enable HDR globally (all displays)
//! controller.set_hdr_global(true)?;
//! println!("HDR enabled on all displays");
//!
//! // Disable HDR globally
//! controller.set_hdr_global(false)?;
//! println!("HDR disabled on all displays");
//! # Ok::<(), easyhdr::error::EasyHdrError>(())
//! ```
//!
//! # Requirements
//!
//! - Requirement 3.1: Enumerate displays using QueryDisplayConfig
//! - Requirement 3.2: Cache display enumeration results
//! - Requirement 3.3: Detect Windows version to select appropriate APIs
//! - Requirement 3.4: Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 for Windows 11 24H2+
//! - Requirement 3.5: Use DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO for older Windows
//! - Requirement 3.8: Use DISPLAYCONFIG_SET_HDR_STATE for Windows 11 24H2+
//! - Requirement 3.9: Use DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE for older Windows

pub mod controller;
pub mod version;
pub mod windows_api;

pub use controller::{DisplayTarget, HdrController};
pub use version::WindowsVersion;

