//! Auto-start registry management
//!
//! Manages Windows auto-start functionality via registry entries in
//! `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`.

use crate::error::Result;

#[cfg(windows)]
use crate::error::EasyHdrError;
#[cfg(windows)]
use tracing::{error, info};
#[cfg(windows)]
use winreg::RegKey;
#[cfg(windows)]
use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};

/// Registry key path for Windows auto-start applications
#[cfg(windows)]
const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Application name used in the registry
#[cfg(windows)]
const APP_NAME: &str = "EasyHDR";

/// Auto-start manager for Windows registry operations
pub struct AutoStartManager;

impl AutoStartManager {
    /// Check if auto-start is enabled by checking for the registry entry
    #[cfg(windows)]
    pub fn is_enabled() -> Result<bool> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // Try to open the Run key
        let run_key = match hkcu.open_subkey(RUN_KEY_PATH) {
            Ok(key) => key,
            Err(e) => {
                error!("Failed to open registry key {RUN_KEY_PATH}: {e}");
                return Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                    format!("Failed to access Windows auto-start registry: {e}"),
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
                error!("Failed to read registry value {APP_NAME}: {e}");
                Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                    format!("Failed to check auto-start status: {e}"),
                )))
            }
        }
    }

    /// Enable auto-start by creating a registry entry with the current executable path
    #[cfg(windows)]
    pub fn enable() -> Result<()> {
        // Get the current executable path
        let exe_path = std::env::current_exe().map_err(|e| {
            error!("Failed to get current executable path: {e}");
            EasyHdrError::ConfigError(crate::error::StringError::new(format!(
                "Failed to determine application location: {e}"
            )))
        })?;

        // Quote the path to handle spaces (e.g., "C:\Program Files\EasyHDR\easyhdr.exe")
        // This respects Windows shell parsing rules per Platform Fit & OS Contracts guidelines
        let quoted_path = format!("\"{}\"", exe_path.to_string_lossy());

        // Open the registry key with write permissions
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = match hkcu.open_subkey_with_flags(RUN_KEY_PATH, KEY_WRITE) {
            Ok(key) => key,
            Err(e) => {
                error!("Failed to open registry key {RUN_KEY_PATH} for writing: {e}");
                return Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                    format!(
                        "Failed to access Windows auto-start registry. Please check your permissions: {e}"
                    ),
                )));
            }
        };

        // Set the registry value with quoted path
        if let Err(e) = run_key.set_value(APP_NAME, &quoted_path) {
            error!("Failed to set registry value {APP_NAME}: {e}");
            return Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                format!("Failed to enable auto-start. Please check your permissions: {e}"),
            )));
        }

        info!("Auto-start enabled: {APP_NAME} -> {quoted_path}");
        Ok(())
    }

    /// Disable auto-start by removing the registry entry
    #[cfg(windows)]
    pub fn disable() -> Result<()> {
        // Open the registry key with write permissions
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = match hkcu.open_subkey_with_flags(RUN_KEY_PATH, KEY_WRITE) {
            Ok(key) => key,
            Err(e) => {
                error!("Failed to open registry key {RUN_KEY_PATH} for writing: {e}");
                return Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                    format!(
                        "Failed to access Windows auto-start registry. Please check your permissions: {e}"
                    ),
                )));
            }
        };

        // Delete the registry value
        match run_key.delete_value(APP_NAME) {
            Ok(()) => {
                info!("Auto-start disabled: {APP_NAME} removed from registry");
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Value doesn't exist, which is fine - it's already disabled
                info!("Auto-start was already disabled");
                Ok(())
            }
            Err(e) => {
                error!("Failed to delete registry value {APP_NAME}: {e}");
                Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                    format!("Failed to disable auto-start. Please check your permissions: {e}"),
                )))
            }
        }
    }

    /// Non-Windows stub for `is_enabled`
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
#[allow(clippy::unwrap_used)]
mod tests {
    #[cfg(windows)]
    use super::{APP_NAME, AutoStartManager, RUN_KEY_PATH};

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
        let is_enabled = AutoStartManager::is_enabled().expect("Failed to check auto-start status");
        assert!(!is_enabled, "Auto-start should be disabled initially");

        // Enable auto-start
        AutoStartManager::enable().expect("Failed to enable auto-start");

        // Verify it's enabled
        let is_enabled = AutoStartManager::is_enabled()
            .expect("Failed to check auto-start status after enabling");
        assert!(
            is_enabled,
            "Auto-start should be enabled after calling enable()"
        );

        // Disable auto-start
        AutoStartManager::disable().expect("Failed to disable auto-start");

        // Verify it's disabled
        let is_enabled = AutoStartManager::is_enabled()
            .expect("Failed to check auto-start status after disabling");
        assert!(
            !is_enabled,
            "Auto-start should be disabled after calling disable()"
        );
    }

    /// Test that disabling auto-start when it's already disabled doesn't error
    #[test]
    #[cfg(windows)]
    fn test_disable_when_already_disabled() {
        // Ensure it's disabled first
        let _ = AutoStartManager::disable();

        // Try to disable again - should succeed without error
        let result = AutoStartManager::disable();
        assert!(
            result.is_ok(),
            "Disabling when already disabled should succeed"
        );

        // Verify it's still disabled
        let is_enabled = AutoStartManager::is_enabled().expect("Failed to check auto-start status");
        assert!(!is_enabled, "Auto-start should remain disabled");
    }

    /// Test that enabling auto-start multiple times is idempotent
    #[test]
    #[cfg(windows)]
    fn test_enable_idempotent() {
        // Cleanup
        let _ = AutoStartManager::disable();

        // Enable twice
        AutoStartManager::enable().expect("First enable should succeed");
        AutoStartManager::enable().expect("Second enable should succeed");

        // Verify it's enabled
        let is_enabled = AutoStartManager::is_enabled().expect("Failed to check auto-start status");
        assert!(is_enabled, "Auto-start should be enabled");

        // Cleanup
        let _ = AutoStartManager::disable();
    }

    /// Test that paths with spaces are correctly quoted in the registry
    ///
    /// This test verifies that executable paths containing spaces (e.g., from Program Files)
    /// are properly quoted when written to the registry, preventing silent boot failures.
    #[test]
    #[cfg(windows)]
    fn test_autostart_handles_paths_with_spaces() {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;

        // Cleanup: ensure auto-start is disabled before we start
        let _ = AutoStartManager::disable();

        // Enable auto-start (this will use the current executable path)
        AutoStartManager::enable().expect("Failed to enable auto-start");

        // Read the registry value directly to verify it's quoted
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = hkcu
            .open_subkey(RUN_KEY_PATH)
            .expect("Failed to open registry key");
        let registry_value: String = run_key
            .get_value(APP_NAME)
            .expect("Failed to read registry value");

        // Verify the path is quoted
        assert!(
            registry_value.starts_with('"') && registry_value.ends_with('"'),
            "Registry value should be quoted, got: {registry_value}"
        );

        // Verify the quoted path contains the executable name
        assert!(
            registry_value.contains("easyhdr") || registry_value.contains("autostart"),
            "Registry value should contain executable name, got: {registry_value}"
        );

        // Cleanup
        let _ = AutoStartManager::disable();
    }
}
