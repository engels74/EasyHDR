//! GUI module
//!
//! This module provides the Slint-based graphical user interface
//! and system tray integration.

pub mod gui_controller;
pub mod tray;

pub use gui_controller::GuiController;
pub use tray::TrayIcon;

