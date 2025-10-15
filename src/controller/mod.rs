//! Application logic controller module
//!
//! This module coordinates between process monitoring, HDR control, and GUI,
//! implementing the core application logic.
//!
//! # Overview
//!
//! The application controller is the central coordinator that:
//! - **Receives process events** from the ProcessMonitor
//! - **Manages HDR state** based on active monitored processes
//! - **Implements debouncing** to prevent rapid HDR toggling
//! - **Sends state updates** to the GUI for display
//! - **Handles configuration changes** from the GUI
//!
//! # Architecture
//!
//! - `AppController`: Main controller coordinating process monitoring and HDR control
//! - `AppState`: State snapshot sent to GUI for display updates
//! - **Event-driven design**: Reacts to process events from monitor thread
//! - **Thread-safe**: Uses Arc<Mutex<>> for shared state
//!
//! # Event Flow
//!
//! ```text
//! ProcessMonitor → ProcessEvent → AppController → HDR Control
//!                                       ↓
//!                                   AppState → GUI
//! ```
//!
//! # HDR Toggle Logic
//!
//! The controller maintains a counter of active monitored processes:
//!
//! 1. **Process Started Event**:
//!    - Increment active process counter
//!    - If counter goes from 0 → 1: Enable HDR
//!    - If counter > 1: HDR already enabled, no action
//!
//! 2. **Process Stopped Event**:
//!    - Decrement active process counter
//!    - If counter goes from 1 → 0: Disable HDR (with debouncing)
//!    - If counter > 0: Other processes still active, keep HDR enabled
//!
//! # Debouncing
//!
//! To prevent rapid HDR toggling when processes start/stop quickly:
//! - Tracks last toggle time
//! - Waits 500ms before toggling HDR back to previous state
//! - Prevents flickering and improves user experience
//!
//! # Example Usage
//!
//! ```no_run
//! use easyhdr::controller::AppController;
//! use easyhdr::config::ConfigManager;
//! use std::sync::{mpsc, Arc};
//! use std::collections::HashSet;
//! use parking_lot::Mutex;
//!
//! // Load configuration
//! let config = ConfigManager::load()?;
//!
//! // Create channels
//! let (event_tx, event_rx) = mpsc::channel();
//! let (_hdr_state_tx, hdr_state_rx) = mpsc::channel();
//! let (state_tx, state_rx) = mpsc::channel();
//!
//! // Create shared watch list
//! let watch_list = Arc::new(Mutex::new(HashSet::new()));
//!
//! // Create controller
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
//! # Requirements
//!
//! - Requirement 4.1: Enable HDR when any monitored application transitions to RUNNING
//! - Requirement 4.2: Disable HDR when last monitored application transitions to NOT_RUNNING
//! - Requirement 4.3: Maintain counter of active monitored processes
//! - Requirement 4.4: Prevent redundant toggle operations
//! - Requirement 4.8: Debounce rapid state changes (500ms)

pub mod app_controller;

pub use app_controller::{AppController, AppState};
