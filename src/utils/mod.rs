//! Utility modules
//!
//! This module contains utility functions for icon extraction,
//! auto-start management, and logging.

pub mod autostart;
pub mod icon_extractor;
pub mod logging;

pub use autostart::AutoStartManager;
pub use icon_extractor::{extract_display_name_from_exe, extract_icon_from_exe};
pub use logging::init_logging;

