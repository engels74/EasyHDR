//! Application controller implementation.

use crate::config::{AppConfig, ConfigManager, MonitoredApp, UserPreferences};
use crate::error::{EasyHdrError, Result};
use crate::hdr::HdrController;
use crate::monitor::{AppIdentifier, HdrStateEvent, ProcessEvent, WatchState};
use parking_lot::{Mutex, RwLock};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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
    /// Flag to show "HDR displays now available" notification
    ///
    /// Set to true when HDR displays become available after being unavailable.
    /// GUI should show notification and then clear this flag.
    pub show_hdr_available_notification: bool,
    /// Flag to show startup warning that no HDR displays were found
    ///
    /// Set to true on first state update if no HDR displays were detected at startup.
    /// GUI should show notification and then clear this flag.
    pub show_no_hdr_warning: bool,
}

/// Application logic controller
pub struct AppController {
    /// Application configuration (public for GUI access)
    pub config: Arc<RwLock<AppConfig>>,
    hdr_controller: HdrController,
    active_process_count: AtomicUsize,
    current_hdr_state: AtomicBool,
    /// Taken when event loop starts
    event_receiver: Option<mpsc::Receiver<ProcessEvent>>,
    /// Taken when event loop starts
    hdr_state_receiver: Option<mpsc::Receiver<HdrStateEvent>>,
    gui_state_sender: mpsc::SyncSender<AppState>,
    /// Reference point for atomic timestamp operations (nanoseconds stored in `last_toggle_time_nanos`)
    startup_time: Instant,
    /// Nanoseconds elapsed since `startup_time` for debouncing
    last_toggle_time_nanos: Arc<AtomicU64>,
    /// Shared watch state with `ProcessMonitor` for atomic updates
    watch_state: Arc<RwLock<WatchState>>,
    /// Tracks whether HDR displays are currently available
    ///
    /// Used to detect when HDR displays become available after being unavailable,
    /// allowing the app to show a notification and enable HDR toggling.
    hdr_displays_available: AtomicBool,
    /// Flag to show HDR available notification on next state update
    ///
    /// Set when HDR displays become available, cleared after notification is sent.
    pending_hdr_available_notification: AtomicBool,
    /// Flag to show startup warning on first state update
    ///
    /// Set if no HDR displays were detected at startup, cleared after notification is sent.
    pending_no_hdr_warning: AtomicBool,
}

impl AppController {
    /// Create a new application controller and detect initial HDR state
    pub fn new(
        config: AppConfig,
        event_receiver: mpsc::Receiver<ProcessEvent>,
        hdr_state_receiver: mpsc::Receiver<HdrStateEvent>,
        gui_state_sender: mpsc::SyncSender<AppState>,
        watch_state: Arc<RwLock<WatchState>>,
    ) -> Result<Self> {
        use tracing::info;

        let hdr_controller = HdrController::new().map_err(|e| {
            use tracing::error;
            error!("Failed to initialize HDR controller: {e}");
            EasyHdrError::HdrControlFailed(Box::new(e))
        })?;

        let initial_hdr_state = Self::detect_current_hdr_state(&hdr_controller);
        info!("Detected initial HDR state: {}", initial_hdr_state);

        let startup_time = Instant::now();

        // Check if HDR displays are available at startup
        let hdr_displays_available = hdr_controller
            .get_display_cache()
            .iter()
            .any(|d| d.supports_hdr);

        // If no HDR displays found at startup, schedule a warning notification
        let show_startup_warning = !hdr_displays_available;

        let controller = Self {
            config: Arc::new(RwLock::new(config)),
            hdr_controller,
            active_process_count: AtomicUsize::new(0),
            current_hdr_state: AtomicBool::new(initial_hdr_state),
            event_receiver: Some(event_receiver),
            hdr_state_receiver: Some(hdr_state_receiver),
            gui_state_sender,
            startup_time,
            last_toggle_time_nanos: Arc::new(AtomicU64::new(0)),
            watch_state,
            hdr_displays_available: AtomicBool::new(hdr_displays_available),
            pending_hdr_available_notification: AtomicBool::new(false),
            pending_no_hdr_warning: AtomicBool::new(show_startup_warning),
        };

        controller.update_process_monitor_watch_list();

        Ok(controller)
    }

