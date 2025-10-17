//! Logging system initialization
//!
//! Sets up tracing-based logging with file output to %APPDATA%\EasyHDR\app.log
//! and automatic rotation at 5MB keeping 3 historical files.

use crate::error::Result;
use std::path::PathBuf;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt};

/// Maximum log file size in bytes (5MB)
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;

/// Initialize the logging system
///
/// Log level defaults to INFO but can be configured via `RUST_LOG` environment variable.
pub fn init_logging() -> Result<()> {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let log_dir = PathBuf::from(appdata).join("EasyHDR");
    std::fs::create_dir_all(&log_dir)?;

    // Check and rotate existing log file if it exceeds size limit
    let log_path = log_dir.join("app.log");
    if log_path.exists() {
        check_and_rotate_log(&log_path)?;
    }

    // Create rolling file appender
    // Note: tracing_appender's RollingFileAppender doesn't support size-based rotation
    // with max_log_files in the way we need, so we'll use manual rotation
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::NEVER) // We handle rotation manually
        .filename_prefix("app")
        .filename_suffix("log")
        .build(log_dir)
        .map_err(|e| {
            crate::error::EasyHdrError::ConfigError(format!("Failed to create log appender: {e}"))
        })?;

    // Build the subscriber with file output
    let subscriber = fmt()
        .with_writer(file_appender)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_ansi(false) // Disable ANSI colors for file output
        .with_target(true) // Include target module
        .with_thread_ids(true) // Include thread IDs
        .with_file(true) // Include file names
        .with_line_number(true) // Include line numbers
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| crate::error::EasyHdrError::ConfigError(e.to_string()))?;

    tracing::info!("EasyHDR v{} started", env!("CARGO_PKG_VERSION"));

    Ok(())
}

/// Check log file size and rotate if necessary
///
/// Rotates logs: app.log.2 deleted, app.log.1 -> app.log.2, app.log -> app.log.1
fn check_and_rotate_log(log_path: &PathBuf) -> Result<()> {
    let metadata = std::fs::metadata(log_path)?;

    if metadata.len() > MAX_LOG_SIZE {
        tracing::debug!(
            "Log file size {} exceeds limit {}, rotating logs",
            metadata.len(),
            MAX_LOG_SIZE
        );

        // Rotate existing log files
        // Delete the oldest log file (app.log.2)
        let oldest_log = log_path.with_extension("log.2");
        if oldest_log.exists() {
            std::fs::remove_file(&oldest_log)?;
        }

        // Rotate app.log.1 -> app.log.2
        let log_1 = log_path.with_extension("log.1");
        if log_1.exists() {
            std::fs::rename(&log_1, &oldest_log)?;
        }

        // Rotate app.log -> app.log.1
        std::fs::rename(log_path, &log_1)?;

        tracing::info!("Log rotation completed");
    }

    Ok(())
}

/// Log rotator for periodic rotation checks during application runtime
pub struct LogRotator {
    log_path: PathBuf,
    max_size: u64,
}

impl LogRotator {
    /// Create a new log rotator
    pub fn new(log_path: PathBuf, max_size: u64) -> Self {
        Self { log_path, max_size }
    }

    /// Check if rotation is needed and perform it
    pub fn check_and_rotate(&self) -> Result<()> {
        if !self.log_path.exists() {
            return Ok(());
        }

        let metadata = std::fs::metadata(&self.log_path)?;

        if metadata.len() > self.max_size {
            check_and_rotate_log(&self.log_path)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Test uses known small constant values that are well within u32 range"
    )]
    fn test_log_rotation() {
        // Create a temporary directory for testing
        let temp_dir = std::env::temp_dir().join("easyhdr_test_logs");
        fs::create_dir_all(&temp_dir).unwrap();

        let log_path = temp_dir.join("app.log");

        // Create a log file larger than MAX_LOG_SIZE
        let mut file = fs::File::create(&log_path).unwrap();
        let large_content = vec![b'x'; (MAX_LOG_SIZE + 1000) as usize];
        file.write_all(&large_content).unwrap();
        drop(file);

        // Perform rotation
        check_and_rotate_log(&log_path).unwrap();

        // Verify that app.log was rotated to app.log.1
        let log_1 = temp_dir.join("app.log.1");
        assert!(log_1.exists(), "app.log.1 should exist after rotation");
        assert!(
            !log_path.exists() || fs::metadata(&log_path).unwrap().len() == 0,
            "app.log should be empty or not exist after rotation"
        );

        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Test uses known small constant values that are well within u32 range"
    )]
    fn test_log_rotator() {
        let temp_dir = std::env::temp_dir().join("easyhdr_test_rotator");
        fs::create_dir_all(&temp_dir).unwrap();

        let log_path = temp_dir.join("app.log");
        let rotator = LogRotator::new(log_path.clone(), MAX_LOG_SIZE);

        // Create a small log file
        let mut file = fs::File::create(&log_path).unwrap();
        file.write_all(b"small log").unwrap();
        drop(file);

        // Should not rotate
        rotator.check_and_rotate().unwrap();
        assert!(log_path.exists(), "app.log should still exist");

        // Create a large log file
        let mut file = fs::File::create(&log_path).unwrap();
        let large_content = vec![b'x'; (MAX_LOG_SIZE + 1000) as usize];
        file.write_all(&large_content).unwrap();
        drop(file);

        // Should rotate
        rotator.check_and_rotate().unwrap();
        let log_1 = temp_dir.join("app.log.1");
        assert!(log_1.exists(), "app.log.1 should exist after rotation");

        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
