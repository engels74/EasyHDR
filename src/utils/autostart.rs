//! Auto-start registry management
//!
//! This module provides functionality to manage Windows auto-start
//! via registry entries in HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run.
//!
//! # Requirements
//!
//! - Requirement 6.6: Create registry entry when auto-start is enabled
//! - Requirement 6.7: Remove registry entry when auto-start is disabled
//! - Requirement 6.8: Handle registry access errors gracefully with user-friendly messages

use crate::error::Result;

#[cfg(windows)]
use crate::error::EasyHdrError;
#[cfg(windows)]
use tracing::{error, info};
#[cfg(windows)]
use winreg::enums::*;
#[cfg(windows)]
use winreg::RegKey;

/// Registry key path for Windows auto-start applications
#[cfg(windows)]
const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Application name used in the registry
#[cfg(windows)]
const APP_NAME: &str = "EasyHDR";

/// Auto-start manager for Windows registry operations
///
/// This struct provides methods to check, enable, and disable auto-start
/// functionality by managing registry entries in HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
pub struct AutoStartManager;

impl AutoStartManager {
    /// Check if auto-start is enabled
    ///
    /// Checks if the EasyHDR registry entry exists in the Windows Run key.
    ///
    /// # Returns
    ///
    /// - `Ok(true)` if auto-start is enabled (registry entry exists)
    /// - `Ok(false)` if auto-start is disabled (registry entry does not exist)
    /// - `Err` if there was an error accessing the registry
    ///
    /// # Requirements
    ///
    /// - Requirement 6.6: Check HKCU\Software\Microsoft\Windows\CurrentVersion\Run
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use easyhdr::utils::AutoStartManager;
    ///
    /// let is_enabled = AutoStartManager::is_enabled()?;
    /// println!("Auto-start is {}", if is_enabled { "enabled" } else { "disabled" });
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    #[cfg(windows)]
    pub fn is_enabled() -> Result<bool> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // Try to open the Run key
        let run_key = match hkcu.open_subkey(RUN_KEY_PATH) {
            Ok(key) => key,
            Err(e) => {
                error!("Failed to open registry key {}: {}", RUN_KEY_PATH, e);
                return Err(EasyHdrError::ConfigError(format!(
                    "Failed to access Windows auto-start registry: {}",
                    e
                )));
            }
        };

