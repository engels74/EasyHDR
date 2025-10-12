//! Process monitoring module
//!
//! This module provides functionality to monitor running Windows processes
//! and detect when configured applications start or stop.

pub mod process_monitor;

pub use process_monitor::{ProcessEvent, ProcessMonitor};

