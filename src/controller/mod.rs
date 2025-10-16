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
//! # HDR Toggle Behavior
//!
//! The controller enables HDR when any monitored application starts and disables HDR
//! when the last monitored application stops. It maintains a counter of active processes
//! and prevents redundant toggle operations. A 500ms debounce prevents rapid toggling
//! during app restarts.

pub mod app_controller;

pub use app_controller::{AppController, AppState};
