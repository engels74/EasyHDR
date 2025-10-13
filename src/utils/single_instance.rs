//! Single instance enforcement
//!
//! This module provides functionality to ensure only one instance of the application
//! runs at a time using a Windows named mutex.

use crate::error::Result;

#[cfg(windows)]
use crate::error::EasyHdrError;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;

/// Single instance guard
///
/// This struct holds a Windows named mutex that ensures only one instance
/// of the application can run at a time. When dropped, the mutex is released.
#[cfg(windows)]
pub struct SingleInstanceGuard {
    mutex_handle: HANDLE,
}

#[cfg(windows)]
impl SingleInstanceGuard {
    /// Create a new single instance guard
    ///
    /// This function attempts to create a named mutex. If the mutex already exists,
    /// it means another instance is running and an error is returned.
    ///
    /// # Returns
    ///
    /// Returns Ok(SingleInstanceGuard) if this is the first instance, or an error
    /// if another instance is already running.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::utils::single_instance::SingleInstanceGuard;
    ///
    /// let _guard = SingleInstanceGuard::new()?;
    /// // Application runs...
    /// // When _guard is dropped, the mutex is released
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    pub fn new() -> Result<Self> {
        use tracing::{debug, error};
        use windows::core::HSTRING;

        // Create a unique name for the mutex based on the application name
        let mutex_name = HSTRING::from("Global\\EasyHDR_SingleInstance_Mutex");

        unsafe {
            // Try to create the mutex
            let mutex_handle = CreateMutexW(None, true, &mutex_name)?;

            // Check if the mutex already existed by checking the last Win32 error
            // In windows-rs 0.52, GetLastError() returns Result<(), Error>
            // We need to check the error code directly using windows::core::Error::from_win32()
            let last_error_code = windows::core::Error::from_win32().code().0 as u32;

            // ERROR_ALREADY_EXISTS = 183
            if last_error_code == 183 {
                // Another instance is already running
                error!("Another instance of EasyHDR is already running");

                // Close the handle we just created
                let _ = CloseHandle(mutex_handle);

                return Err(EasyHdrError::ConfigError(
                    "Another instance of EasyHDR is already running".to_string(),
                ));
            }

            debug!("Single instance mutex created successfully");

            Ok(Self { mutex_handle })
        }
    }
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        use tracing::debug;

        unsafe {
            // Release the mutex when the guard is dropped
            let _ = CloseHandle(self.mutex_handle);
            debug!("Single instance mutex released");
        }
    }
}

/// Stub implementation for non-Windows platforms
#[cfg(not(windows))]
pub struct SingleInstanceGuard;

#[cfg(not(windows))]
impl SingleInstanceGuard {
    /// Create a new single instance guard (stub for non-Windows)
    ///
    /// On non-Windows platforms, this always succeeds as single-instance
    /// enforcement is only needed on Windows.
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn test_single_instance_guard_creation() {
        // First instance should succeed
        let guard1 = SingleInstanceGuard::new();
        assert!(guard1.is_ok(), "First instance should succeed");

        // Second instance should fail
        let guard2 = SingleInstanceGuard::new();
        assert!(guard2.is_err(), "Second instance should fail");

        // Drop the first guard
        drop(guard1);

        // Now a new instance should succeed
        let guard3 = SingleInstanceGuard::new();
        assert!(guard3.is_ok(), "Instance after drop should succeed");
    }

    #[test]
    #[cfg(not(windows))]
    fn test_single_instance_guard_stub() {
        // On non-Windows, should always succeed
        let guard1 = SingleInstanceGuard::new();
        assert!(guard1.is_ok());

        let guard2 = SingleInstanceGuard::new();
        assert!(guard2.is_ok());
    }
}
