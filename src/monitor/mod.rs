//! Process monitoring module
//!
//! Provides background monitoring of running processes to detect when configured
//! applications start or stop, enabling automatic HDR toggling.

pub mod hdr_state_monitor;
pub mod process_monitor;

pub use hdr_state_monitor::{HdrStateEvent, HdrStateMonitor};
pub use process_monitor::{AppIdentifier, ProcessEvent, ProcessMonitor, WatchState};
