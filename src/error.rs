//! Error types for EasyHDR application
//!
//! This module defines all error types used throughout the application,
//! providing clear error messages and proper error propagation.

use thiserror::Error;

/// Main error type for EasyHDR application
#[derive(Debug, Error)]
pub enum EasyHdrError {
    /// HDR is not supported on the display
    #[error("HDR not supported on this display")]
    HdrNotSupported,
    
    /// Failed to control HDR settings
    #[error("Failed to control HDR: {0}")]
    HdrControlFailed(String),
    
    /// Display driver error
    #[error("Display driver error: {0}")]
    DriverError(String),
    
    /// Process monitoring error
    #[error("Process monitoring error: {0}")]
    ProcessMonitorError(String),
    
    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    /// Windows API error
    #[cfg(windows)]
    #[error("Windows API error: {0}")]
    WindowsApiError(#[from] windows::core::Error),
    
    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Result type alias for EasyHDR operations
pub type Result<T> = std::result::Result<T, EasyHdrError>;

/// Convert an error to a user-friendly message
///
/// This function takes an EasyHdrError and returns a message suitable
/// for displaying to end users in error dialogs.
pub fn get_user_friendly_error(error: &EasyHdrError) -> String {
    match error {
        EasyHdrError::HdrNotSupported => {
            "Your display doesn't support HDR. Please check your hardware specifications.".to_string()
        }
        EasyHdrError::HdrControlFailed(_) | EasyHdrError::DriverError(_) => {
            "Unable to control HDR. Please ensure your display drivers are up to date.".to_string()
        }
        EasyHdrError::ProcessMonitorError(_) => {
            "Failed to monitor processes. The application may not function correctly.".to_string()
        }
        EasyHdrError::ConfigError(_) => {
            "Failed to load or save configuration. Your settings may not persist.".to_string()
        }
        #[cfg(windows)]
        EasyHdrError::WindowsApiError(e) => {
            format!("Windows API error: {}. Please check your system configuration.", e)
        }
        EasyHdrError::IoError(e) => {
            format!("File system error: {}. Please check file permissions.", e)
        }
        EasyHdrError::JsonError(e) => {
            format!("Configuration format error: {}. The configuration file may be corrupted.", e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let error = EasyHdrError::HdrNotSupported;
        assert_eq!(error.to_string(), "HDR not supported on this display");
    }

    #[test]
    fn test_user_friendly_messages() {
        let error = EasyHdrError::HdrNotSupported;
        let message = get_user_friendly_error(&error);
        assert!(message.contains("display doesn't support HDR"));
    }

    #[test]
    fn test_error_from_io() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let error: EasyHdrError = io_error.into();
        assert!(matches!(error, EasyHdrError::IoError(_)));
    }
}

