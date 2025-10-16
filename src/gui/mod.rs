//! GUI module
//!
//! This module provides the Slint-based graphical user interface
//! and system tray integration.
//!
//! # Overview
//!
//! The GUI system provides:
//! - **Main window** with application list, settings, and status display
//! - **System tray icon** with context menu and notifications
//! - **File picker** for adding applications
//! - **Settings panel** for user preferences
//! - **State synchronization** with the application controller
//!
//! # Architecture
//!
//! - `GuiController`: Bridge between Slint UI and application logic
//! - `TrayIcon`: System tray icon with context menu
//! - **MainWindow**: Slint component defined in ui/main.slint
//! - **Callbacks**: GUI → Controller communication
//! - **State updates**: Controller → GUI communication via mpsc channel
//!
//! # Threading Model
//!
//! ```text
//! Main Thread (Slint Event Loop)
//!   ├─ GuiController
//!   ├─ MainWindow (Slint)
//!   └─ TrayIcon
//!
//! Background Thread
//!   └─ AppController
//!       └─ ProcessMonitor
//! ```
//!
//! # Communication Patterns
//!
//! ## GUI → Controller (Callbacks)
//!
//! User interactions trigger callbacks that modify shared state:
//! - Add application → Update config, update watch list
//! - Remove application → Update config, update watch list
//! - Toggle enabled → Update config, update watch list
//! - Change settings → Update config, apply changes
//!
//! ## Controller → GUI (State Updates)
//!
//! Controller sends AppState updates via mpsc channel:
//! - HDR state changes → Update status indicator
//! - Process events → Update active apps list
//! - Configuration changes → Refresh UI
//!
//! # Example Usage
//!
//! ```no_run
//! use easyhdr::gui::GuiController;
//! use easyhdr::controller::AppController;
//! use std::sync::{mpsc, Arc};
//! use parking_lot::Mutex;
//!
//! // Create state channel
//! let (state_tx, state_rx) = mpsc::channel();
//!
//! // Create controller (simplified)
//! # let config = easyhdr::config::ConfigManager::load()?;
//! # let (event_tx, event_rx) = mpsc::channel();
//! # let watch_list = Arc::new(Mutex::new(std::collections::HashSet::new()));
//! # let controller_instance = AppController::new(config, event_rx, state_tx.clone(), watch_list)?;
//! let controller = Arc::new(Mutex::new(controller_instance));
//!
//! // Create GUI controller
//! let gui = GuiController::new(controller, state_rx)?;
//!
//! // Run GUI (blocks until window closes)
//! gui.run()?;
//! # Ok::<(), easyhdr::error::EasyHdrError>(())
//! ```

pub mod gui_controller;
pub mod tray;

pub use gui_controller::GuiController;
#[allow(unused_imports)]
pub use tray::TrayIcon;
