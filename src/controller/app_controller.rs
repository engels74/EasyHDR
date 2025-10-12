//! Application controller implementation
//!
//! This module implements the main application logic controller that
//! coordinates between process monitoring and HDR control.

use crate::config::AppConfig;
use crate::error::Result;
use crate::hdr::HdrController;
use crate::monitor::ProcessEvent;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

/// Application state for GUI updates
#[derive(Debug, Clone)]
pub struct AppState {
    /// Whether HDR is currently enabled
    pub hdr_enabled: bool,
    /// List of currently active applications
    pub active_apps: Vec<String>,
    /// Last event message
    pub last_event: String,
}

/// Application logic controller
pub struct AppController {
    /// Application configuration
    config: Arc<Mutex<AppConfig>>,
    /// HDR controller
    hdr_controller: HdrController,
    /// Count of active monitored processes
    active_process_count: AtomicUsize,
    /// Current HDR state
    current_hdr_state: AtomicBool,
    /// Event receiver from process monitor
    event_receiver: mpsc::Receiver<ProcessEvent>,
    /// State sender to GUI
    gui_state_sender: mpsc::Sender<AppState>,
    /// Last toggle time for debouncing
    last_toggle_time: Arc<Mutex<Instant>>,
}

impl AppController {
    /// Create a new application controller
    pub fn new(
        config: AppConfig,
        event_receiver: mpsc::Receiver<ProcessEvent>,
        gui_state_sender: mpsc::Sender<AppState>,
    ) -> Result<Self> {
        let hdr_controller = HdrController::new()?;
        
        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            hdr_controller,
            active_process_count: AtomicUsize::new(0),
            current_hdr_state: AtomicBool::new(false),
            event_receiver,
            gui_state_sender,
            last_toggle_time: Arc::new(Mutex::new(Instant::now())),
        })
    }

    /// Run the main event loop
    pub fn run(&mut self) {
        // TODO: Implement main event loop
        // This will be implemented in task 6
    }

    /// Handle a process event
    ///
    /// Processes ProcessEvent::Started and ProcessEvent::Stopped events from the process monitor.
    /// Implements the core logic for automatic HDR toggling based on monitored applications.
    ///
    /// # Arguments
    ///
    /// * `event` - The process event to handle (Started or Stopped)
    ///
    /// # Requirements
    ///
    /// - Requirement 4.1: Enable HDR when any monitored and enabled application transitions to RUNNING
    /// - Requirement 4.2: Disable HDR when the last monitored application transitions to NOT_RUNNING
    /// - Requirement 4.3: Maintain a counter of active monitored processes
    /// - Requirement 4.4: Prevent redundant toggle operations when HDR is already in desired state
    /// - Requirement 4.8: Debounce rapid state changes by waiting 500ms before toggling back
    fn handle_process_event(&mut self, event: ProcessEvent) {
        use tracing::{debug, error, info};

        match event {
            ProcessEvent::Started(process_name) => {
                debug!("Process started event: {}", process_name);

                // Check if this process is in our monitored list and enabled
                let config = self.config.lock();
                let is_monitored = config.monitored_apps.iter()
                    .any(|app| app.enabled && app.process_name.eq_ignore_ascii_case(&process_name));
                drop(config); // Release lock early

                if is_monitored {
                    info!("Monitored application started: {}", process_name);

                    // Increment active process count
                    let prev_count = self.active_process_count.fetch_add(1, Ordering::SeqCst);
                    debug!("Active process count: {} -> {}", prev_count, prev_count + 1);

                    // If this is the first active process and HDR is off, enable HDR
                    if prev_count == 0 && !self.current_hdr_state.load(Ordering::SeqCst) {
                        info!("First monitored application started, enabling HDR");
                        if let Err(e) = self.toggle_hdr(true) {
                            error!("Failed to enable HDR: {}", e);
                        }
                    } else {
                        debug!("HDR already enabled or other processes running, skipping toggle");
                    }

                    // Send state update to GUI
                    self.send_state_update();
                }
            }

            ProcessEvent::Stopped(process_name) => {
                debug!("Process stopped event: {}", process_name);

                // Check if this process is in our monitored list and enabled
                let config = self.config.lock();
                let is_monitored = config.monitored_apps.iter()
                    .any(|app| app.enabled && app.process_name.eq_ignore_ascii_case(&process_name));
                drop(config); // Release lock early

                if is_monitored {
                    info!("Monitored application stopped: {}", process_name);

                    // Decrement active process count
                    let prev_count = self.active_process_count.fetch_sub(1, Ordering::SeqCst);
                    debug!("Active process count: {} -> {}", prev_count, prev_count.saturating_sub(1));

                    // Debounce: wait 500ms before disabling to handle quick restarts
                    let last_toggle = *self.last_toggle_time.lock();
                    if last_toggle.elapsed() < std::time::Duration::from_millis(500) {
                        debug!("Debouncing: last toggle was less than 500ms ago, skipping HDR disable");
                        return;
                    }

                    // If this was the last active process and HDR is on, disable HDR
                    if prev_count == 1 && self.current_hdr_state.load(Ordering::SeqCst) {
                        info!("Last monitored application stopped, disabling HDR");
                        if let Err(e) = self.toggle_hdr(false) {
                            error!("Failed to disable HDR: {}", e);
                        }
                    } else {
                        debug!("Other processes still running or HDR already off, skipping toggle");
                    }

                    // Send state update to GUI
                    self.send_state_update();
                }
            }
        }
    }

    /// Toggle HDR state
    ///
    /// Calls the HDR controller to enable or disable HDR globally on all displays.
    /// Updates the current HDR state and last toggle time for debouncing.
    ///
    /// # Arguments
    ///
    /// * `enable` - True to enable HDR, false to disable
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, or an error if HDR control fails.
    ///
    /// # Requirements
    ///
    /// - Requirement 4.5: Log HDR state change with timestamp
    /// - Requirement 4.6: Handle errors gracefully and continue monitoring
    fn toggle_hdr(&mut self, enable: bool) -> Result<()> {
        use tracing::{info, warn};

        info!("Toggling HDR: {}", if enable { "ON" } else { "OFF" });

        // Call HDR controller to set HDR state globally
        let results = self.hdr_controller.set_hdr_global(enable)?;

        // Log results for each display
        for (target, result) in results {
            match result {
                Ok(()) => {
                    info!(
                        "HDR {} for display (adapter={:#x}:{:#x}, target={})",
                        if enable { "enabled" } else { "disabled" },
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to toggle HDR for display (adapter={:#x}:{:#x}, target={}): {}",
                        target.adapter_id.LowPart,
                        target.adapter_id.HighPart,
                        target.target_id,
                        e
                    );
                }
            }
        }

        // Update current HDR state
        self.current_hdr_state.store(enable, Ordering::SeqCst);

        // Update last toggle time for debouncing
        *self.last_toggle_time.lock() = Instant::now();

        Ok(())
    }

    /// Send state update to GUI
    ///
    /// Sends the current application state to the GUI via the state sender channel.
    /// This includes HDR enabled state, active applications, and last event.
    fn send_state_update(&self) {
        use tracing::warn;

        let config = self.config.lock();
        let active_apps: Vec<String> = config.monitored_apps.iter()
            .filter(|app| app.enabled)
            .map(|app| app.display_name.clone())
            .collect();
        drop(config);

        let state = AppState {
            hdr_enabled: self.current_hdr_state.load(Ordering::SeqCst),
            active_apps,
            last_event: format!("Active processes: {}", self.active_process_count.load(Ordering::SeqCst)),
        };

        if let Err(e) = self.gui_state_sender.send(state) {
            warn!("Failed to send state update to GUI: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, MonitoredApp};
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn test_app_controller_creation() {
        let config = AppConfig::default();
        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();

        let controller = AppController::new(config, event_rx, state_tx);
        assert!(controller.is_ok());
    }

    #[test]
    fn test_handle_process_started_increments_count() {
        // Create a config with one monitored app
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        });

        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, state_rx) = mpsc::channel();

        let mut controller = AppController::new(config, event_rx, state_tx).unwrap();

        // Initial count should be 0
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);

        // Handle a started event for the monitored app
        controller.handle_process_event(ProcessEvent::Started("app".to_string()));

        // Count should be incremented to 1
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);

        // Should have sent a state update
        let state = state_rx.try_recv().unwrap();
        assert_eq!(state.hdr_enabled, true);
    }

    #[test]
    fn test_handle_process_started_case_insensitive() {
        // Create a config with one monitored app (lowercase)
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        });

        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();

        let mut controller = AppController::new(config, event_rx, state_tx).unwrap();

        // Handle a started event with different case
        controller.handle_process_event(ProcessEvent::Started("APP".to_string()));

        // Count should be incremented (case-insensitive match)
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_handle_process_started_disabled_app_ignored() {
        // Create a config with one disabled app
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: false, // Disabled
            icon_data: None,
        });

        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();

        let mut controller = AppController::new(config, event_rx, state_tx).unwrap();

        // Handle a started event for the disabled app
        controller.handle_process_event(ProcessEvent::Started("app".to_string()));

        // Count should remain 0 (disabled apps are ignored)
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_handle_process_stopped_decrements_count() {
        // Create a config with one monitored app
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        });

        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, state_rx) = mpsc::channel();

        let mut controller = AppController::new(config, event_rx, state_tx).unwrap();

        // Start the app first
        controller.handle_process_event(ProcessEvent::Started("app".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);

        // Clear the state update from start
        let _ = state_rx.try_recv();

        // Wait for debounce period to pass
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop the app
        controller.handle_process_event(ProcessEvent::Stopped("app".to_string()));

        // Count should be decremented to 0
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);

        // Should have sent a state update
        let state = state_rx.try_recv().unwrap();
        assert_eq!(state.hdr_enabled, false);
    }

    #[test]
    fn test_handle_multiple_processes() {
        // Create a config with two monitored apps
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::new_v4(),
            display_name: "App 1".to_string(),
            exe_path: PathBuf::from("C:\\test\\app1.exe"),
            process_name: "app1".to_string(),
            enabled: true,
            icon_data: None,
        });
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::new_v4(),
            display_name: "App 2".to_string(),
            exe_path: PathBuf::from("C:\\test\\app2.exe"),
            process_name: "app2".to_string(),
            enabled: true,
            icon_data: None,
        });

        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();

        let mut controller = AppController::new(config, event_rx, state_tx).unwrap();

        // Start first app
        controller.handle_process_event(ProcessEvent::Started("app1".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert_eq!(controller.current_hdr_state.load(Ordering::SeqCst), true);

        // Start second app
        controller.handle_process_event(ProcessEvent::Started("app2".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 2);
        assert_eq!(controller.current_hdr_state.load(Ordering::SeqCst), true);

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop first app - HDR should remain on
        controller.handle_process_event(ProcessEvent::Stopped("app1".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert_eq!(controller.current_hdr_state.load(Ordering::SeqCst), true);

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop second app - HDR should turn off
        controller.handle_process_event(ProcessEvent::Stopped("app2".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert_eq!(controller.current_hdr_state.load(Ordering::SeqCst), false);
    }
}

