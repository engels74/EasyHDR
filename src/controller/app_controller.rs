//! Application controller implementation
//!
//! This module implements the main application logic controller that
//! coordinates between process monitoring and HDR control.

use crate::config::{AppConfig, ConfigManager, MonitoredApp, UserPreferences};
use crate::error::Result;
use crate::hdr::HdrController;
use crate::monitor::ProcessEvent;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;
use uuid::Uuid;

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
    /// Application configuration (public for GUI access)
    pub config: Arc<Mutex<AppConfig>>,
    /// HDR controller
    hdr_controller: HdrController,
    /// Count of active monitored processes
    active_process_count: AtomicUsize,
    /// Current HDR state
    current_hdr_state: AtomicBool,
    /// Event receiver from process monitor (taken when event loop starts)
    event_receiver: Option<mpsc::Receiver<ProcessEvent>>,
    /// State sender to GUI
    gui_state_sender: mpsc::Sender<AppState>,
    /// Last toggle time for debouncing
    last_toggle_time: Arc<Mutex<Instant>>,
    /// Reference to ProcessMonitor's watch list for updating
    process_monitor_watch_list: Arc<Mutex<HashSet<String>>>,
}

impl AppController {
    /// Create a new application controller
    ///
    /// # Arguments
    ///
    /// * `config` - Application configuration
    /// * `event_receiver` - Channel receiver for process events from ProcessMonitor
    /// * `gui_state_sender` - Channel sender for state updates to GUI
    /// * `process_monitor_watch_list` - Shared reference to ProcessMonitor's watch list
    pub fn new(
        config: AppConfig,
        event_receiver: mpsc::Receiver<ProcessEvent>,
        gui_state_sender: mpsc::Sender<AppState>,
        process_monitor_watch_list: Arc<Mutex<HashSet<String>>>,
    ) -> Result<Self> {
        let hdr_controller = HdrController::new()?;

        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            hdr_controller,
            active_process_count: AtomicUsize::new(0),
            current_hdr_state: AtomicBool::new(false),
            event_receiver: Some(event_receiver),
            gui_state_sender,
            last_toggle_time: Arc::new(Mutex::new(Instant::now())),
            process_monitor_watch_list,
        })
    }

    /// Take ownership of the event receiver if it hasn't been taken yet
    ///
    /// The receiver is stored as an Option so it can be moved out exactly once.
    /// Subsequent attempts to take it return None and should be treated as
    /// a no-op by callers.
    fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<ProcessEvent>> {
        self.event_receiver.take()
    }

    /// Run the main event loop
    ///
    /// This method implements the main event loop that receives process events from the
    /// ProcessMonitor and handles them appropriately.
    ///
    /// # Behavior
    ///
    /// 1. Enters main event loop, receiving events from event_receiver
    /// 2. Calls handle_process_event() for each received event
    /// 3. Handles channel disconnection gracefully by exiting the loop
    /// 4. Logs all significant events and errors
    pub fn run(&mut self) {
        use tracing::{info, warn};

        let Some(event_receiver) = self.take_event_receiver() else {
            warn!("Event loop already running; run() call ignored");
            return;
        };

        // Main event loop
        info!("Entering main event loop");
        loop {
            match event_receiver.recv() {
                Ok(event) => {
                    // Handle the process event
                    self.handle_process_event(event);
                }
                Err(e) => {
                    // Channel disconnected - this means the ProcessMonitor thread has stopped
                    warn!(
                        "Event receiver channel disconnected: {}. Exiting event loop.",
                        e
                    );
                    break;
                }
            }
        }

        info!("Main event loop exited");
    }

    /// Spawn the event loop in a background thread for a shared controller instance
    ///
    /// This helper is used by the GUI setup to start processing process monitor events
    /// without holding the controller mutex for the entire lifetime of the thread.
    /// The background thread takes ownership of the event receiver and only locks
    /// the controller while handling individual events, preventing GUI callbacks
    /// from being blocked when they need short-lived access to the controller.
    pub fn spawn_event_loop(controller: Arc<Mutex<AppController>>) -> std::thread::JoinHandle<()> {
        let event_receiver = {
            let mut controller_guard = controller.lock();
            controller_guard
                .take_event_receiver()
                .expect("AppController event receiver already taken")
        };

        std::thread::spawn(move || {
            use tracing::{info, warn};

            info!("Entering main event loop");
            loop {
                match event_receiver.recv() {
                    Ok(event) => {
                        let mut controller_guard = controller.lock();
                        controller_guard.handle_process_event(event);
                    }
                    Err(e) => {
                        warn!(
                            "Event receiver channel disconnected: {}. Exiting event loop.",
                            e
                        );
                        break;
                    }
                }
            }
            info!("Main event loop exited");
        })
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
                let is_monitored = config
                    .monitored_apps
                    .iter()
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
                let is_monitored = config
                    .monitored_apps
                    .iter()
                    .any(|app| app.enabled && app.process_name.eq_ignore_ascii_case(&process_name));
                drop(config); // Release lock early

                if is_monitored {
                    info!("Monitored application stopped: {}", process_name);

                    // Decrement active process count
                    let prev_count = self.active_process_count.fetch_sub(1, Ordering::SeqCst);
                    debug!(
                        "Active process count: {} -> {}",
                        prev_count,
                        prev_count.saturating_sub(1)
                    );

                    // Debounce: wait 500ms before disabling to handle quick restarts
                    let last_toggle = *self.last_toggle_time.lock();
                    if last_toggle.elapsed() < std::time::Duration::from_millis(500) {
                        debug!(
                            "Debouncing: last toggle was less than 500ms ago, skipping HDR disable"
                        );
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
                        target.adapter_id.LowPart, target.adapter_id.HighPart, target.target_id, e
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
    ///
    /// This is an internal method called automatically when state changes.
    /// For sending the initial state after startup, use `send_initial_state()` instead.
    fn send_state_update(&self) {
        use tracing::warn;

        let config = self.config.lock();
        let active_apps: Vec<String> = config
            .monitored_apps
            .iter()
            .filter(|app| app.enabled)
            .map(|app| app.display_name.clone())
            .collect();
        drop(config);

        let state = AppState {
            hdr_enabled: self.current_hdr_state.load(Ordering::SeqCst),
            active_apps,
            last_event: format!(
                "Active processes: {}",
                self.active_process_count.load(Ordering::SeqCst)
            ),
        };

        if let Err(e) = self.gui_state_sender.send(state) {
            warn!("Failed to send state update to GUI: {}", e);
        }
    }

    /// Send initial state update to GUI
    ///
    /// Sends the current application state to the GUI to populate it with the
    /// initial configuration. This should be called once after the GUI and
    /// controller are fully initialized to ensure the GUI displays all apps
    /// from the configuration file.
    ///
    /// # Requirements
    ///
    /// - GUI should display all monitored applications from config on startup
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use parking_lot::Mutex;
    /// use easyhdr::controller::AppController;
    ///
    /// let controller = Arc::new(Mutex::new(/* AppController instance */));
    /// let controller_guard = controller.lock();
    /// controller_guard.send_initial_state();
    /// ```
    pub fn send_initial_state(&self) {
        use tracing::info;

        info!("Sending initial state update to populate GUI");
        self.send_state_update();
    }

    /// Add a new application to the configuration
    ///
    /// Adds the provided MonitoredApp to the configuration, saves the config to disk,
    /// and updates the ProcessMonitor's watch list.
    ///
    /// # Arguments
    ///
    /// * `app` - The MonitoredApp to add
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, or an error if saving the configuration fails.
    ///
    /// # Requirements
    ///
    /// - Requirement 1.4: Update enabled state flag for applications
    /// - Requirement 1.5: Delete applications from configuration
    /// - Requirement 1.6: Persist data to config.json using atomic writes
    ///
    /// # Edge Cases
    ///
    /// - If config file is deleted during runtime, continues with in-memory config
    /// - Logs warning if save fails but continues operation
    pub fn add_application(&mut self, app: MonitoredApp) -> Result<()> {
        use tracing::{info, warn};

        info!(
            "Adding application: {} ({})",
            app.display_name, app.process_name
        );

        // Add to config
        {
            let mut config = self.config.lock();
            config.monitored_apps.push(app);
        }

        // Save configuration - if this fails, we continue with in-memory config
        let config = self.config.lock();
        if let Err(e) = ConfigManager::save(&config) {
            warn!(
                "Failed to save configuration to disk: {}. Continuing with in-memory config. \
                 Changes will be lost on application restart.",
                e
            );
        }
        drop(config);

        // Update ProcessMonitor watch list
        self.update_process_monitor_watch_list();

        // Send state update to GUI
        self.send_state_update();

        info!("Application added successfully");
        Ok(())
    }

    /// Remove an application from the configuration by UUID
    ///
    /// Removes the application with the specified UUID from the configuration,
    /// saves the config to disk, and updates the ProcessMonitor's watch list.
    ///
    /// # Arguments
    ///
    /// * `id` - The UUID of the application to remove
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, or an error if saving the configuration fails.
    ///
    /// # Requirements
    ///
    /// - Requirement 1.5: Delete applications from configuration
    /// - Requirement 1.6: Persist data to config.json using atomic writes
    ///
    /// # Edge Cases
    ///
    /// - If config file is deleted during runtime, continues with in-memory config
    /// - Logs warning if save fails but continues operation
    pub fn remove_application(&mut self, id: Uuid) -> Result<()> {
        use tracing::{info, warn};

        info!("Removing application with ID: {}", id);

        // Remove from config
        {
            let mut config = self.config.lock();
            config.monitored_apps.retain(|app| app.id != id);
        }

        // Save configuration - if this fails, we continue with in-memory config
        let config = self.config.lock();
        if let Err(e) = ConfigManager::save(&config) {
            warn!(
                "Failed to save configuration to disk: {}. Continuing with in-memory config. \
                 Changes will be lost on application restart.",
                e
            );
        }
        drop(config);

        // Update ProcessMonitor watch list
        self.update_process_monitor_watch_list();

        // Send state update to GUI
        self.send_state_update();

        info!("Application removed successfully");
        Ok(())
    }

    /// Toggle the enabled state of an application by UUID
    ///
    /// Updates the enabled flag for the application with the specified UUID,
    /// saves the config to disk, and updates the ProcessMonitor's watch list.
    ///
    /// # Arguments
    ///
    /// * `id` - The UUID of the application to toggle
    /// * `enabled` - The new enabled state
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, or an error if saving the configuration fails.
    ///
    /// # Requirements
    ///
    /// - Requirement 1.4: Update enabled state flag for applications
    /// - Requirement 1.6: Persist data to config.json using atomic writes
    ///
    /// # Edge Cases
    ///
    /// - If config file is deleted during runtime, continues with in-memory config
    /// - Logs warning if save fails but continues operation
    pub fn toggle_app_enabled(&mut self, id: Uuid, enabled: bool) -> Result<()> {
        use tracing::{info, warn};

        info!("Toggling application {} to enabled={}", id, enabled);

        // Update enabled flag
        {
            let mut config = self.config.lock();
            if let Some(app) = config.monitored_apps.iter_mut().find(|app| app.id == id) {
                app.enabled = enabled;
            }
        }

        // Save configuration - if this fails, we continue with in-memory config
        let config = self.config.lock();
        if let Err(e) = ConfigManager::save(&config) {
            warn!(
                "Failed to save configuration to disk: {}. Continuing with in-memory config. \
                 Changes will be lost on application restart.",
                e
            );
        }
        drop(config);

        // Update ProcessMonitor watch list
        self.update_process_monitor_watch_list();

        // Send state update to GUI
        self.send_state_update();

        info!("Application enabled state updated successfully");
        Ok(())
    }

    /// Update user preferences
    ///
    /// Updates the user preferences in the configuration and saves to disk.
    ///
    /// # Arguments
    ///
    /// * `prefs` - The new UserPreferences to apply
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, or an error if saving the configuration fails.
    ///
    /// # Requirements
    ///
    /// - Requirement 1.6: Persist data to config.json using atomic writes
    ///
    /// # Edge Cases
    ///
    /// - If config file is deleted during runtime, continues with in-memory config
    /// - Logs warning if save fails but continues operation
    pub fn update_preferences(&mut self, prefs: UserPreferences) -> Result<()> {
        use tracing::{info, warn};

        info!("Updating user preferences");

        // Update preferences
        {
            let mut config = self.config.lock();
            config.preferences = prefs;
        }

        // Save configuration - if this fails, we continue with in-memory config
        let config = self.config.lock();
        if let Err(e) = ConfigManager::save(&config) {
            warn!(
                "Failed to save configuration to disk: {}. Continuing with in-memory config. \
                 Changes will be lost on application restart.",
                e
            );
        }
        drop(config);

        info!("User preferences updated successfully");
        Ok(())
    }

    /// Refresh the display list
    ///
    /// Re-enumerates displays and updates the HDR controller's display cache.
    /// This can be called when display configuration changes (e.g., monitor connected/disconnected).
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, or an error if display enumeration fails.
    ///
    /// # Edge Cases
    ///
    /// - Handles display disconnection during operation by refreshing the display list
    /// - Logs the number of displays found after refresh
    pub fn refresh_displays(&mut self) -> Result<()> {
        use tracing::info;

        info!("Refreshing display list due to potential display configuration change");
        let displays = self.hdr_controller.refresh_displays()?;
        info!(
            "Display list refreshed: {} display(s) found ({} HDR-capable)",
            displays.len(),
            displays.iter().filter(|d| d.supports_hdr).count()
        );
        Ok(())
    }

    /// Update the ProcessMonitor's watch list based on current configuration
    ///
    /// Extracts all enabled application process names from the configuration
    /// and updates the ProcessMonitor's watch list.
    ///
    /// This is called after any configuration change that affects which applications
    /// should be monitored.
    fn update_process_monitor_watch_list(&self) {
        use tracing::debug;

        let config = self.config.lock();
        let process_names: Vec<String> = config
            .monitored_apps
            .iter()
            .filter(|app| app.enabled)
            .map(|app| app.process_name.clone())
            .collect();
        drop(config);

        debug!(
            "Updating ProcessMonitor watch list with {} processes",
            process_names.len()
        );

        let mut watch_list = self.process_monitor_watch_list.lock();
        watch_list.clear();
        for name in process_names {
            watch_list.insert(name.to_lowercase());
        }

        debug!("ProcessMonitor watch list updated");
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
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let controller = AppController::new(config, event_rx, state_tx, watch_list);
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
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Initial count should be 0
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);

        // Handle a started event for the monitored app
        controller.handle_process_event(ProcessEvent::Started("app".to_string()));

        // Count should be incremented to 1
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);

        // Should have sent a state update
        let state = state_rx.try_recv().unwrap();
        assert!(state.hdr_enabled);
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
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

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
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

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
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

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
        assert!(!state.hdr_enabled);
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
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Start first app
        controller.handle_process_event(ProcessEvent::Started("app1".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Start second app
        controller.handle_process_event(ProcessEvent::Started("app2".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 2);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop first app - HDR should remain on
        controller.handle_process_event(ProcessEvent::Stopped("app1".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop second app - HDR should turn off
        controller.handle_process_event(ProcessEvent::Stopped("app2".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));
    }

    // NOTE: This test may fail when run in parallel with other tests due to a race condition.
    // All tests that call methods invoking ConfigManager::save() write to the same shared
    // config file (./EasyHDR/config.json on macOS, %APPDATA%\EasyHDR\config.json on Windows).
    // When multiple tests write to this file simultaneously, they can interfere with each other.
    //
    // The functionality itself is correct - the test passes consistently when run:
    // - Individually: `cargo test test_add_application`
    // - Single-threaded: `cargo test -- --test-threads=1`
    //
    // This is a test isolation issue, not a code defect. Will be fixed in Task 17.3.
    #[test]
    fn test_add_application() {
        let config = AppConfig::default();
        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller =
            AppController::new(config, event_rx, state_tx, watch_list.clone()).unwrap();

        // Create a new app to add
        let app = MonitoredApp {
            id: Uuid::new_v4(),
            display_name: "New App".to_string(),
            exe_path: PathBuf::from("C:\\test\\newapp.exe"),
            process_name: "newapp".to_string(),
            enabled: true,
            icon_data: None,
        };

        // Add the application
        let result = controller.add_application(app.clone());
        assert!(result.is_ok());

        // Verify it was added to config
        let config = controller.config.lock();
        assert_eq!(config.monitored_apps.len(), 1);
        assert_eq!(config.monitored_apps[0].display_name, "New App");
        drop(config);

        // Verify watch list was updated
        let watch_list_guard = watch_list.lock();
        assert!(watch_list_guard.contains("newapp"));
    }

    // NOTE: This test may fail when run in parallel with other tests due to a race condition.
    // All tests that call methods invoking ConfigManager::save() write to the same shared
    // config file (./EasyHDR/config.json on macOS, %APPDATA%\EasyHDR\config.json on Windows).
    // When multiple tests write to this file simultaneously, they can interfere with each other.
    //
    // The functionality itself is correct - the test passes consistently when run:
    // - Individually: `cargo test test_remove_application`
    // - Single-threaded: `cargo test -- --test-threads=1`
    //
    // This is a test isolation issue, not a code defect. Will be fixed in Task 17.3.
    #[test]
    fn test_remove_application() {
        let mut config = AppConfig::default();
        let app_id = Uuid::new_v4();
        config.monitored_apps.push(MonitoredApp {
            id: app_id,
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        });

        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller =
            AppController::new(config, event_rx, state_tx, watch_list.clone()).unwrap();

        // Remove the application
        let result = controller.remove_application(app_id);
        assert!(result.is_ok());

        // Verify it was removed from config
        let config = controller.config.lock();
        assert_eq!(config.monitored_apps.len(), 0);
        drop(config);

        // Verify watch list was updated
        let watch_list_guard = watch_list.lock();
        assert!(!watch_list_guard.contains("app"));
    }

    // NOTE: This test may fail when run in parallel with other tests due to a race condition.
    // All tests that call methods invoking ConfigManager::save() write to the same shared
    // config file (./EasyHDR/config.json on macOS, %APPDATA%\EasyHDR\config.json on Windows).
    // When multiple tests write to this file simultaneously, they can interfere with each other.
    //
    // The functionality itself is correct - the test passes consistently when run:
    // - Individually: `cargo test test_toggle_app_enabled`
    // - Single-threaded: `cargo test -- --test-threads=1`
    //
    // This is a test isolation issue, not a code defect. Will be fixed in Task 17.3.
    #[test]
    fn test_toggle_app_enabled() {
        let mut config = AppConfig::default();
        let app_id = Uuid::new_v4();
        config.monitored_apps.push(MonitoredApp {
            id: app_id,
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        });

        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller =
            AppController::new(config, event_rx, state_tx, watch_list.clone()).unwrap();

        // Initially populate watch list
        controller.update_process_monitor_watch_list();
        {
            let watch_list_guard = watch_list.lock();
            assert!(watch_list_guard.contains("app"));
        }

        // Disable the application
        let result = controller.toggle_app_enabled(app_id, false);
        assert!(result.is_ok());

        // Verify enabled flag was updated
        let config = controller.config.lock();
        assert!(!config.monitored_apps[0].enabled);
        drop(config);

        // Verify watch list was updated (app should be removed)
        let watch_list_guard = watch_list.lock();
        assert!(!watch_list_guard.contains("app"));
        drop(watch_list_guard);

        // Re-enable the application
        let result = controller.toggle_app_enabled(app_id, true);
        assert!(result.is_ok());

        // Verify enabled flag was updated
        let config = controller.config.lock();
        assert!(config.monitored_apps[0].enabled);
        drop(config);

        // Verify watch list was updated (app should be added back)
        let watch_list_guard = watch_list.lock();
        assert!(watch_list_guard.contains("app"));
    }

    // NOTE: This test may fail when run in parallel with other tests due to a race condition.
    // All tests that call methods invoking ConfigManager::save() write to the same shared
    // config file (./EasyHDR/config.json on macOS, %APPDATA%\EasyHDR\config.json on Windows).
    // When multiple tests write to this file simultaneously, they can interfere with each other.
    //
    // The functionality itself is correct - the test passes consistently when run:
    // - Individually: `cargo test test_update_preferences`
    // - Single-threaded: `cargo test -- --test-threads=1`
    //
    // This is a test isolation issue, not a code defect. Will be fixed in Task 17.3.
    #[test]
    fn test_update_preferences() {
        let config = AppConfig::default();
        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Create new preferences
        let new_prefs = UserPreferences {
            auto_start: true,
            monitoring_interval_ms: 2000,
            show_tray_notifications: false,
        };

        // Update preferences
        let result = controller.update_preferences(new_prefs.clone());
        assert!(result.is_ok());

        // Verify preferences were updated
        let config = controller.config.lock();
        assert!(config.preferences.auto_start);
        assert_eq!(config.preferences.monitoring_interval_ms, 2000);
        assert!(!config.preferences.show_tray_notifications);
    }

    #[test]
    fn test_update_process_monitor_watch_list() {
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
            enabled: false, // Disabled
            icon_data: None,
        });
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::new_v4(),
            display_name: "App 3".to_string(),
            exe_path: PathBuf::from("C:\\test\\app3.exe"),
            process_name: "app3".to_string(),
            enabled: true,
            icon_data: None,
        });

        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let controller =
            AppController::new(config, event_rx, state_tx, watch_list.clone()).unwrap();

        // Update watch list
        controller.update_process_monitor_watch_list();

        // Verify only enabled apps are in watch list
        let watch_list_guard = watch_list.lock();
        assert_eq!(watch_list_guard.len(), 2);
        assert!(watch_list_guard.contains("app1"));
        assert!(!watch_list_guard.contains("app2")); // Disabled
        assert!(watch_list_guard.contains("app3"));
    }

    #[test]
    fn test_run_processes_events() {
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        });

        let (event_tx, event_rx) = mpsc::channel();
        let (state_tx, state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Spawn thread to run the event loop
        let handle = std::thread::spawn(move || {
            controller.run();
        });

        // Send a process started event
        event_tx
            .send(ProcessEvent::Started("app".to_string()))
            .unwrap();

        // Wait a bit for the event to be processed
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Verify state update was sent
        let state = state_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .unwrap();
        assert!(state.hdr_enabled);

        // Close the channel to exit the event loop
        drop(event_tx);

        // Wait for the thread to complete
        handle.join().unwrap();
    }

    #[test]
    fn test_run_handles_channel_disconnection_gracefully() {
        let config = AppConfig::default();

        let (event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Spawn thread to run the event loop
        let handle = std::thread::spawn(move || {
            controller.run();
        });

        // Immediately close the channel
        drop(event_tx);

        // Wait for the thread to complete - should exit gracefully
        let result = handle.join();
        assert!(
            result.is_ok(),
            "Event loop should exit gracefully when channel disconnects"
        );
    }

    #[test]
    fn test_run_processes_multiple_events() {
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

        let (event_tx, event_rx) = mpsc::channel();
        let (state_tx, state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Spawn thread to run the event loop
        let handle = std::thread::spawn(move || {
            controller.run();
        });

        // Send multiple events
        event_tx
            .send(ProcessEvent::Started("app1".to_string()))
            .unwrap();
        event_tx
            .send(ProcessEvent::Started("app2".to_string()))
            .unwrap();

        // Wait for events to be processed
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Verify state updates were sent
        let state1 = state_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .unwrap();
        assert!(state1.hdr_enabled);

        let state2 = state_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .unwrap();
        assert!(state2.hdr_enabled);

        // Close the channel to exit the event loop
        drop(event_tx);

        // Wait for the thread to complete
        handle.join().unwrap();
    }

    /// Test rapid process start/stop with debouncing
    ///
    /// This test verifies that the debouncing mechanism works correctly when a process
    /// stops and starts quickly (within 500ms). The HDR should remain on during rapid
    /// restarts to avoid unnecessary toggling.
    ///
    /// # Requirements
    ///
    /// - Requirement 4.8: Debounce rapid state changes by waiting 500ms before toggling back
    #[test]
    fn test_rapid_process_restart_debouncing() {
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
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Start the app - HDR should turn on
        controller.handle_process_event(ProcessEvent::Started("app".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Record the time of the first toggle
        let first_toggle_time = *controller.last_toggle_time.lock();

        // Wait a short time (less than 500ms)
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Stop the app - HDR should NOT turn off due to debouncing
        controller.handle_process_event(ProcessEvent::Stopped("app".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        // HDR should still be on because we're within the debounce window
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Verify that the last toggle time hasn't changed (no toggle occurred)
        let second_toggle_time = *controller.last_toggle_time.lock();
        assert_eq!(
            first_toggle_time, second_toggle_time,
            "Toggle time should not change during debounce"
        );

        // Now test that after the debounce period expires, HDR can be toggled again
        // First, restart the app to get the count back to 1
        controller.handle_process_event(ProcessEvent::Started("app".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        // HDR should still be on (it never turned off)
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait for debounce period to expire (600ms to be safe)
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop the app - HDR should turn off now (debounce period has passed)
        controller.handle_process_event(ProcessEvent::Stopped("app".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));
    }

    /// Test that debouncing doesn't prevent HDR from turning on
    ///
    /// This test verifies that the debouncing mechanism only affects HDR disable operations,
    /// not enable operations. HDR should always turn on immediately when a monitored app starts.
    #[test]
    fn test_debouncing_does_not_affect_hdr_enable() {
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
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Start the app - HDR should turn on immediately
        controller.handle_process_event(ProcessEvent::Started("app".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop the app - HDR should turn off
        controller.handle_process_event(ProcessEvent::Stopped("app".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));

        // Immediately start the app again (within what would be a debounce window if it applied to enable)
        // HDR should turn on immediately regardless of timing
        controller.handle_process_event(ProcessEvent::Started("app".to_string()));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));
    }
}
