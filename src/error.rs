//! Error types for `EasyHDR` application
//!
//! This module defines all error types used throughout the application,
//! providing clear error messages and proper error propagation.
//!
//! Error variants use `#[source]` to preserve error chains for better
//! observability and debugging (Rust-Bible: Error Handling & Observability).

use thiserror::Error;

/// Simple error type for wrapping string messages while implementing `std::error::Error`
#[derive(Debug, Error)]
#[error("{0}")]
pub struct StringError(pub String);

impl StringError {
    /// Create a new `StringError` from a string message
    pub fn new(msg: impl Into<String>) -> Box<Self> {
        Box::new(Self(msg.into()))
    }
}

/// Main error type for `EasyHDR` application
#[derive(Debug, Error)]
pub enum EasyHdrError {
    /// HDR is not supported on the display
    #[error("HDR not supported on this display")]
    HdrNotSupported,

    /// Failed to control HDR settings
    /// Preserves the underlying error source for full error chain transparency
    #[error("Failed to control HDR: {0}")]
    HdrControlFailed(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Display driver error
    /// Preserves the underlying error source for full error chain transparency
    #[error("Display driver error: {0}")]
    DriverError(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Process monitoring error
    /// Preserves the underlying error source for full error chain transparency
    #[error("Process monitoring error: {0}")]
    ProcessMonitorError(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Configuration error
    /// Preserves the underlying error source for full error chain transparency
    #[error("Configuration error: {0}")]
    ConfigError(#[source] Box<dyn std::error::Error + Send + Sync>),

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

/// Result type alias for `EasyHDR` operations
pub type Result<T> = std::result::Result<T, EasyHdrError>;

/// Convert an error to a user-friendly message
///
/// This function takes an `EasyHdrError` and returns a message suitable
/// for displaying to end users in error dialogs.
///
/// The messages include detailed troubleshooting hints to help users
/// resolve common issues.
pub fn get_user_friendly_error(error: &EasyHdrError) -> String {
    match error {
        EasyHdrError::HdrNotSupported => "Your display doesn't support HDR.\n\n\
             Please check your hardware specifications and ensure:\n\
             - Your display supports HDR10 or higher\n\
             - Your GPU supports HDR output\n\
             - You're using a compatible connection (HDMI 2.0+ or DisplayPort 1.4+)"
            .to_string(),
        EasyHdrError::HdrControlFailed(_) | EasyHdrError::DriverError(_) => {
            "Unable to control HDR.\n\n\
             Please ensure:\n\
             - Your display drivers are up to date\n\
             - HDR is enabled in Windows display settings\n\
             - Your display is properly connected"
                .to_string()
        }
        EasyHdrError::ProcessMonitorError(_) => "Failed to monitor processes.\n\n\
             The application may not function correctly.\n\
             Try restarting the application."
            .to_string(),
        EasyHdrError::ConfigError(_) => "Failed to load or save configuration.\n\n\
             Your settings may not persist.\n\
             Check that you have write permissions to:\n\
             %APPDATA%\\EasyHDR"
            .to_string(),
        #[cfg(windows)]
        EasyHdrError::WindowsApiError(e) => {
            format!(
                "A Windows API error occurred:\n\n{e}\n\n\
                 Please ensure your Windows installation is up to date."
            )
        }
        EasyHdrError::IoError(e) => {
            format!(
                "A file system error occurred:\n\n{e}\n\n\
                 Please check file permissions and disk space."
            )
        }
        EasyHdrError::JsonError(e) => {
            format!(
                "Configuration file is corrupted:\n\n{e}\n\n\
                 The application will use default settings."
            )
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
