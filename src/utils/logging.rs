//! Logging system initialization
//!
//! Sets up tracing-based logging with file output to %APPDATA%\EasyHDR\app.log
//! and automatic rotation on application startup keeping 10 historical files.

use crate::error::Result;
use std::path::PathBuf;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt};

/// Maximum number of historical log files to keep (app.log.1 through app.log.9)
const MAX_LOG_FILES: u8 = 9;

/// Initialize the logging system
///
/// Log level defaults to INFO but can be configured via `RUST_LOG` environment variable.
/// Rotates existing logs on startup to maintain a history of the last 10 sessions.
pub fn init_logging() -> Result<()> {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let log_dir = PathBuf::from(appdata).join("EasyHDR");
    std::fs::create_dir_all(&log_dir)?;

    // Rotate existing log files on startup
    let log_path = log_dir.join("app.log");
    rotate_logs_on_startup(&log_path)?;

    // Create rolling file appender
    // Note: tracing_appender's RollingFileAppender doesn't support startup-based rotation
    // with our desired file retention policy, so we handle rotation manually
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::NEVER) // We handle rotation manually on startup
        .filename_prefix("app")
        .filename_suffix("log")
        .build(log_dir)
        .map_err(|e| {
            // Preserve error chain by wrapping the source error
            crate::error::EasyHdrError::ConfigError(Box::new(e))
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
        .map_err(|e| crate::error::EasyHdrError::ConfigError(Box::new(e)))?;

    tracing::info!("EasyHDR v{} started", env!("CARGO_PKG_VERSION"));

    Ok(())
}

/// Rotate log files on application startup
///
/// Rotates existing logs to maintain a history of the last 10 application sessions:
/// - app.log.9 is deleted (oldest log)
/// - app.log.8 -> app.log.9
/// - app.log.7 -> app.log.8
/// - ... (and so on)
/// - app.log.1 -> app.log.2
/// - app.log -> app.log.1
/// - A fresh app.log will be created by the logger
///
/// This function is called unconditionally on every application startup,
/// regardless of log file size, ensuring each session's logs are preserved separately.
fn rotate_logs_on_startup(log_path: &PathBuf) -> Result<()> {
    // If the current log doesn't exist, nothing to rotate
    if !log_path.exists() {
        return Ok(());
    }

    // Get the parent directory for constructing numbered log paths
    let log_dir = log_path.parent().ok_or_else(|| {
        crate::error::EasyHdrError::ConfigError(crate::error::StringError::new("Invalid log path"))
    })?;

    let log_name = log_path
        .file_name()
        .ok_or_else(|| {
            crate::error::EasyHdrError::ConfigError(crate::error::StringError::new(
                "Invalid log filename",
            ))
        })?
        .to_string_lossy();

    // Delete the oldest log file (app.log.9) if it exists
    let oldest_log = log_dir.join(format!("{log_name}.{MAX_LOG_FILES}"));
    if oldest_log.exists() {
        std::fs::remove_file(&oldest_log)?;
    }

    // Rotate log files from 8 down to 1
    // app.log.8 -> app.log.9, app.log.7 -> app.log.8, ..., app.log.1 -> app.log.2
    for i in (1..MAX_LOG_FILES).rev() {
        let current_log = log_dir.join(format!("{log_name}.{i}"));
        let next_log = log_dir.join(format!("{log_name}.{}", i + 1));

        if current_log.exists() {
            std::fs::rename(&current_log, &next_log)?;
        }
    }

    // Rotate the current log file (app.log -> app.log.1)
    let log_1 = log_dir.join(format!("{log_name}.1"));
    std::fs::rename(log_path, &log_1)?;

    tracing::info!("Log rotation completed on startup");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    /// Helper function to create a test log file with specific content
    fn create_test_log(path: &PathBuf, content: &str) {
        let mut file = fs::File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_rotate_logs_on_startup_basic() {
        // Create a temporary directory for testing
        let temp_dir = std::env::temp_dir().join("easyhdr_test_basic_rotation");
        fs::create_dir_all(&temp_dir).unwrap();

        let log_path = temp_dir.join("app.log");

        // Create initial log file
        create_test_log(&log_path, "Session 1 log content");

        // Perform rotation
        rotate_logs_on_startup(&log_path).unwrap();

        // Verify that app.log was rotated to app.log.1
        let log_1 = temp_dir.join("app.log.1");
        assert!(log_1.exists(), "app.log.1 should exist after rotation");
        assert!(
            !log_path.exists(),
            "app.log should not exist after rotation (will be created fresh by logger)"
        );

        // Verify content was preserved
        let content = fs::read_to_string(&log_1).unwrap();
        assert_eq!(content, "Session 1 log content");

        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_rotate_logs_on_startup_multiple_rotations() {
        // Create a temporary directory for testing
        let temp_dir = std::env::temp_dir().join("easyhdr_test_multiple_rotations");
        fs::create_dir_all(&temp_dir).unwrap();

        let log_path = temp_dir.join("app.log");

        // Simulate multiple application startups
        for i in 1..=5 {
            create_test_log(&log_path, &format!("Session {i} log content"));
            rotate_logs_on_startup(&log_path).unwrap();
        }

        // Verify rotation chain
        for i in 1..=5 {
            let log_i = temp_dir.join(format!("app.log.{i}"));
            assert!(log_i.exists(), "app.log.{i} should exist");

            let content = fs::read_to_string(&log_i).unwrap();
            let expected_session = 6 - i; // Most recent is in .1, oldest in .5
            assert_eq!(
                content,
                format!("Session {expected_session} log content"),
                "app.log.{i} should contain Session {expected_session}"
            );
        }

        // app.log should not exist (gets created fresh by logger)
        assert!(
            !log_path.exists(),
            "app.log should not exist after rotation"
        );

        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_rotate_logs_on_startup_respects_max_files() {
        // Create a temporary directory for testing
        let temp_dir = std::env::temp_dir().join("easyhdr_test_max_files");
        fs::create_dir_all(&temp_dir).unwrap();

        let log_path = temp_dir.join("app.log");

        // Simulate more than MAX_LOG_FILES startups
        for i in 1..=12 {
            create_test_log(&log_path, &format!("Session {i} log content"));
            rotate_logs_on_startup(&log_path).unwrap();
        }

        // Verify we only have MAX_LOG_FILES historical logs
        for i in 1..=MAX_LOG_FILES {
            let log_i = temp_dir.join(format!("app.log.{i}"));
            assert!(
                log_i.exists(),
                "app.log.{i} should exist (within MAX_LOG_FILES)"
            );
        }

        // Verify files beyond MAX_LOG_FILES don't exist
        let log_10 = temp_dir.join("app.log.10");
        let log_11 = temp_dir.join("app.log.11");
        let log_12 = temp_dir.join("app.log.12");
        assert!(
            !log_10.exists(),
            "app.log.10 should not exist (beyond MAX_LOG_FILES)"
        );
        assert!(
            !log_11.exists(),
            "app.log.11 should not exist (beyond MAX_LOG_FILES)"
        );
        assert!(
            !log_12.exists(),
            "app.log.12 should not exist (beyond MAX_LOG_FILES)"
        );

        // Verify the oldest log (app.log.9) contains the 4th session
        // (Sessions 1, 2, 3 were deleted, session 4 is now in app.log.9)
        let log_9 = temp_dir.join("app.log.9");
        let content = fs::read_to_string(&log_9).unwrap();
        assert_eq!(
            content, "Session 4 log content",
            "app.log.9 should contain the 4th session (oldest retained)"
        );

        // Verify the most recent log (app.log.1) contains the 12th session
        let log_1 = temp_dir.join("app.log.1");
        let content = fs::read_to_string(&log_1).unwrap();
        assert_eq!(
            content, "Session 12 log content",
            "app.log.1 should contain the most recent session"
        );

        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_rotate_logs_on_startup_no_existing_log() {
        // Create a temporary directory for testing
        let temp_dir = std::env::temp_dir().join("easyhdr_test_no_existing_log");
        fs::create_dir_all(&temp_dir).unwrap();

        let log_path = temp_dir.join("app.log");

        // Should not fail when log doesn't exist
        let result = rotate_logs_on_startup(&log_path);
        assert!(
            result.is_ok(),
            "Rotation should succeed when log doesn't exist"
        );

        // No files should be created
        assert!(!log_path.exists(), "app.log should not exist");
        let log_1 = temp_dir.join("app.log.1");
        assert!(!log_1.exists(), "app.log.1 should not exist");

        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_rotate_logs_on_startup_partial_history() {
        // Create a temporary directory for testing
        let temp_dir = std::env::temp_dir().join("easyhdr_test_partial_history");
        fs::create_dir_all(&temp_dir).unwrap();

        let log_path = temp_dir.join("app.log");

        // Create current log and only a few historical logs (simulating partial history)
        create_test_log(&log_path, "Current session");
        create_test_log(&temp_dir.join("app.log.1"), "Previous session");
        create_test_log(&temp_dir.join("app.log.5"), "Very old session");

        // Perform rotation
        rotate_logs_on_startup(&log_path).unwrap();

        // Verify rotation worked with gaps
        let log_1 = temp_dir.join("app.log.1");
        let log_2 = temp_dir.join("app.log.2");
        let log_6 = temp_dir.join("app.log.6");

        assert!(
            log_1.exists(),
            "app.log.1 should exist (rotated from app.log)"
        );
        assert!(
            log_2.exists(),
            "app.log.2 should exist (rotated from app.log.1)"
        );
        assert!(
            log_6.exists(),
            "app.log.6 should exist (rotated from app.log.5)"
        );

        let content_1 = fs::read_to_string(&log_1).unwrap();
        let content_2 = fs::read_to_string(&log_2).unwrap();
        let content_6 = fs::read_to_string(&log_6).unwrap();

        assert_eq!(content_1, "Current session");
        assert_eq!(content_2, "Previous session");
        assert_eq!(content_6, "Very old session");

        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
