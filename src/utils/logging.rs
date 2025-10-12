//! Logging system initialization
//!
//! This module sets up the tracing-based logging system with file output
//! and log rotation.

use crate::error::Result;
use std::path::PathBuf;
use tracing_subscriber::{fmt, EnvFilter};

/// Initialize the logging system
///
/// Sets up logging to %APPDATA%\EasyHDR\app.log with rotation
pub fn init_logging() -> Result<()> {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let log_dir = PathBuf::from(appdata).join("EasyHDR");
    std::fs::create_dir_all(&log_dir)?;
    
    // For now, use simple stdout logging
    // TODO: Implement file-based logging with rotation in task 7
    let subscriber = fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .with_target(true)
        .with_thread_ids(true)
        .finish();
    
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| crate::error::EasyHdrError::ConfigError(e.to_string()))?;
    
    tracing::info!("EasyHDR v{} started", env!("CARGO_PKG_VERSION"));
    
    Ok(())
}