    /// Create a new application controller with mock HDR controller
    ///
    /// **For test use only** - uses mock HDR controller to avoid Windows API dependencies.
    #[doc(hidden)]
    pub fn new_with_mock_hdr(
        config: AppConfig,
        event_receiver: mpsc::Receiver<ProcessEvent>,
        hdr_state_receiver: mpsc::Receiver<HdrStateEvent>,
        gui_state_sender: mpsc::SyncSender<AppState>,
        watch_state: Arc<RwLock<WatchState>>,
    ) -> Result<Self> {
        use tracing::info;

        let hdr_controller = HdrController::new_mock().map_err(|e| {
            use tracing::error;
            error!("Failed to initialize mock HDR controller: {e}");
            EasyHdrError::HdrControlFailed(Box::new(e))
        })?;

        let initial_hdr_state = Self::detect_current_hdr_state(&hdr_controller);
        info!("AppController initialized with mock HDR controller (test mode)");

        let startup_time = Instant::now();

        // Mock controller has empty display cache
        let hdr_displays_available = hdr_controller
            .get_display_cache()
            .iter()
            .any(|d| d.supports_hdr);

        let controller = Self {
            config: Arc::new(RwLock::new(config)),
            hdr_controller,
            active_process_count: AtomicUsize::new(0),
            current_hdr_state: AtomicBool::new(initial_hdr_state),
            event_receiver: Some(event_receiver),
            hdr_state_receiver: Some(hdr_state_receiver),
            gui_state_sender,
            startup_time,
            last_toggle_time_nanos: Arc::new(AtomicU64::new(0)),
            watch_state,
            hdr_displays_available: AtomicBool::new(hdr_displays_available),
            pending_hdr_available_notification: AtomicBool::new(false),
            // Don't show warning for mock controller (test mode)
            pending_no_hdr_warning: AtomicBool::new(false),
        };

        controller.update_process_monitor_watch_list();

        Ok(controller)
    }

    /// Detect the current HDR state from the system by checking all HDR-capable displays.
    fn detect_current_hdr_state(hdr_controller: &HdrController) -> bool {
        hdr_controller.detect_current_hdr_state()
    }

    /// Take ownership of the event receiver if it hasn't been taken yet.
    fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<ProcessEvent>> {
        self.event_receiver.take()
    }

    /// Take ownership of the HDR state receiver if it hasn't been taken yet.
    fn take_hdr_state_receiver(&mut self) -> Option<mpsc::Receiver<HdrStateEvent>> {
        self.hdr_state_receiver.take()
    }

