//! Process monitoring module
//!
//! This module provides functionality to monitor running Windows processes
//! and detect when configured applications start or stop.
//!
//! # Overview
//!
//! The process monitoring system provides:
//! - **Background polling** of running processes at configurable intervals
//! - **Change detection** using efficient `HashSet` difference operations
//! - **Event notification** when monitored processes start or stop
//! - **Case-insensitive matching** for process names
//! - **Low CPU usage** (<1%) through optimized polling and change detection
//!
//! # Architecture
//!
//! - `ProcessMonitor`: Background thread that polls for process changes
//! - `ProcessEvent`: Events sent when monitored processes start or stop
//! - **Watch list**: Set of process names to monitor (lowercase for case-insensitive matching)
//! - **Event channel**: mpsc channel for sending events to the application controller
//!
//! # Process Enumeration
//!
//! Uses Windows API `CreateToolhelp32Snapshot` to enumerate all running processes:
//! 1. Create snapshot of all processes
//! 2. Iterate through processes using `Process32FirstW` and `Process32NextW`
//! 3. Extract process executable name (e.g., "Cyberpunk2077.exe")
//! 4. Convert to lowercase and remove extension (e.g., "cyberpunk2077")
//! 5. Add to current snapshot `HashSet`
//!
//! # Change Detection Algorithm
//!
//! Uses `HashSet` difference operations for O(n) change detection:
//! - **Started** = `current - previous` (processes in current but not in previous)
//! - **Stopped** = `previous - current` (processes in previous but not in current)
//! - Filter by watch list to only send events for monitored processes
//!
//! # Example Usage
//!
//! ```no_run
//! use easyhdr::monitor::{ProcessMonitor, ProcessEvent};
//! use std::sync::mpsc;
//! use std::time::Duration;
//!
//! // Create event channel
//! let (tx, rx) = mpsc::channel();
//!
//! // Create process monitor with 1-second polling interval
//! let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);
//!
//! // Set up watch list (process names to monitor)
//! let watch_list = vec![
//!     "cyberpunk2077".to_string(),
//!     "witcher3".to_string(),
//! ];
//! monitor.update_watch_list(watch_list);
//!
//! // Start monitoring in background thread
//! monitor.start();
//!
//! // Receive events
//! loop {
//!     match rx.recv() {
//!         Ok(ProcessEvent::Started(name)) => {
//!             println!("Process started: {}", name);
//!         }
//!         Ok(ProcessEvent::Stopped(name)) => {
//!             println!("Process stopped: {}", name);
//!         }
//!         Err(_) => break,
//!     }
//! }
//! ```
//!
//! # Known Limitations
//!
//! - **Process name collisions**: Multiple applications with the same executable name
//!   (e.g., "game.exe") cannot be distinguished.
//! - **UWP applications**: Universal Windows Platform apps may not be detected reliably
//!   as they use different process models.

pub mod hdr_state_monitor;
pub mod process_monitor;

pub use hdr_state_monitor::{HdrStateEvent, HdrStateMonitor};
pub use process_monitor::{ProcessEvent, ProcessMonitor};