        // Check if our app entry exists
        match run_key.get_value::<String, _>(APP_NAME) {
            Ok(_) => {
                info!("Auto-start is enabled");
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!("Auto-start is disabled");
                Ok(false)
            }
            Err(e) => {
                error!("Failed to read registry value {}: {}", APP_NAME, e);
                Err(EasyHdrError::ConfigError(format!(
                    "Failed to check auto-start status: {}",
                    e
                )))
            }
        }
    }

    /// Enable auto-start
    ///
    /// Creates a registry entry in HKCU\Software\Microsoft\Windows\CurrentVersion\Run
    /// with the current executable path, causing the application to start automatically
    /// when the user logs in to Windows.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if auto-start was successfully enabled
    /// - `Err` if there was an error accessing the registry or getting the executable path
    ///
    /// # Requirements
    ///
    /// - Requirement 6.6: Create registry entry in HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
    /// - Requirement 6.8: Handle registry access errors gracefully with user-friendly messages
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use easyhdr::utils::AutoStartManager;
    ///
    /// AutoStartManager::enable()?;
    /// println!("Auto-start enabled successfully");
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    #[cfg(windows)]
    pub fn enable() -> Result<()> {
        // Get the current executable path
        let exe_path = std::env::current_exe().map_err(|e| {
            error!("Failed to get current executable path: {}", e);
            EasyHdrError::ConfigError(format!(
                "Failed to determine application location: {}",
                e
            ))
        })?;

        let exe_path_str = exe_path.to_string_lossy();

        // Open the registry key with write permissions
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = match hkcu.open_subkey_with_flags(RUN_KEY_PATH, KEY_WRITE) {
            Ok(key) => key,
            Err(e) => {
                error!("Failed to open registry key {} for writing: {}", RUN_KEY_PATH, e);
                return Err(EasyHdrError::ConfigError(format!(
                    "Failed to access Windows auto-start registry. Please check your permissions: {}",
                    e
                )));
            }
        };

        // Set the registry value
        if let Err(e) = run_key.set_value(APP_NAME, &exe_path_str.as_ref()) {
            error!("Failed to set registry value {}: {}", APP_NAME, e);
            return Err(EasyHdrError::ConfigError(format!(
                "Failed to enable auto-start. Please check your permissions: {}",
                e
            )));
        }

        info!("Auto-start enabled: {} -> {}", APP_NAME, exe_path_str);
        Ok(())
    }

    /// Disable auto-start
    ///
    /// Removes the registry entry from HKCU\Software\Microsoft\Windows\CurrentVersion\Run,
    /// preventing the application from starting automatically when the user logs in to Windows.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if auto-start was successfully disabled
    /// - `Err` if there was an error accessing the registry
    ///
    /// # Requirements
    ///
    /// - Requirement 6.7: Remove the registry entry
    /// - Requirement 6.8: Handle registry access errors gracefully with user-friendly messages
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use easyhdr::utils::AutoStartManager;
    ///
    /// AutoStartManager::disable()?;
    /// println!("Auto-start disabled successfully");
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    #[cfg(windows)]
    pub fn disable() -> Result<()> {
        // Open the registry key with write permissions
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = match hkcu.open_subkey_with_flags(RUN_KEY_PATH, KEY_WRITE) {
            Ok(key) => key,
            Err(e) => {
                error!("Failed to open registry key {} for writing: {}", RUN_KEY_PATH, e);
                return Err(EasyHdrError::ConfigError(format!(
                    "Failed to access Windows auto-start registry. Please check your permissions: {}",
                    e
                )));
            }
        };

        // Delete the registry value
        match run_key.delete_value(APP_NAME) {
            Ok(()) => {
                info!("Auto-start disabled: {} removed from registry", APP_NAME);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Value doesn't exist, which is fine - it's already disabled
                info!("Auto-start was already disabled");
                Ok(())
            }
            Err(e) => {
                error!("Failed to delete registry value {}: {}", APP_NAME, e);
                Err(EasyHdrError::ConfigError(format!(
                    "Failed to disable auto-start. Please check your permissions: {}",
                    e
                )))
            }
        }
    }

    /// Non-Windows stub for is_enabled
    #[cfg(not(windows))]
    pub fn is_enabled() -> Result<bool> {
        Ok(false)
    }

    /// Non-Windows stub for enable
    #[cfg(not(windows))]
    pub fn enable() -> Result<()> {
        Ok(())
    }

    /// Non-Windows stub for disable
    #[cfg(not(windows))]
    pub fn disable() -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that auto-start can be enabled and disabled
    ///
    /// This test verifies the complete lifecycle:
    /// 1. Disable auto-start (cleanup from previous runs)
    /// 2. Verify it's disabled
    /// 3. Enable auto-start
    /// 4. Verify it's enabled
    /// 5. Disable auto-start
    /// 6. Verify it's disabled again
    #[test]
    #[cfg(windows)]
    fn test_autostart_lifecycle() {
        // Cleanup: ensure auto-start is disabled before we start
        let _ = AutoStartManager::disable();

        // Verify it's disabled
        let is_enabled = AutoStartManager::is_enabled()
            .expect("Failed to check auto-start status");
        assert!(!is_enabled, "Auto-start should be disabled initially");

        // Enable auto-start
        AutoStartManager::enable()
            .expect("Failed to enable auto-start");

        // Verify it's enabled
        let is_enabled = AutoStartManager::is_enabled()
            .expect("Failed to check auto-start status after enabling");
        assert!(is_enabled, "Auto-start should be enabled after calling enable()");

        // Disable auto-start
        AutoStartManager::disable()
            .expect("Failed to disable auto-start");

        // Verify it's disabled
        let is_enabled = AutoStartManager::is_enabled()
            .expect("Failed to check auto-start status after disabling");
        assert!(!is_enabled, "Auto-start should be disabled after calling disable()");
    }

    /// Test that disabling auto-start when it's already disabled doesn't error
    #[test]
    #[cfg(windows)]
    fn test_disable_when_already_disabled() {
        // Ensure it's disabled first
        let _ = AutoStartManager::disable();

        // Try to disable again - should succeed without error
        let result = AutoStartManager::disable();
        assert!(result.is_ok(), "Disabling when already disabled should succeed");

        // Verify it's still disabled
        let is_enabled = AutoStartManager::is_enabled()
            .expect("Failed to check auto-start status");
        assert!(!is_enabled, "Auto-start should remain disabled");
    }

    /// Test that enabling auto-start multiple times is idempotent
    #[test]
    #[cfg(windows)]
    fn test_enable_idempotent() {
        // Cleanup
        let _ = AutoStartManager::disable();

        // Enable twice
        AutoStartManager::enable()
            .expect("First enable should succeed");
        AutoStartManager::enable()
            .expect("Second enable should succeed");

        // Verify it's enabled
        let is_enabled = AutoStartManager::is_enabled()
            .expect("Failed to check auto-start status");
        assert!(is_enabled, "Auto-start should be enabled");

        // Cleanup
        let _ = AutoStartManager::disable();
    }

    /// Test non-Windows stubs
    #[test]
    #[cfg(not(windows))]
    fn test_non_windows_stubs() {
        // On non-Windows platforms, these should just return Ok without errors
        assert!(AutoStartManager::is_enabled().is_ok());
        assert!(AutoStartManager::enable().is_ok());
        assert!(AutoStartManager::disable().is_ok());
    }
}


