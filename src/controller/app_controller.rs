//! Application controller implementation
//!
//! This module implements the main application logic controller that
//! coordinates between process monitoring and HDR control.

use crate::config::{AppConfig, ConfigManager, MonitoredApp, UserPreferences};
use crate::error::{EasyHdrError, Result};
use crate::hdr::HdrController;
use crate::monitor::{AppIdentifier, HdrStateEvent, ProcessEvent};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
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
    /// Event receiver from HDR state monitor (taken when event loop starts)
    hdr_state_receiver: Option<mpsc::Receiver<HdrStateEvent>>,
    /// State sender to GUI
    gui_state_sender: mpsc::SyncSender<AppState>,
    /// Last toggle time for debouncing
    last_toggle_time: Arc<Mutex<Instant>>,
    /// Reference to `ProcessMonitor`'s watch list for updating
    process_monitor_watch_list: Arc<Mutex<Vec<MonitoredApp>>>,
}

impl AppController {
    /// Create a new application controller and detect initial HDR state
    pub fn new(
        config: AppConfig,
        event_receiver: mpsc::Receiver<ProcessEvent>,
        hdr_state_receiver: mpsc::Receiver<HdrStateEvent>,
        gui_state_sender: mpsc::SyncSender<AppState>,
        process_monitor_watch_list: Arc<Mutex<Vec<MonitoredApp>>>,
    ) -> Result<Self> {
        use tracing::info;

        let hdr_controller = HdrController::new().map_err(|e| {
            use tracing::error;
            error!("Failed to initialize HDR controller: {e}");
            // Preserve error chain by wrapping the source error
            EasyHdrError::HdrControlFailed(Box::new(e))
        })?;

        // Detect the actual current HDR state at startup
        // This ensures the GUI displays the correct initial state
        let initial_hdr_state = Self::detect_current_hdr_state(&hdr_controller);
        info!("Detected initial HDR state: {}", initial_hdr_state);

        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            hdr_controller,
            active_process_count: AtomicUsize::new(0),
            current_hdr_state: AtomicBool::new(initial_hdr_state),
            event_receiver: Some(event_receiver),
            hdr_state_receiver: Some(hdr_state_receiver),
            gui_state_sender,
            last_toggle_time: Arc::new(Mutex::new(Instant::now())),
            process_monitor_watch_list,
        })
    }

    /// Detect the current HDR state from the system by checking all HDR-capable displays
    fn detect_current_hdr_state(hdr_controller: &HdrController) -> bool {
        // Delegate to the shared implementation in HdrController
        hdr_controller.detect_current_hdr_state()
    }

    /// Take ownership of the event receiver if it hasn't been taken yet.
    /// Returns None if already taken. Caller should treat None as a no-op.
    fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<ProcessEvent>> {
        self.event_receiver.take()
    }

    /// Take ownership of the HDR state receiver if it hasn't been taken yet.
    /// Returns None if already taken. Caller should treat None as a no-op.
    fn take_hdr_state_receiver(&mut self) -> Option<mpsc::Receiver<HdrStateEvent>> {
        self.hdr_state_receiver.take()
    }

    /// Run the main event loop to receive process and HDR state events.
    /// Uses 100ms timeout to ensure prompt HDR state change detection.
    pub fn run(&mut self) {
        use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
        use std::time::Duration;
        use tracing::{info, warn};

        let Some(event_receiver) = self.take_event_receiver() else {
            warn!("Event loop already running; run() call ignored");
            return;
        };

        let Some(hdr_state_receiver) = self.take_hdr_state_receiver() else {
            warn!("Event loop already running; run() call ignored");
            return;
        };

        // Main event loop
        info!("Entering main event loop (process events + HDR state events)");
        loop {
            // Check for process events with timeout to allow periodic HDR state checks
            match event_receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(event) => {
                    self.handle_process_event(event);
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Timeout is normal - just continue to check HDR state events
                }
                Err(RecvTimeoutError::Disconnected) => {
                    warn!("Process event receiver channel disconnected. Exiting event loop.");
                    break;
                }
            }

            // Check for HDR state events (non-blocking, drain all available)
            loop {
                match hdr_state_receiver.try_recv() {
                    Ok(event) => {
                        self.handle_hdr_state_event(event);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        warn!("HDR state receiver channel disconnected.");
                        // Continue processing process events even if HDR monitor stops
                        break;
                    }
                }
            }
        }

        info!("Main event loop exited");
    }

    /// Spawn the event loop in a background thread. Only locks controller while handling individual events,
    /// preventing GUI callbacks from being blocked.
    pub fn spawn_event_loop(controller: Arc<Mutex<AppController>>) -> std::thread::JoinHandle<()> {
        let (event_receiver, hdr_state_receiver) = {
            let mut controller_guard = controller.lock();
            (
                controller_guard
                    .take_event_receiver()
                    .expect("AppController event receiver already taken"),
                controller_guard
                    .take_hdr_state_receiver()
                    .expect("AppController HDR state receiver already taken"),
            )
        };

        std::thread::spawn(move || {
            use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
            use std::time::Duration;
            use tracing::{info, warn};

            info!("Entering main event loop (process events + HDR state events)");
            loop {
                // Check for process events with timeout to allow periodic HDR state checks
                match event_receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => {
                        let mut controller_guard = controller.lock();
                        controller_guard.handle_process_event(event);
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // Timeout is normal - just continue to check HDR state events
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        warn!("Process event receiver channel disconnected. Exiting event loop.");
                        break;
                    }
                }

                // Check for HDR state events (non-blocking, drain all available)
                loop {
                    match hdr_state_receiver.try_recv() {
                        Ok(event) => {
                            let mut controller_guard = controller.lock();
                            controller_guard.handle_hdr_state_event(event);
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            warn!("HDR state receiver channel disconnected.");
                            // Continue processing process events even if HDR monitor stops
                            break;
                        }
                    }
                }
            }
            info!("Main event loop exited");
        })
    }

    /// Handle a process event to automatically toggle HDR.
    /// Enables HDR when first monitored app starts, disables when last one stops.
    /// Uses 500ms debouncing to prevent rapid toggling during app restarts.
    fn handle_process_event(&mut self, event: ProcessEvent) {
        use tracing::{debug, error, info};

        match event {
            ProcessEvent::Started(app_id) => {
                debug!("Process started event: {:?}", app_id);

                // Check if this app is in our monitored list and enabled
                let config = self.config.lock();
                let is_monitored = config
                    .monitored_apps
                    .iter()
                    .any(|app| match (&app_id, app) {
                        (AppIdentifier::Win32(process_name), MonitoredApp::Win32(win32_app)) => {
                            win32_app.enabled
                                && win32_app.process_name.eq_ignore_ascii_case(process_name)
                        }
                        (AppIdentifier::Uwp(package_family_name), MonitoredApp::Uwp(uwp_app)) => {
                            uwp_app.enabled && uwp_app.package_family_name == *package_family_name
                        }
                        _ => false, // Mismatched types (Win32 vs UWP)
                    });
                drop(config); // Release lock early

                if is_monitored {
                    match &app_id {
                        AppIdentifier::Win32(process_name) => {
                            info!("Monitored Win32 application started: {}", process_name);
                        }
                        AppIdentifier::Uwp(package_family_name) => {
                            info!("Monitored UWP application started: {}", package_family_name);
                        }
                    }

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

            ProcessEvent::Stopped(app_id) => {
                debug!("Process stopped event: {:?}", app_id);

                // Check if this app is in our monitored list and enabled
                let config = self.config.lock();
                let is_monitored = config
                    .monitored_apps
                    .iter()
                    .any(|app| match (&app_id, app) {
                        (AppIdentifier::Win32(process_name), MonitoredApp::Win32(win32_app)) => {
                            win32_app.enabled
                                && win32_app.process_name.eq_ignore_ascii_case(process_name)
                        }
                        (AppIdentifier::Uwp(package_family_name), MonitoredApp::Uwp(uwp_app)) => {
                            uwp_app.enabled && uwp_app.package_family_name == *package_family_name
                        }
                        _ => false, // Mismatched types (Win32 vs UWP)
                    });
                drop(config); // Release lock early

                if is_monitored {
                    match &app_id {
                        AppIdentifier::Win32(process_name) => {
                            info!("Monitored Win32 application stopped: {}", process_name);
                        }
                        AppIdentifier::Uwp(package_family_name) => {
                            info!("Monitored UWP application stopped: {}", package_family_name);
                        }
                    }

                    // Decrement active process count using checked atomic pattern to prevent underflow
                    // Uses fetch_update with saturating_sub to ensure count never wraps to usize::MAX
                    let prev_count = self
                        .active_process_count
                        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                            Some(count.saturating_sub(1))
                        })
                        .expect("fetch_update with Some(_) never fails");
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

    /// Handle an HDR state event from external Windows settings changes.
    /// Updates internal state and GUI without calling `toggle_hdr()` since the change already occurred.
    fn handle_hdr_state_event(&mut self, event: HdrStateEvent) {
        use tracing::{debug, info};

        match event {
            HdrStateEvent::Enabled => {
                info!("HDR was enabled externally (via Windows settings)");
                self.current_hdr_state.store(true, Ordering::SeqCst);
                debug!("Updated internal HDR state to: true");
            }
            HdrStateEvent::Disabled => {
                info!("HDR was disabled externally (via Windows settings)");
                self.current_hdr_state.store(false, Ordering::SeqCst);
                debug!("Updated internal HDR state to: false");
            }
        }

        // Send state update to GUI to reflect the external change
        debug!("Sending state update to GUI after external HDR state change");
        self.send_state_update();
    }

    /// Toggle HDR state globally on all displays and update debouncing timestamp
    fn toggle_hdr(&mut self, enable: bool) -> Result<()> {
        use tracing::{info, warn};

        info!("Toggling HDR: {}", if enable { "ON" } else { "OFF" });

        // Call HDR controller to set HDR state globally
        let results = self.hdr_controller.set_hdr_global(enable).map_err(|e| {
            use tracing::error;
            error!("Failed to set HDR state globally: {e}");
            // Preserve error chain by wrapping the source error
            EasyHdrError::HdrControlFailed(Box::new(e))
        })?;

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

    /// Send current state update to GUI (HDR state, active apps, process count)
    fn send_state_update(&self) {
        use tracing::{debug, warn};

        let config = self.config.lock();
        let active_apps: Vec<String> = config
            .monitored_apps
            .iter()
            .filter(|app| app.is_enabled())
            .map(|app| app.display_name().to_string())
            .collect();
        drop(config);

        let hdr_enabled = self.current_hdr_state.load(Ordering::SeqCst);
        let state = AppState {
            hdr_enabled,
            active_apps,
            last_event: format!(
                "Active processes: {}",
                self.active_process_count.load(Ordering::SeqCst)
            ),
        };

        debug!("Sending state update to GUI: HDR enabled = {}", hdr_enabled);

        if let Err(e) = self.gui_state_sender.send(state) {
            warn!("Failed to send state update to GUI: {}", e);
        } else {
            debug!("State update sent successfully to GUI");
        }
    }

    /// Send initial state to GUI and populate `ProcessMonitor` watch list.
    /// Call once after initialization to display all configured apps.
    pub fn send_initial_state(&self) {
        use tracing::info;

        info!("Sending initial state update to populate GUI");

        // Populate ProcessMonitor watch list with enabled apps from loaded config
        // This is critical for process detection to work after startup
        info!("Initializing ProcessMonitor watch list from loaded configuration");
        self.update_process_monitor_watch_list();

        self.send_state_update();
    }

    /// Add application to config, save to disk, and update `ProcessMonitor` watch list.
    /// Logs warning and continues with in-memory config if save fails.
    pub fn add_application(&mut self, app: MonitoredApp) -> Result<()> {
        use tracing::{info, warn};

        match &app {
            MonitoredApp::Win32(win32_app) => {
                info!(
                    "Adding Win32 application: {} ({})",
                    win32_app.display_name, win32_app.process_name
                );
            }
            MonitoredApp::Uwp(uwp_app) => {
                info!(
                    "Adding UWP application: {} ({})",
                    uwp_app.display_name, uwp_app.package_family_name
                );
            }
        }

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

    /// Remove application by UUID, save to disk, and update `ProcessMonitor` watch list.
    /// Logs warning and continues with in-memory config if save fails.
    pub fn remove_application(&mut self, id: Uuid) -> Result<()> {
        use tracing::{info, warn};

        info!("Removing application with ID: {}", id);

        // Remove from config
        {
            let mut config = self.config.lock();
            config.monitored_apps.retain(|app| app.id() != &id);
        }

        // Clean up cached icon (Requirement 4.4: graceful failure)
        if let Ok(cache) = crate::utils::icon_cache::IconCache::new(
            crate::utils::icon_cache::IconCache::default_cache_dir(),
        ) {
            if let Err(e) = cache.remove_icon(id) {
                warn!("Failed to remove cached icon for app {}: {}", id, e);
                // Continue with app removal despite cache cleanup failure
            }
        } else {
            warn!("Failed to initialize icon cache for cleanup of app {}", id);
            // Continue with app removal despite cache initialization failure
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

    /// Toggle application enabled state by UUID, save to disk, and update `ProcessMonitor` watch list.
    /// Logs warning and continues with in-memory config if save fails.
    pub fn toggle_app_enabled(&mut self, id: Uuid, enabled: bool) -> Result<()> {
        use tracing::{info, warn};

        info!("Toggling application {} to enabled={}", id, enabled);

        // Update enabled flag
        {
            let mut config = self.config.lock();
            if let Some(app) = config.monitored_apps.iter_mut().find(|app| app.id() == &id) {
                match app {
                    MonitoredApp::Win32(win32_app) => win32_app.enabled = enabled,
                    MonitoredApp::Uwp(uwp_app) => uwp_app.enabled = enabled,
                }
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

    /// Update user preferences and save to disk.
    /// Logs warning and continues with in-memory config if save fails.
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

    /// Re-enumerate displays and update HDR controller's display cache.
    /// Call when display configuration changes (e.g., monitor connected/disconnected).
    pub fn refresh_displays(&mut self) -> Result<()> {
        use tracing::info;

        info!("Refreshing display list due to potential display configuration change");
        let displays = self.hdr_controller.refresh_displays().map_err(|e| {
            use tracing::error;
            error!("Failed to refresh display list: {e}");
            // Preserve error chain by wrapping the source error
            EasyHdrError::HdrControlFailed(Box::new(e))
        })?;
        info!(
            "Display list refreshed: {} display(s) found ({} HDR-capable)",
            displays.len(),
            displays.iter().filter(|d| d.supports_hdr).count()
        );
        Ok(())
    }

    /// Update `ProcessMonitor` watch list with enabled monitored applications from config
    ///
    /// Filters the config to include only enabled applications (both Win32 and UWP)
    /// and updates the `ProcessMonitor`'s watch list.
    fn update_process_monitor_watch_list(&self) {
        use tracing::debug;

        let config = self.config.lock();
        let monitored_apps: Vec<MonitoredApp> = config
            .monitored_apps
            .iter()
            .filter(|app| app.is_enabled())
            .cloned()
            .collect();
        drop(config);

        debug!(
            "Updating ProcessMonitor watch list with {} monitored applications",
            monitored_apps.len()
        );

        let mut watch_list = self.process_monitor_watch_list.lock();
        *watch_list = monitored_apps;

        debug!("ProcessMonitor watch list updated");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::Win32App;
    use crate::config::{AppConfig, MonitoredApp};
    use std::path::PathBuf;
    use tempfile::TempDir;
    use uuid::Uuid;

    /// Helper to create a temporary directory for tests
    /// Returns a `TempDir` that automatically cleans up when dropped
    fn create_test_dir() -> TempDir {
        tempfile::tempdir().expect("Failed to create temp directory")
    }

    /// Helper to set APPDATA for a test scope
    /// Returns a guard that restores the original value when dropped
    ///
    /// # Safety Considerations
    ///
    /// This guard uses `std::env::set_var` and `std::env::remove_var`, which are marked
    /// unsafe because they can cause data races when other threads are reading environment
    /// variables concurrently.
    ///
    /// **Safety Invariants:**
    /// 1. Each test gets its own unique `TempDir`, so parallel tests write to different paths
    /// 2. The guard is RAII-based and restores the original value on drop, preventing
    ///    environment pollution between tests
    /// 3. No other threads should be spawned or running during the lifetime of this guard
    ///    within the same test function
    ///
    /// **Why this is safe in parallel test execution:**
    /// - While `std::env::set_var` is unsafe, the actual risk is when threads read env vars
    ///   while another thread modifies them
    /// - Each test function runs in its own thread with its own stack frame
    /// - The `AppController` being tested is not spawning additional threads during these tests
    /// - The guard ensures cleanup even on panic via Drop
    /// - The modification is scoped to the test function's lifetime
    /// - Tests can safely run in parallel (`cargo test --lib`) without `--test-threads=1`
    ///
    /// **Note:** While these tests CAN run in parallel, they can also run single-threaded
    /// if needed for other reasons (e.g., debugging, Miri analysis).
    struct AppdataGuard {
        original: Option<String>,
    }

    #[expect(
        unsafe_code,
        reason = "Test-only code that modifies environment variables with documented safety invariants. Safe in parallel test execution."
    )]
    impl AppdataGuard {
        fn new(temp_dir: &TempDir) -> Self {
            let original = std::env::var("APPDATA").ok();
            // SAFETY: This is safe because:
            // 1. Each test gets its own unique TempDir path (no shared state between tests)
            // 2. The guard is RAII-based and restores the original value on drop
            // 3. No other threads are spawned during the test function
            // 4. Each test runs in its own thread with isolated stack frame
            // See struct-level documentation for full safety invariants.
            unsafe {
                std::env::set_var("APPDATA", temp_dir.path());
            }
            Self { original }
        }
    }

    #[expect(
        unsafe_code,
        reason = "Test-only code that restores environment variables with documented safety invariants. Safe in parallel test execution."
    )]
    impl Drop for AppdataGuard {
        fn drop(&mut self) {
            // SAFETY: This is safe because:
            // 1. Each test has its own guard instance (no shared state)
            // 2. We're restoring the original state, preventing test pollution
            // 3. No other threads are accessing environment variables within this test
            // 4. Drop runs in the same thread that created the guard
            // See struct-level documentation for full safety invariants.
            if let Some(ref original) = self.original {
                unsafe {
                    std::env::set_var("APPDATA", original);
                }
            } else {
                unsafe {
                    std::env::remove_var("APPDATA");
                }
            }
        }
    }

    #[test]
    fn test_app_controller_creation() {
        let config = AppConfig::default();
        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let controller = AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list);
        assert!(controller.is_ok());
    }

    #[test]
    fn test_handle_process_started_increments_count() {
        // Create a config with one monitored app
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Initial count should be 0
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);

        // Handle a started event for the monitored app
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app".to_string(),
        )));

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
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Handle a started event with different case
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "APP".to_string(),
        )));

        // Count should be incremented (case-insensitive match)
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_handle_process_started_disabled_app_ignored() {
        // Create a config with one disabled app
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: false, // Disabled
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Handle a started event for the disabled app
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app".to_string(),
        )));

        // Count should remain 0 (disabled apps are ignored)
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_handle_process_stopped_decrements_count() {
        // Create a config with one monitored app
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Start the app first
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);

        // Clear the state update from start
        let _ = state_rx.try_recv();

        // Wait for debounce period to pass
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop the app
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "app".to_string(),
        )));

        // Count should be decremented to 0
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);

        // Should have sent a state update
        let state = state_rx.try_recv().unwrap();
        assert!(!state.hdr_enabled);
    }

    #[test]
    fn test_handle_process_stopped_when_count_is_zero() {
        // Create a config with one monitored app
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Initial count should be 0
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);

        // Send a spurious ProcessEvent::Stopped when count is already 0
        // This should NOT cause underflow (wrapping to usize::MAX)
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "app".to_string(),
        )));

        // Count should remain 0 (saturating_sub prevents underflow)
        assert_eq!(
            controller.active_process_count.load(Ordering::SeqCst),
            0,
            "Count should remain 0 and not wrap to usize::MAX"
        );

        // Send another spurious stop event to verify idempotency
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "app".to_string(),
        )));

        // Count should still be 0
        assert_eq!(
            controller.active_process_count.load(Ordering::SeqCst),
            0,
            "Multiple spurious stops should not corrupt state"
        );

        // Verify that normal operation still works after spurious events
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app".to_string(),
        )));
        assert_eq!(
            controller.active_process_count.load(Ordering::SeqCst),
            1,
            "Normal increment should work after spurious stops"
        );
    }

    #[test]
    fn test_handle_multiple_processes() {
        // Create a config with two monitored apps
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "App 1".to_string(),
            exe_path: PathBuf::from("C:\\test\\app1.exe"),
            process_name: "app1".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "App 2".to_string(),
            exe_path: PathBuf::from("C:\\test\\app2.exe"),
            process_name: "app2".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Start first app
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app1".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Start second app
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app2".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 2);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop first app - HDR should remain on
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "app1".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop second app - HDR should turn off
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "app2".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));
    }

    #[test]
    fn test_add_application() {
        // Isolate test environment to prevent writing to real config directory
        let temp_dir = create_test_dir();
        let _guard = AppdataGuard::new(&temp_dir);

        let config = AppConfig::default();
        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list.clone())
                .unwrap();

        // Create a new app to add
        let app = MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "New App".to_string(),
            exe_path: PathBuf::from("C:\\test\\newapp.exe"),
            process_name: "newapp".to_string(),
            enabled: true,
            icon_data: None,
        });

        // Add the application
        let result = controller.add_application(app.clone());
        assert!(result.is_ok());

        // Verify it was added to config
        let config = controller.config.lock();
        assert_eq!(config.monitored_apps.len(), 1);
        assert_eq!(config.monitored_apps[0].display_name(), "New App");
        drop(config);

        // Verify watch list was updated
        let watch_list_guard = watch_list.lock();
        assert_eq!(watch_list_guard.len(), 1);
        assert!(watch_list_guard.iter().any(|app| {
            if let MonitoredApp::Win32(win32_app) = app {
                win32_app.process_name == "newapp"
            } else {
                false
            }
        }));
    }

    #[test]
    fn test_remove_application() {
        // Isolate test environment to prevent writing to real config directory
        let temp_dir = create_test_dir();
        let _guard = AppdataGuard::new(&temp_dir);

        let mut config = AppConfig::default();
        let app_id = Uuid::new_v4();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: app_id,
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list.clone())
                .unwrap();

        // Remove the application
        let result = controller.remove_application(app_id);
        assert!(result.is_ok());

        // Verify it was removed from config
        let config = controller.config.lock();
        assert_eq!(config.monitored_apps.len(), 0);
        drop(config);

        // Verify watch list was updated
        let watch_list_guard = watch_list.lock();
        assert_eq!(watch_list_guard.len(), 0);
    }

    #[test]
    fn test_toggle_app_enabled() {
        // Isolate test environment to prevent writing to real config directory
        let temp_dir = create_test_dir();
        let _guard = AppdataGuard::new(&temp_dir);

        let mut config = AppConfig::default();
        let app_id = Uuid::new_v4();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: app_id,
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list.clone())
                .unwrap();

        // Initially populate watch list
        controller.update_process_monitor_watch_list();
        {
            let watch_list_guard = watch_list.lock();
            assert_eq!(watch_list_guard.len(), 1);
            assert!(watch_list_guard.iter().any(|app| {
                if let MonitoredApp::Win32(win32_app) = app {
                    win32_app.process_name == "app"
                } else {
                    false
                }
            }));
        }

        // Disable the application
        let result = controller.toggle_app_enabled(app_id, false);
        assert!(result.is_ok());

        // Verify enabled flag was updated
        let config = controller.config.lock();
        assert!(!config.monitored_apps[0].is_enabled());
        drop(config);

        // Verify watch list was updated (app should be removed)
        let watch_list_guard = watch_list.lock();
        assert_eq!(watch_list_guard.len(), 0);
        drop(watch_list_guard);

        // Re-enable the application
        let result = controller.toggle_app_enabled(app_id, true);
        assert!(result.is_ok());

        // Verify enabled flag was updated
        let config = controller.config.lock();
        assert!(config.monitored_apps[0].is_enabled());
        drop(config);

        // Verify watch list was updated (app should be added back)
        let watch_list_guard = watch_list.lock();
        assert_eq!(watch_list_guard.len(), 1);
        assert!(watch_list_guard.iter().any(|app| {
            if let MonitoredApp::Win32(win32_app) = app {
                win32_app.process_name == "app"
            } else {
                false
            }
        }));
    }

    #[test]
    fn test_update_preferences() {
        // Isolate test environment to prevent writing to real config directory
        let temp_dir = create_test_dir();
        let _guard = AppdataGuard::new(&temp_dir);

        let config = AppConfig::default();
        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Create new preferences
        let new_prefs = UserPreferences {
            auto_start: true,
            monitoring_interval_ms: 2000,
            show_tray_notifications: false,
            show_update_notifications: true,
            minimize_to_tray_on_minimize: true,
            minimize_to_tray_on_close: false,
            start_minimized_to_tray: false,
            last_update_check_time: 0,
            cached_latest_version: String::new(),
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
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "App 1".to_string(),
            exe_path: PathBuf::from("C:\\test\\app1.exe"),
            process_name: "app1".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "App 2".to_string(),
            exe_path: PathBuf::from("C:\\test\\app2.exe"),
            process_name: "app2".to_string(),
            enabled: false, // Disabled
            icon_data: None,
        }));
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "App 3".to_string(),
            exe_path: PathBuf::from("C:\\test\\app3.exe"),
            process_name: "app3".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list.clone())
                .unwrap();

        // Update watch list
        controller.update_process_monitor_watch_list();

        // Verify only enabled apps are in watch list
        let watch_list_guard = watch_list.lock();
        assert_eq!(watch_list_guard.len(), 2);
        assert!(watch_list_guard.iter().any(|app| {
            if let MonitoredApp::Win32(win32_app) = app {
                win32_app.process_name == "app1"
            } else {
                false
            }
        }));
        assert!(!watch_list_guard.iter().any(|app| {
            if let MonitoredApp::Win32(win32_app) = app {
                win32_app.process_name == "app2"
            } else {
                false
            }
        })); // Disabled
        assert!(watch_list_guard.iter().any(|app| {
            if let MonitoredApp::Win32(win32_app) = app {
                win32_app.process_name == "app3"
            } else {
                false
            }
        }));
    }

    #[test]
    fn test_run_processes_events() {
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Spawn thread to run the event loop
        let handle = std::thread::spawn(move || {
            controller.run();
        });

        // Send a process started event
        event_tx
            .send(ProcessEvent::Started(AppIdentifier::Win32(
                "app".to_string(),
            )))
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

        let (event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

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
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "App 1".to_string(),
            exe_path: PathBuf::from("C:\\test\\app1.exe"),
            process_name: "app1".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "App 2".to_string(),
            exe_path: PathBuf::from("C:\\test\\app2.exe"),
            process_name: "app2".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Spawn thread to run the event loop
        let handle = std::thread::spawn(move || {
            controller.run();
        });

        // Send multiple events
        event_tx
            .send(ProcessEvent::Started(AppIdentifier::Win32(
                "app1".to_string(),
            )))
            .unwrap();
        event_tx
            .send(ProcessEvent::Started(AppIdentifier::Win32(
                "app2".to_string(),
            )))
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

    /// Test rapid process start/stop with debouncing.
    /// Verifies 500ms debouncing prevents unnecessary HDR toggling during app restarts.
    #[test]
    fn test_rapid_process_restart_debouncing() {
        // Create a config with one monitored app
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Start the app - HDR should turn on
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Record the time of the first toggle
        let first_toggle_time = *controller.last_toggle_time.lock();

        // Wait a short time (less than 500ms)
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Stop the app - HDR should NOT turn off due to debouncing
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "app".to_string(),
        )));
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
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        // HDR should still be on (it never turned off)
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait for debounce period to expire (600ms to be safe)
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop the app - HDR should turn off now (debounce period has passed)
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "app".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));
    }

    /// Test that debouncing only affects HDR disable, not enable.
    /// HDR should always turn on immediately when a monitored app starts.
    #[test]
    fn test_debouncing_does_not_affect_hdr_enable() {
        // Create a config with one monitored app
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Test App".to_string(),
            exe_path: PathBuf::from("C:\\test\\app.exe"),
            process_name: "app".to_string(),
            enabled: true,
            icon_data: None,
        }));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Start the app - HDR should turn on immediately
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop the app - HDR should turn off
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "app".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));

        // Immediately start the app again (within what would be a debounce window if it applied to enable)
        // HDR should turn on immediately regardless of timing
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));
    }

    // ========================================================================================
    // UWP Application Tests
    // ========================================================================================

    /// Helper function to create a test `UwpApp`
    fn create_test_uwp_app(
        package_family_name: &str,
        display_name: &str,
        app_id: &str,
    ) -> MonitoredApp {
        use crate::config::models::UwpApp;
        MonitoredApp::Uwp(UwpApp {
            id: Uuid::new_v4(),
            display_name: display_name.to_string(),
            package_family_name: package_family_name.to_string(),
            app_id: app_id.to_string(),
            enabled: true,
            icon_data: None,
        })
    }

    /// Test that `AppController` correctly handles UWP application started event.
    /// Verifies requirement 3.1: UWP app starts and HDR enables.
    #[test]
    fn test_handle_uwp_app_started() {
        let mut config = AppConfig::default();
        config.monitored_apps.push(create_test_uwp_app(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe",
            "Calculator",
            "App",
        ));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Initial count should be 0
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);

        // Handle a started event for the UWP app
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));

        // Count should be incremented to 1
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);

        // HDR should be enabled
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Should have sent a state update
        let state = state_rx.try_recv().unwrap();
        assert!(state.hdr_enabled);
    }

    /// Test that `AppController` correctly handles UWP application stopped event.
    /// Verifies requirement 3.2: Last UWP app stops and HDR disables after debounce.
    #[test]
    fn test_handle_uwp_app_stopped() {
        let mut config = AppConfig::default();
        config.monitored_apps.push(create_test_uwp_app(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe",
            "Calculator",
            "App",
        ));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Start the UWP app first
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Clear the state update from start
        let _ = state_rx.try_recv();

        // Wait for debounce period to pass
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop the UWP app
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));

        // Count should be decremented to 0
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);

        // HDR should be disabled
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));

        // Should have sent a state update
        let state = state_rx.try_recv().unwrap();
        assert!(!state.hdr_enabled);
    }

    /// Test that `AppController` handles both Win32 and UWP apps running simultaneously.
    /// Verifies requirement 3.3: Both Win32 and UWP monitored apps running maintains HDR enabled.
    #[test]
    fn test_handle_mixed_win32_and_uwp_apps_simultaneously() {
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Notepad".to_string(),
            exe_path: PathBuf::from("C:\\Windows\\notepad.exe"),
            process_name: "notepad".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.monitored_apps.push(create_test_uwp_app(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe",
            "Calculator",
            "App",
        ));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Start Win32 app
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "notepad".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Start UWP app while Win32 app is running
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 2);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop Win32 app - HDR should remain on because UWP app is still running
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "notepad".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(
            controller.current_hdr_state.load(Ordering::SeqCst),
            "HDR should remain enabled when UWP app is still running"
        );

        // Wait for debounce period
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop UWP app - HDR should turn off
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));
    }

    /// Test that the last monitored application stops regardless of type.
    /// Verifies requirement 3.4: Last app stops (regardless of Win32/UWP) disables HDR after debounce.
    #[test]
    fn test_last_app_stops_regardless_of_type() {
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Notepad".to_string(),
            exe_path: PathBuf::from("C:\\Windows\\notepad.exe"),
            process_name: "notepad".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.monitored_apps.push(create_test_uwp_app(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe",
            "Calculator",
            "App",
        ));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Scenario 1: Start UWP app, then Win32 app, then stop UWP app (Win32 is last)
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "notepad".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 2);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop UWP app - HDR should remain on
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop Win32 app (last app) - HDR should turn off
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "notepad".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));

        // Wait to reset state
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Scenario 2: Start Win32 app, then UWP app, then stop Win32 app (UWP is last)
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "notepad".to_string(),
        )));
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 2);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop Win32 app - HDR should remain on
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "notepad".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        std::thread::sleep(std::time::Duration::from_millis(600));

        // Stop UWP app (last app) - HDR should turn off
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));
    }

    /// Test that counter-based logic works identically for Win32 and UWP apps.
    /// Verifies requirement 3.5: `AppController` uses same counter-based logic for both app types.
    #[test]
    fn test_counter_based_logic_works_for_both_app_types() {
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "App 1".to_string(),
            exe_path: PathBuf::from("C:\\test\\app1.exe"),
            process_name: "app1".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.monitored_apps.push(create_test_uwp_app(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe",
            "Calculator",
            "App",
        ));

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Test that both app types increment the counter
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app1".to_string(),
        )));
        assert_eq!(
            controller.active_process_count.load(Ordering::SeqCst),
            1,
            "Win32 app should increment counter"
        );

        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));
        assert_eq!(
            controller.active_process_count.load(Ordering::SeqCst),
            2,
            "UWP app should increment counter"
        );

        // Verify HDR is on with both apps running
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        std::thread::sleep(std::time::Duration::from_millis(600));

        // Test that both app types decrement the counter
        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Win32(
            "app1".to_string(),
        )));
        assert_eq!(
            controller.active_process_count.load(Ordering::SeqCst),
            1,
            "Win32 app should decrement counter"
        );

        // HDR should still be on
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        std::thread::sleep(std::time::Duration::from_millis(600));

        controller.handle_process_event(ProcessEvent::Stopped(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));
        assert_eq!(
            controller.active_process_count.load(Ordering::SeqCst),
            0,
            "UWP app should decrement counter"
        );

        // HDR should be off now
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));
    }

    /// Test that disabled UWP apps are ignored by the `AppController`.
    #[test]
    fn test_disabled_uwp_app_ignored() {
        let mut config = AppConfig::default();
        let mut uwp_app = create_test_uwp_app(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe",
            "Calculator",
            "App",
        );
        // Disable the app
        if let MonitoredApp::Uwp(ref mut app) = uwp_app {
            app.enabled = false;
        }
        config.monitored_apps.push(uwp_app);

        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_list = Arc::new(Mutex::new(Vec::new()));

        let mut controller =
            AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list).unwrap();

        // Handle a started event for the disabled UWP app
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Uwp(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        )));

        // Count should remain 0 (disabled apps are ignored)
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 0);
        // HDR should remain off
        assert!(!controller.current_hdr_state.load(Ordering::SeqCst));
    }
}
