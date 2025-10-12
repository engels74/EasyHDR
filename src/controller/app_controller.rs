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
            event_receiver,
            gui_state_sender,
            last_toggle_time: Arc::new(Mutex::new(Instant::now())),
            process_monitor_watch_list,
        })
    }

    /// Run the main event loop
    ///
    /// This method implements the main event loop that receives process events from the
    /// ProcessMonitor and handles them appropriately. It implements an optional startup
    /// delay to avoid boot race conditions.
    ///
    /// # Requirements
    ///
    /// - Requirement 4.7: Implement optional startup delay of 2-5 seconds to avoid boot race conditions
    ///
    /// # Behavior
    ///
    /// 1. Applies startup delay if configured in preferences (startup_delay_ms)
    /// 2. Enters main event loop, receiving events from event_receiver
    /// 3. Calls handle_process_event() for each received event
    /// 4. Handles channel disconnection gracefully by exiting the loop
    /// 5. Logs all significant events and errors
    pub fn run(&mut self) {
        use tracing::{info, warn};

        // Apply startup delay if configured
        let startup_delay_ms = {
            let config = self.config.lock();
            config.preferences.startup_delay_ms
        };

        if startup_delay_ms > 0 {
            info!("Applying startup delay of {}ms to avoid boot race conditions", startup_delay_ms);
            std::thread::sleep(std::time::Duration::from_millis(startup_delay_ms));
            info!("Startup delay complete, beginning process monitoring");
        } else {
            info!("No startup delay configured, beginning process monitoring immediately");
        }

        // Main event loop
        info!("Entering main event loop");
        loop {
            match self.event_receiver.recv() {
                Ok(event) => {
                    // Handle the process event
                    self.handle_process_event(event);
                }
                Err(e) => {
                    // Channel disconnected - this means the ProcessMonitor thread has stopped
                    warn!("Event receiver channel disconnected: {}. Exiting event loop.", e);
                    break;
                }
            }
        }

        info!("Main event loop exited");
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
    pub fn add_application(&mut self, app: MonitoredApp) -> Result<()> {
        use tracing::info;

        info!("Adding application: {} ({})", app.display_name, app.process_name);

        // Add to config
        {
            let mut config = self.config.lock();
            config.monitored_apps.push(app);
        }

        // Save configuration
        let config = self.config.lock();
        ConfigManager::save(&config)?;
        drop(config);

        // Update ProcessMonitor watch list
        self.update_process_monitor_watch_list();

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
    pub fn remove_application(&mut self, id: Uuid) -> Result<()> {
        use tracing::info;

        info!("Removing application with ID: {}", id);

        // Remove from config
        {
            let mut config = self.config.lock();
            config.monitored_apps.retain(|app| app.id != id);
        }

        // Save configuration
        let config = self.config.lock();
        ConfigManager::save(&config)?;
        drop(config);

        // Update ProcessMonitor watch list
        self.update_process_monitor_watch_list();

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
    pub fn toggle_app_enabled(&mut self, id: Uuid, enabled: bool) -> Result<()> {
        use tracing::info;

        info!("Toggling application {} to enabled={}", id, enabled);

        // Update enabled flag
        {
            let mut config = self.config.lock();
            if let Some(app) = config.monitored_apps.iter_mut().find(|app| app.id == id) {
                app.enabled = enabled;
            }
        }

        // Save configuration
        let config = self.config.lock();
        ConfigManager::save(&config)?;
        drop(config);

        // Update ProcessMonitor watch list
        self.update_process_monitor_watch_list();

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
    pub fn update_preferences(&mut self, prefs: UserPreferences) -> Result<()> {
        use tracing::info;

        info!("Updating user preferences");

        // Update preferences
        {
            let mut config = self.config.lock();
            config.preferences = prefs;
        }

        // Save configuration
        let config = self.config.lock();
        ConfigManager::save(&config)?;
        drop(config);

        info!("User preferences updated successfully");
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
        let process_names: Vec<String> = config.monitored_apps.iter()
            .filter(|app| app.enabled)
            .map(|app| app.process_name.clone())
            .collect();
        drop(config);

        debug!("Updating ProcessMonitor watch list with {} processes", process_names.len());

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
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

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

    #[test]
    fn test_add_application() {
        let config = AppConfig::default();
        let (_event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list.clone()).unwrap();

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

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list.clone()).unwrap();

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

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list.clone()).unwrap();

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
        assert_eq!(config.monitored_apps[0].enabled, false);
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
        assert_eq!(config.monitored_apps[0].enabled, true);
        drop(config);

        // Verify watch list was updated (app should be added back)
        let watch_list_guard = watch_list.lock();
        assert!(watch_list_guard.contains("app"));
    }

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
            startup_delay_ms: 5000,
            show_tray_notifications: false,
        };

        // Update preferences
        let result = controller.update_preferences(new_prefs.clone());
        assert!(result.is_ok());

        // Verify preferences were updated
        let config = controller.config.lock();
        assert_eq!(config.preferences.auto_start, true);
        assert_eq!(config.preferences.monitoring_interval_ms, 2000);
        assert_eq!(config.preferences.startup_delay_ms, 5000);
        assert_eq!(config.preferences.show_tray_notifications, false);
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

        let controller = AppController::new(config, event_rx, state_tx, watch_list.clone()).unwrap();

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
    fn test_run_applies_startup_delay() {
        use std::time::Instant;

        let mut config = AppConfig::default();
        config.preferences.startup_delay_ms = 100; // Short delay for testing

        let (event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Spawn thread to run the event loop
        let start_time = Instant::now();
        let handle = std::thread::spawn(move || {
            controller.run();
            start_time.elapsed()
        });

        // Close the channel to exit the event loop
        drop(event_tx);

        // Wait for the thread to complete
        let elapsed = handle.join().unwrap();

        // Verify that at least the startup delay was applied
        assert!(elapsed.as_millis() >= 100, "Startup delay should be at least 100ms, was {}ms", elapsed.as_millis());
    }

    #[test]
    fn test_run_no_startup_delay_when_zero() {
        use std::time::Instant;

        let mut config = AppConfig::default();
        config.preferences.startup_delay_ms = 0; // No delay

        let (event_tx, event_rx) = mpsc::channel();
        let (state_tx, _state_rx) = mpsc::channel();
        let watch_list = Arc::new(Mutex::new(HashSet::new()));

        let mut controller = AppController::new(config, event_rx, state_tx, watch_list).unwrap();

        // Spawn thread to run the event loop
        let start_time = Instant::now();
        let handle = std::thread::spawn(move || {
            controller.run();
            start_time.elapsed()
        });

        // Close the channel to exit the event loop
        drop(event_tx);

        // Wait for the thread to complete
        let elapsed = handle.join().unwrap();

        // Verify that the delay is minimal (should be very quick)
        assert!(elapsed.as_millis() < 50, "Should complete quickly without delay, took {}ms", elapsed.as_millis());
    }

    #[test]
    fn test_run_processes_events() {
        let mut config = AppConfig::default();
        config.preferences.startup_delay_ms = 0; // No delay for faster test
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
        event_tx.send(ProcessEvent::Started("app".to_string())).unwrap();

        // Wait a bit for the event to be processed
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Verify state update was sent
        let state = state_rx.recv_timeout(std::time::Duration::from_millis(100)).unwrap();
        assert_eq!(state.hdr_enabled, true);

        // Close the channel to exit the event loop
        drop(event_tx);

        // Wait for the thread to complete
        handle.join().unwrap();
    }

    #[test]
    fn test_run_handles_channel_disconnection_gracefully() {
        let mut config = AppConfig::default();
        config.preferences.startup_delay_ms = 0; // No delay for faster test

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
        assert!(result.is_ok(), "Event loop should exit gracefully when channel disconnects");
    }

    #[test]
    fn test_run_processes_multiple_events() {
        let mut config = AppConfig::default();
        config.preferences.startup_delay_ms = 0; // No delay for faster test
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
        event_tx.send(ProcessEvent::Started("app1".to_string())).unwrap();
        event_tx.send(ProcessEvent::Started("app2".to_string())).unwrap();

        // Wait for events to be processed
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Verify state updates were sent
        let state1 = state_rx.recv_timeout(std::time::Duration::from_millis(100)).unwrap();
        assert_eq!(state1.hdr_enabled, true);

        let state2 = state_rx.recv_timeout(std::time::Duration::from_millis(100)).unwrap();
        assert_eq!(state2.hdr_enabled, true);

        // Close the channel to exit the event loop
        drop(event_tx);

        // Wait for the thread to complete
        handle.join().unwrap();
    }
}

