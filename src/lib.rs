//! `EasyHDR` - Automatic HDR management for Windows
//!
//! This library provides functionality to automatically enable and disable HDR
//! on Windows displays based on configured applications.
//!
//! # Overview
//!
//! `EasyHDR` is a Windows utility that automatically toggles HDR (High Dynamic Range)
//! on your displays when you launch HDR-capable games or applications. It runs in
//! the background, monitoring for configured applications and enabling HDR when they
//! start, then disabling HDR when they close.
//!
//! # Architecture
//!
//! The application uses a multi-threaded event-driven architecture:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Main Thread                            │
//! │  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
//! │  │ Slint Event  │───▶│     GUI      │───▶│  TrayIcon    │  │
//! │  │     Loop     │    │  Controller  │    │              │  │
//! │  └──────────────┘    └──────┬───────┘    └──────────────┘  │
//! │                             │                               │
//! │                             │ Callbacks                     │
//! │                             ▼                               │
//! │                      ┌──────────────┐                       │
//! │                      │     App      │                       │
//! │                      │  Controller  │◀──── State Updates    │
//! │                      └──────┬───────┘                       │
//! └─────────────────────────────┼─────────────────────────────┘
//!                               │
//!                               │ Process Events
//!                               │
//! ┌─────────────────────────────┼─────────────────────────────┐
//! │                  Background Thread                         │
//! │                      ┌──────▼───────┐                      │
//! │                      │   Process    │                      │
//! │                      │   Monitor    │                      │
//! │                      └──────────────┘                      │
//! └────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Components
//!
//! - **`ProcessMonitor`**: Polls running processes at configurable intervals (default 1s)
//! - **`AppController`**: Coordinates HDR control based on process events
//! - **`HdrController`**: Interfaces with Windows Display Configuration API
//! - **`GuiController`**: Bridges Slint UI with application logic
//! - **`ConfigManager`**: Handles persistent configuration storage
//!
//! # Example Usage
//!
//! ```no_run
//! use easyhdr::{
//!     config::{ConfigManager, MonitoredApp},
//!     controller::AppController,
//!     monitor::ProcessMonitor,
//!     hdr::HdrController,
//! };
//! use std::sync::{mpsc, Arc};
//! use std::collections::HashSet;
//! use parking_lot::Mutex;
//! use std::path::PathBuf;
//! use std::time::Duration;
//!
//! // Load configuration
//! let mut config = ConfigManager::load()?;
//!
//! // Add a monitored application
//! let game_path = PathBuf::from(r"C:\Games\Cyberpunk2077\bin\x64\Cyberpunk2077.exe");
//! let app = MonitoredApp::from_exe_path(game_path)?;
//! config.monitored_apps.push(app);
//! ConfigManager::save(&config)?;
//!
//! // Create communication channels
//! let (event_tx, event_rx) = mpsc::sync_channel(32);
//! let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
//! let (state_tx, state_rx) = mpsc::sync_channel(32);
//!
//! // Create shared watch list
//! let watch_list = Arc::new(Mutex::new(HashSet::new()));
//!
//! // Create process monitor
//! let mut monitor = ProcessMonitor::new(
//!     Duration::from_millis(config.preferences.monitoring_interval_ms),
//!     event_tx,
//! );
//!
//! // Update watch list with monitored apps
//! let mut watch_vec = Vec::new();
//! for app in &config.monitored_apps {
//!     if app.enabled {
//!         watch_vec.push(app.process_name.clone());
//!     }
//! }
//! monitor.update_watch_list(watch_vec);
//!
//! // Start monitoring
//! monitor.start();
//!
//! // Create application controller
//! let mut controller = AppController::new(
//!     config,
//!     event_rx,
//!     hdr_state_rx,
//!     state_tx,
//!     watch_list,
//! )?;
//!
//! // Run event loop (blocks until channel closes)
//! controller.run();
//! # Ok::<(), easyhdr::error::EasyHdrError>(())
//! ```
//!
//! # Modules
//!
//! - `config`: Configuration management and persistence
//! - `controller`: Application logic controller
//! - `error`: Error types and handling
//! - `hdr`: HDR control and Windows version detection
//! - `monitor`: Process monitoring
//! - `utils`: Utility functions
//!
//! # Requirements
//!
//! - Windows 10 21H2+ (Build 19044+) or Windows 11
//! - HDR-capable display
//! - Updated display drivers
//!
//! # Performance Targets
//!
//! - CPU usage: <1% during monitoring
//! - Memory usage: <50MB RAM
//! - Startup time: <200ms to GUI display
//! - Process detection: Within 1-2 seconds

// Module declarations
pub mod config;
pub mod controller;
pub mod error;
pub mod hdr;
pub mod monitor;
pub mod utils;

// Re-export commonly used types
pub use error::{EasyHdrError, Result};
