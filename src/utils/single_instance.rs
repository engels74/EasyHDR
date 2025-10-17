//! Single instance enforcement
//!
//! Ensures only one instance of the application runs at a time using a Windows named mutex.

use crate::error::Result;

#[cfg(windows)]
use crate::error::EasyHdrError;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::Threading::{CreateMutexW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};

/// Single instance guard using a Windows named mutex (released on drop)
#[cfg(windows)]
pub struct SingleInstanceGuard {
    mutex_handle: HANDLE,
}

#[cfg(windows)]
impl SingleInstanceGuard {
    /// Create a new single instance guard, returning an error if another instance is running
    #[allow(unsafe_code)] // Windows FFI for mutex
    pub fn new() -> Result<Self> {
        use tracing::{debug, error};
        use windows::core::HSTRING;

        // Create a unique name for the mutex based on the application name
        let mutex_name = HSTRING::from("Global\\EasyHDR_SingleInstance_Mutex");

        unsafe {
            // First, try to open an existing mutex
            // If this succeeds, another instance is already running
            if let Ok(existing_handle) = OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &mutex_name)
            {
                // Mutex already exists - another instance is running
                error!("Another instance of EasyHDR is already running");
                let _ = CloseHandle(existing_handle);
                Err(EasyHdrError::ConfigError(
                    "Another instance of EasyHDR is already running".to_string(),
                ))
            } else {
                // Mutex doesn't exist, create it
                let mutex_handle = CreateMutexW(None, true, &mutex_name)?;
                debug!("Single instance mutex created successfully");
                Ok(Self { mutex_handle })
            }
        }
    }
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    #[allow(unsafe_code)] // Windows FFI for mutex cleanup
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
    /// Create a new single instance guard (stub for non-Windows, always succeeds)
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
