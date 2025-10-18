//! Utility modules
//!
//! Provides auto-start management, icon extraction, logging, memory and startup profiling,
//! single instance enforcement, and update checking.

pub mod autostart;
pub mod icon_extractor;
pub mod logging;
pub mod memory_profiler;
pub mod single_instance;
pub mod startup_profiler;
pub mod update_checker;

pub use autostart::AutoStartManager;
pub use icon_extractor::{extract_display_name_from_exe, extract_icon_from_exe};
pub use logging::init_logging;
pub use single_instance::SingleInstanceGuard;
pub use update_checker::{UpdateCheckResult, UpdateChecker};
