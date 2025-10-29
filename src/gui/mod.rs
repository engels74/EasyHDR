//! GUI module
//!
//! Provides the Slint-based graphical user interface and system tray integration.
//! Includes main window, settings panel, and state synchronization with the application controller.

pub mod gui_controller;
pub mod tray;

pub use gui_controller::GuiController;
#[expect(unused_imports)]
pub use tray::TrayIcon;