    /// Core event loop logic shared between `run()` and `spawn_event_loop()`.
    ///
    /// Processes events from both process and HDR state receivers, calling the
    /// provided handlers for each event type. Returns `false` when the process
    /// event channel disconnects, signaling the loop should exit.
    fn process_event_loop_iteration<F, G>(
        event_receiver: &mpsc::Receiver<ProcessEvent>,
        hdr_state_receiver: &mpsc::Receiver<HdrStateEvent>,
        process_handler: &mut F,
        hdr_handler: &mut G,
    ) -> bool
    where
        F: FnMut(ProcessEvent),
        G: FnMut(HdrStateEvent),
    {
        use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
        use std::time::Duration;
        use tracing::warn;

        match event_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => process_handler(event),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                warn!("Process event receiver channel disconnected. Exiting event loop.");
                return false;
            }
        }

        loop {
            match hdr_state_receiver.try_recv() {
                Ok(event) => hdr_handler(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    warn!("HDR state receiver channel disconnected.");
                    break;
                }
            }
        }

        true
    }

    /// Run the main event loop to receive process and HDR state events.
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

        info!("Entering main event loop (process events + HDR state events)");
        loop {
            match event_receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(event) => {
                    self.handle_process_event(event);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    warn!("Process event receiver channel disconnected. Exiting event loop.");
                    break;
                }
            }

            loop {
                match hdr_state_receiver.try_recv() {
                    Ok(event) => {
                        self.handle_hdr_state_event(event);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        warn!("HDR state receiver channel disconnected.");
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
            use tracing::info;

            info!("Entering main event loop (process events + HDR state events)");
            while Self::process_event_loop_iteration(
                &event_receiver,
                &hdr_state_receiver,
                &mut |event| {
                    let mut controller_guard = controller.lock();
                    controller_guard.handle_process_event(event);
                },
                &mut |event| {
                    let mut controller_guard = controller.lock();
                    controller_guard.handle_hdr_state_event(event);
                },
            ) {}
            info!("Main event loop exited");
        })
    }

    /// Handle a process event to automatically toggle HDR.
    ///
    /// Enables HDR when first monitored app starts, disables when last one stops.
    /// Uses 500ms debouncing to prevent rapid toggling during app restarts.
    fn handle_process_event(&mut self, event: ProcessEvent) {
        use tracing::{debug, error, info};

        match event {
            ProcessEvent::Started(app_id) => {
                debug!("Process started event: {:?}", app_id);

                let normalized_id = Self::normalize_app_identifier(&app_id);
                let state = self.watch_state.read();
                let is_monitored = state.identifiers.contains(&normalized_id);
                drop(state);

                if is_monitored {
                    match &app_id {
                        AppIdentifier::Win32(process_name) => {
                            info!("Monitored Win32 application started: {}", process_name);
                        }
                        AppIdentifier::Uwp(package_family_name) => {
                            info!("Monitored UWP application started: {}", package_family_name);
                        }
                    }

                    let prev_count = self.active_process_count.fetch_add(1, Ordering::SeqCst);
                    debug!("Active process count: {} -> {}", prev_count, prev_count + 1);

                    if prev_count == 0 && !self.current_hdr_state.load(Ordering::SeqCst) {
                        info!("First monitored application started, enabling HDR");
                        if let Err(e) = self.toggle_hdr(true) {
                            error!("Failed to enable HDR: {}", e);
                        }
                    } else {
                        debug!("HDR already enabled or other processes running, skipping toggle");
                    }

                    self.send_state_update();
                }
            }

            ProcessEvent::Stopped(app_id) => {
                debug!("Process stopped event: {:?}", app_id);

                let normalized_id = Self::normalize_app_identifier(&app_id);
                let state = self.watch_state.read();
                let is_monitored = state.identifiers.contains(&normalized_id);
                drop(state);

                if is_monitored {
                    match &app_id {
                        AppIdentifier::Win32(process_name) => {
                            info!("Monitored Win32 application stopped: {}", process_name);
                        }
                        AppIdentifier::Uwp(package_family_name) => {
                            info!("Monitored UWP application stopped: {}", package_family_name);
                        }
                    }

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

                    let last_toggle_nanos = self.last_toggle_time_nanos.load(Ordering::Relaxed);
                    let last_toggle =
                        self.startup_time + std::time::Duration::from_nanos(last_toggle_nanos);
                    if last_toggle.elapsed() < std::time::Duration::from_millis(500) {
                        debug!(
                            "Debouncing: last toggle was less than 500ms ago, skipping HDR disable"
                        );
                        return;
                    }

                    if prev_count == 1 && self.current_hdr_state.load(Ordering::SeqCst) {
                        info!("Last monitored application stopped, disabling HDR");
                        if let Err(e) = self.toggle_hdr(false) {
                            error!("Failed to disable HDR: {}", e);
                        }
                    } else {
                        debug!("Other processes still running or HDR already off, skipping toggle");
                    }

                    self.send_state_update();
                }
            }
        }
    }

    /// Handle an HDR state event from external Windows settings changes.
    ///
    /// Updates internal state and GUI without calling `toggle_hdr()` since the change already occurred.
    fn handle_hdr_state_event(&mut self, event: HdrStateEvent) {
        use tracing::{debug, info, warn};

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
            HdrStateEvent::DisplayConfigurationChanged { hdr_capable_count } => {
                info!(
                    "Display configuration changed: {} HDR-capable display(s) detected",
                    hdr_capable_count
                );

                // Refresh local display cache
                if let Err(e) = self.refresh_displays() {
                    warn!("Failed to refresh display cache: {}", e);
                }

                // Track state transition for notification
                let was_unavailable = !self.hdr_displays_available.load(Ordering::SeqCst);
                let now_available = hdr_capable_count > 0;
                self.hdr_displays_available
                    .store(now_available, Ordering::SeqCst);

                // Show notification when HDR displays become available after being unavailable
                if was_unavailable && now_available {
                    info!("HDR displays now available - scheduling notification");
                    self.pending_hdr_available_notification
                        .store(true, Ordering::SeqCst);
                }

                // If HDR displays are now available and we have active monitored processes,
                // attempt to enable HDR
                let active_count = self.active_process_count.load(Ordering::SeqCst);
                let current_hdr = self.current_hdr_state.load(Ordering::SeqCst);

                if now_available && active_count > 0 && !current_hdr {
                    info!(
                        "HDR displays now available with {} active monitored process(es), enabling HDR",
                        active_count
                    );
                    if let Err(e) = self.toggle_hdr(true) {
                        warn!(
                            "Failed to enable HDR after display configuration change: {}",
                            e
                        );
                    }
                }
            }
        }

        self.send_state_update();
    }

    /// Toggle HDR state globally on all displays and update debouncing timestamp.
    fn toggle_hdr(&mut self, enable: bool) -> Result<()> {
        use tracing::{info, warn};

        info!("Toggling HDR: {}", if enable { "ON" } else { "OFF" });

        let results = self.hdr_controller.set_hdr_global(enable).map_err(|e| {
            use tracing::error;
            error!("Failed to set HDR state globally: {e}");
            EasyHdrError::HdrControlFailed(Box::new(e))
        })?;

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

        self.current_hdr_state.store(enable, Ordering::SeqCst);

        #[expect(
            clippy::cast_possible_truncation,
            reason = "Elapsed nanos will not exceed u64::MAX within application lifetime"
        )]
        let elapsed_nanos = self.startup_time.elapsed().as_nanos() as u64;
        self.last_toggle_time_nanos
            .store(elapsed_nanos, Ordering::Relaxed);

        Ok(())
    }

    /// Send current state update to GUI.
    fn send_state_update(&self) {
        use tracing::{debug, warn};

        let config = self.config.read();
        let active_apps: Vec<String> = config
            .monitored_apps
            .iter()
            .filter(|app| app.is_enabled())
            .map(|app| app.display_name().to_string())
            .collect();
        drop(config);

        let hdr_enabled = self.current_hdr_state.load(Ordering::SeqCst);

        // Atomically get and clear the pending notification flags
        let show_hdr_available_notification = self
            .pending_hdr_available_notification
            .swap(false, Ordering::SeqCst);
        let show_no_hdr_warning = self.pending_no_hdr_warning.swap(false, Ordering::SeqCst);

        let state = AppState {
            hdr_enabled,
            active_apps,
            last_event: format!(
                "Active processes: {}",
                self.active_process_count.load(Ordering::SeqCst)
            ),
            show_hdr_available_notification,
            show_no_hdr_warning,
        };

        debug!(
            "Sending state update to GUI: HDR enabled = {}, show HDR available notification = {}, show no HDR warning = {}",
            hdr_enabled, show_hdr_available_notification, show_no_hdr_warning
        );

        if let Err(e) = self.gui_state_sender.send(state) {
            warn!("Failed to send state update to GUI: {}", e);
        } else {
            debug!("State update sent successfully to GUI");
        }
    }

    /// Send initial state to GUI and populate `ProcessMonitor` watch list.
    pub fn send_initial_state(&self) {
        use tracing::info;

        info!("Sending initial state update to populate GUI");
        self.update_process_monitor_watch_list();

        self.send_state_update();
    }

    /// Add application to config, save to disk, and update `ProcessMonitor` watch list.
    /// Logs warning and continues with in-memory config if save fails.
    pub fn add_application(&mut self, app: MonitoredApp) -> Result<()> {
        use tracing::info;

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
            let mut config = self.config.write();
            config.monitored_apps.push(app);
        }

        // Save configuration - if this fails, we continue with in-memory config
        self.save_config_gracefully();

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

        {
            let mut config = self.config.write();
            config.monitored_apps.retain(|app| app.id() != &id);
        }

        if let Ok(cache) = crate::utils::icon_cache::IconCache::new(
            crate::utils::icon_cache::IconCache::default_cache_dir(),
        ) {
            if let Err(e) = cache.remove_icon(id) {
                warn!("Failed to remove cached icon for app {}: {}", id, e);
            }
        } else {
            warn!("Failed to initialize icon cache for cleanup of app {}", id);
        }

        self.save_config_gracefully();
        self.update_process_monitor_watch_list();
        self.send_state_update();

        info!("Application removed successfully");
        Ok(())
    }

    /// Toggle application enabled state by UUID, save to disk, and update `ProcessMonitor` watch list.
    /// Logs warning and continues with in-memory config if save fails.
    pub fn toggle_app_enabled(&mut self, id: Uuid, enabled: bool) -> Result<()> {
        use tracing::info;

        info!("Toggling application {} to enabled={}", id, enabled);

        {
            let mut config = self.config.write();
            if let Some(app) = config.monitored_apps.iter_mut().find(|app| app.id() == &id) {
                app.set_enabled(enabled);
            }
        }

        self.save_config_gracefully();
        self.update_process_monitor_watch_list();
        self.send_state_update();

        info!("Application enabled state updated successfully");
        Ok(())
    }

    /// Update user preferences and save to disk.
    /// Logs warning and continues with in-memory config if save fails.
    pub fn update_preferences(&mut self, prefs: UserPreferences) -> Result<()> {
        use tracing::info;

        info!("Updating user preferences");

        {
            let mut config = self.config.write();
            config.preferences = prefs;
        }

        self.save_config_gracefully();

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
            EasyHdrError::HdrControlFailed(Box::new(e))
        })?;
        info!(
            "Display list refreshed: {} display(s) found ({} HDR-capable)",
            displays.len(),
            displays.iter().filter(|d| d.supports_hdr).count()
        );
        Ok(())
    }

    /// Save configuration with graceful error handling
    ///
    /// Attempts to save the current configuration to disk. Failures are logged
    /// but do not propagate errors, allowing the application to continue with
    /// in-memory configuration. This implements graceful degradation for config
    /// persistence.
    ///
    /// # Design
    ///
    /// This helper consolidates the config save pattern used across multiple
    /// methods in `AppController`, eliminating ~20 lines of duplication.
    fn save_config_gracefully(&self) {
        use tracing::warn;

        let config = self.config.read();
        if let Err(e) = ConfigManager::save(&config) {
            warn!(
                "Failed to save configuration to disk: {}. Continuing with in-memory config. \
                 Changes will be lost on application restart.",
                e
            );
        }
    }

    /// Normalize `AppIdentifier` for case-insensitive matching.
    ///
    /// Win32 process names are normalized to lowercase. UWP package family names are case-sensitive.
    fn normalize_app_identifier(app_id: &AppIdentifier) -> AppIdentifier {
        match app_id {
            AppIdentifier::Win32(process_name) => AppIdentifier::Win32(process_name.to_lowercase()),
            AppIdentifier::Uwp(package_family_name) => {
                AppIdentifier::Uwp(package_family_name.clone())
            }
        }
    }

    /// Update `ProcessMonitor` watch list with enabled monitored applications from config.
    fn update_process_monitor_watch_list(&self) {
        use tracing::debug;

        let config = self.config.read();
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

        let identifiers: HashSet<AppIdentifier> = monitored_apps
            .iter()
            .map(|app| match app {
                MonitoredApp::Win32(win32_app) => {
                    AppIdentifier::Win32(win32_app.process_name.to_lowercase())
                }
                MonitoredApp::Uwp(uwp_app) => {
                    AppIdentifier::Uwp(uwp_app.package_family_name.clone())
                }
            })
            .collect();

        let mut state = self.watch_state.write();
        *state = WatchState {
            apps: Arc::new(monitored_apps),
            identifiers,
        };

        debug!("ProcessMonitor watch state updated atomically");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::models::Win32App;
    use crate::config::{AppConfig, MonitoredApp};
    use crate::test_utils::{AppdataGuard, create_test_dir};
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn test_app_controller_creation() {
        let config = AppConfig::default();
        let (_event_tx, event_rx) = mpsc::sync_channel(32);
        let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
        let (state_tx, _state_rx) = mpsc::sync_channel(32);
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        );
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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
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
        let config = controller.config.read();
        assert_eq!(config.monitored_apps.len(), 1);
        assert_eq!(config.monitored_apps[0].display_name(), "New App");
        drop(config);

        // Verify watch list was updated
        let watch_state_guard = watch_state.read();
        assert_eq!(watch_state_guard.apps.len(), 1);
        assert!(watch_state_guard.apps.iter().any(|app| {
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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

        // Remove the application
        let result = controller.remove_application(app_id);
        assert!(result.is_ok());

        // Verify it was removed from config
        let config = controller.config.read();
        assert_eq!(config.monitored_apps.len(), 0);
        drop(config);

        // Verify watch list was updated
        let watch_state_guard = watch_state.read();
        assert_eq!(watch_state_guard.apps.len(), 0);
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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

        // Initially populate watch list
        controller.update_process_monitor_watch_list();
        {
            let watch_state_guard = watch_state.read();
            assert_eq!(watch_state_guard.apps.len(), 1);
            assert!(watch_state_guard.apps.iter().any(|app| {
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
        let config = controller.config.read();
        assert!(!config.monitored_apps[0].is_enabled());
        drop(config);

        // Verify watch list was updated (app should be removed)
        let watch_state_guard = watch_state.read();
        assert_eq!(watch_state_guard.apps.len(), 0);
        drop(watch_state_guard);

        // Re-enable the application
        let result = controller.toggle_app_enabled(app_id, true);
        assert!(result.is_ok());

        // Verify enabled flag was updated
        let config = controller.config.read();
        assert!(config.monitored_apps[0].is_enabled());
        drop(config);

        // Verify watch list was updated (app should be added back)
        let watch_state_guard = watch_state.read();
        assert_eq!(watch_state_guard.apps.len(), 1);
        assert!(watch_state_guard.apps.iter().any(|app| {
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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

        // Create new preferences
        let new_prefs = UserPreferences {
            auto_start: true,
            monitoring_interval_ms: 2000,
            show_tray_notifications: false,
            show_update_notifications: true,
            auto_open_release_page: false,
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
        let config = controller.config.read();
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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

        // Update watch list
        controller.update_process_monitor_watch_list();

        // Verify only enabled apps are in watch list
        let watch_state_guard = watch_state.read();
        assert_eq!(watch_state_guard.apps.len(), 2);
        assert!(watch_state_guard.apps.iter().any(|app| {
            if let MonitoredApp::Win32(win32_app) = app {
                win32_app.process_name == "app1"
            } else {
                false
            }
        }));
        assert!(!watch_state_guard.apps.iter().any(|app| {
            if let MonitoredApp::Win32(win32_app) = app {
                win32_app.process_name == "app2"
            } else {
                false
            }
        })); // Disabled
        assert!(watch_state_guard.apps.iter().any(|app| {
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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

        // Start the app - HDR should turn on
        controller.handle_process_event(ProcessEvent::Started(AppIdentifier::Win32(
            "app".to_string(),
        )));
        assert_eq!(controller.active_process_count.load(Ordering::SeqCst), 1);
        assert!(controller.current_hdr_state.load(Ordering::SeqCst));

        let first_toggle_nanos = controller.last_toggle_time_nanos.load(Ordering::Relaxed);

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
        let second_toggle_nanos = controller.last_toggle_time_nanos.load(Ordering::Relaxed);
        assert_eq!(
            first_toggle_nanos, second_toggle_nanos,
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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
        let watch_state = Arc::new(RwLock::new(WatchState::new()));

        let mut controller = AppController::new(
            config,
            event_rx,
            hdr_state_rx,
            state_tx,
            watch_state.clone(),
        )
        .unwrap();

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
