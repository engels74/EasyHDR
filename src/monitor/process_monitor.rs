//! Process monitoring implementation.
//!
//! Polls Windows processes and detects state changes. Matches by executable filename only
//! (lowercase, no extension). Name collisions trigger HDR for all matching processes.

use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// Ordering used for poll_cycle_count atomic operations (test diagnostics)
use std::sync::atomic::Ordering;

#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES};

#[cfg(windows)]
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

use crate::config::MonitoredApp;
use crate::error::{EasyHdrError, Result};

/// Identifier for a monitored application
///
/// Distinguishes between Win32 desktop applications and UWP applications.
/// Win32 apps are identified by their process name (lowercase, no extension),
/// while UWP apps are identified by their package family name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AppIdentifier {
    /// Win32 application identified by process name (lowercase, no extension)
    Win32(String),
    /// UWP application identified by package family name
    Uwp(String),
}

impl std::fmt::Display for AppIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Win32(name) => write!(f, "Win32: {name}"),
            Self::Uwp(family_name) => write!(f, "UWP: {family_name}"),
        }
    }
}

/// Events emitted by the process monitor
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    /// A monitored process has started
    Started(AppIdentifier),
    /// A monitored process has stopped
    Stopped(AppIdentifier),
}

/// Combined watch list and identifier cache for atomic state updates
///
/// Groups the monitored applications list and the identifier cache into a single
/// structure that can be updated atomically. This prevents race conditions where
/// `poll_processes()` could observe an inconsistent state between the two caches.
///
/// The `apps` field is wrapped in `Arc` to enable cheap cloning during event handling,
/// while `identifiers` is owned directly for O(1) lookups.
#[derive(Clone, Debug)]
pub struct WatchState {
    /// Monitored applications (Arc-wrapped for cheap cloning during event handling)
    pub apps: Arc<Vec<MonitoredApp>>,
    /// Cached set of monitored app identifiers for O(1) filtering
    pub identifiers: HashSet<AppIdentifier>,
}

impl Default for WatchState {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchState {
    /// Create an empty `WatchState`
    pub fn new() -> Self {
        Self {
            apps: Arc::new(Vec::new()),
            identifiers: HashSet::new(),
        }
    }
}

/// Process monitor that polls for running processes.
///
/// Matches processes by executable filename only (without path or extension).
pub struct ProcessMonitor {
    /// Combined watch list and identifier cache for atomic state updates.
    ///
    /// Prevents race conditions by updating both caches simultaneously. Shared with
    /// `AppController` via `Arc` for coordinated updates.
    watch_state: Arc<RwLock<WatchState>>,
    /// PID → (`AppIdentifier`, timestamp) cache; expires after 5s to handle PID reuse
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "Field used only on Windows for process detection")
    )]
    app_id_cache: HashMap<u32, (AppIdentifier, Instant)>,
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "Field used only on Windows for process event dispatch"
        )
    )]
    event_sender: mpsc::SyncSender<ProcessEvent>,
    interval: Duration,
    /// Previous snapshot for change detection
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "Field used only on Windows for change detection")
    )]
    running_processes: HashSet<AppIdentifier>,
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "Field used only on Windows for capacity estimation"
        )
    )]
    estimated_process_count: usize,
    /// Poll cycles completed (test/diagnostic counter)
    poll_cycle_count: Arc<AtomicU64>,
}

impl ProcessMonitor {
    /// Create a new process monitor with the specified polling interval
    pub fn new(interval: Duration, event_sender: mpsc::SyncSender<ProcessEvent>) -> Self {
        // Typical Windows system has 150-250 processes
        const DEFAULT_PROCESS_COUNT: usize = 200;

        Self {
            watch_state: Arc::new(RwLock::new(WatchState::new())),
            app_id_cache: HashMap::with_capacity(DEFAULT_PROCESS_COUNT),
            event_sender,
            interval,
            running_processes: HashSet::with_capacity(DEFAULT_PROCESS_COUNT),
            estimated_process_count: DEFAULT_PROCESS_COUNT,
            poll_cycle_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Update the list of monitored applications to watch.
    ///
    /// Only enabled applications should be passed. Performs atomic update of both app list
    /// and identifier cache to prevent race conditions.
    pub fn update_watch_list(&self, monitored_apps: Vec<MonitoredApp>) {
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
    }

    /// Get a reference to the watch state for external updates.
    pub fn get_watch_state_ref(&self) -> Arc<RwLock<WatchState>> {
        Arc::clone(&self.watch_state)
    }

    /// Get the number of completed poll cycles
    ///
    /// Used for testing and diagnostics to verify the monitor is actively polling.
    #[doc(hidden)]
    pub fn get_poll_cycle_count(&self) -> u64 {
        self.poll_cycle_count.load(Ordering::Relaxed)
    }

    /// Get a reference to the poll cycle counter for external monitoring
    ///
    /// # Test-Only API
    ///
    /// Hidden from documentation as this is internal test infrastructure.
    /// Integration tests are compiled as separate crates, so `cfg(test)` doesn't apply.
    /// This method is always compiled but hidden from docs to signal test-only usage.
    #[doc(hidden)]
    pub fn get_poll_cycle_count_ref(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.poll_cycle_count)
    }

    /// Start the monitoring thread
    pub fn start(mut self) -> JoinHandle<()> {
        thread::spawn(move || {
            loop {
                if let Err(e) = self.poll_processes() {
                    tracing::error!("Error polling processes: {}", e);
                }
                thread::sleep(self.interval);
            }
        })
    }

    /// Poll processes and detect changes.
    ///
    /// # Safety
    ///
    /// `CreateToolhelp32Snapshot` called with valid flags. Handle wrapped in `SnapshotGuard` (RAII).
    /// `PROCESSENTRY32W` initialized with correct `dwSize`. Return codes checked before data access.
    #[cfg_attr(
        windows,
        expect(
            unsafe_code,
            reason = "Windows FFI for process enumeration via CreateToolhelp32Snapshot and Process32FirstW/NextW"
        )
    )]
    #[cfg_attr(
        not(windows),
        expect(
            clippy::unused_self,
            reason = "self is used on Windows but not in non-Windows stub implementation"
        )
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "Complex polling logic with UWP detection requires extended function body"
    )]
    fn poll_processes(&mut self) -> Result<()> {
        #[cfg(windows)]
        {
            use tracing::{debug, warn};

            const CACHE_EXPIRY: Duration = Duration::from_secs(5);
            let now = Instant::now();
            self.app_id_cache
                .retain(|_pid, (_app_id, last_seen)| now.duration_since(*last_seen) < CACHE_EXPIRY);

            let mut cache_hits = 0usize;
            let mut cache_misses = 0usize;

            let snapshot = unsafe {
                CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| {
                    use tracing::error;
                    error!("Windows API error - CreateToolhelp32Snapshot failed: {e}");
                    EasyHdrError::ProcessMonitorError(Box::new(e))
                })?
            };

            let _guard = SnapshotGuard(snapshot);

            let capacity = self
                .running_processes
                .len()
                .max(self.estimated_process_count);
            let mut current_processes = HashSet::with_capacity(capacity);

            #[expect(
                clippy::cast_possible_truncation,
                reason = "size_of::<PROCESSENTRY32W>() is a compile-time constant (592 bytes) that fits in u32"
            )]
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };

            let mut has_process = unsafe { Process32FirstW(snapshot, &raw mut entry).is_ok() };

            while has_process {
                let pid = entry.th32ProcessID;

                let handle_result =
                    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };

                if let Some((cached_app_id, _)) = self.app_id_cache.get(&pid) {
                    cache_hits += 1;
                    if self.watch_state.read().identifiers.contains(cached_app_id) {
                        current_processes.insert(cached_app_id.clone());
                        self.app_id_cache.insert(pid, (cached_app_id.clone(), now));
                    }
                } else {
                    cache_misses += 1;

                    match handle_result {
                        Ok(handle) => {
                            let _guard = ProcessHandleGuard(handle);

                            match unsafe { crate::uwp::detect_uwp_process(handle) } {
                                Ok(Some(family_name)) => {
                                    let app_id = AppIdentifier::Uwp(family_name);

                                    self.app_id_cache.insert(pid, (app_id.clone(), now));

                                    if self.watch_state.read().identifiers.contains(&app_id) {
                                        debug!(
                                            "Found monitored UWP process (PID {}): {}",
                                            pid, app_id
                                        );
                                        current_processes.insert(app_id);
                                    }
                                }
                                Ok(None) => {
                                    if let Some(app_id) =
                                        extract_win32_app_identifier(&entry.szExeFile, pid)
                                    {
                                        self.app_id_cache.insert(pid, (app_id.clone(), now));

                                        if self.watch_state.read().identifiers.contains(&app_id) {
                                            current_processes.insert(app_id);
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(
                                        "Failed to detect UWP package for process ID {}: {:#}",
                                        pid, e
                                    );

                                    if let Some(app_id) =
                                        extract_win32_app_identifier(&entry.szExeFile, pid)
                                    {
                                        self.app_id_cache.insert(pid, (app_id.clone(), now));

                                        if self.watch_state.read().identifiers.contains(&app_id) {
                                            current_processes.insert(app_id);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            debug!("Failed to open process handle for PID {}: {}", pid, e);

                            if let Some(app_id) =
                                extract_win32_app_identifier(&entry.szExeFile, pid)
                            {
                                self.app_id_cache.insert(pid, (app_id.clone(), now));

                                if self.watch_state.read().identifiers.contains(&app_id) {
                                    current_processes.insert(app_id);
                                }
                            }
                        }
                    }
                }

                has_process = unsafe {
                    match Process32NextW(snapshot, &raw mut entry) {
                        Ok(()) => true,
                        Err(e) => {
                            if e.code() == ERROR_NO_MORE_FILES.to_hresult() {
                                false
                            } else {
                                warn!("Error iterating processes: {e}");
                                false
                            }
                        }
                    }
                };
            }

            debug!("Found {} running processes", current_processes.len());

            let total_lookups = cache_hits + cache_misses;
            if total_lookups > 0 {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "f64 has sufficient precision for process count statistics"
                )]
                let hit_rate = (cache_hits as f64 / total_lookups as f64) * 100.0;
                debug!(
                    "AppIdentifier cache: {} hits, {} misses ({:.1}% hit rate)",
                    cache_hits, cache_misses, hit_rate
                );
            }

            self.detect_changes(current_processes);

            self.poll_cycle_count.fetch_add(1, Ordering::Relaxed);

            Ok(())
        }

        #[cfg(not(windows))]
        {
            Err(EasyHdrError::ProcessMonitorError(
                crate::error::StringError::new("Process monitoring is only supported on Windows"),
            ))
        }
    }

    /// Detect changes between current and previous snapshots.
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "Function used only on Windows for process change detection"
        )
    )]
    fn detect_changes(&mut self, current: HashSet<AppIdentifier>) {
        use tracing::info;

        let apps = {
            let state = self.watch_state.read();
            Arc::clone(&state.apps)
        };

        for app_id in current.difference(&self.running_processes) {
            if Self::is_monitored(app_id, &apps) {
                info!("Detected process started: {:?}", app_id);
                if let Err(e) = self
                    .event_sender
                    .send(ProcessEvent::Started(app_id.clone()))
                {
                    use tracing::error;
                    error!(
                        "Failed to send ProcessEvent::Started for '{:?}': {}",
                        app_id, e
                    );
                }
            }
        }

        for app_id in self.running_processes.difference(&current) {
            if Self::is_monitored(app_id, &apps) {
                info!("Detected process stopped: {:?}", app_id);
                if let Err(e) = self
                    .event_sender
                    .send(ProcessEvent::Stopped(app_id.clone()))
                {
                    use tracing::error;
                    error!(
                        "Failed to send ProcessEvent::Stopped for '{:?}': {}",
                        app_id, e
                    );
                }
            }
        }

        self.estimated_process_count = (self.estimated_process_count * 3 + current.len()) / 4;

        self.running_processes = current;
    }

    /// Check if an app identifier is monitored.
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "Function used only on Windows for process monitoring"
        )
    )]
    fn is_monitored(app_id: &AppIdentifier, watch_list: &[MonitoredApp]) -> bool {
        match app_id {
            AppIdentifier::Win32(process_name) => watch_list.iter().any(|app| {
                if let MonitoredApp::Win32(win32_app) = app {
                    win32_app.enabled && win32_app.process_name.eq_ignore_ascii_case(process_name)
                } else {
                    false
                }
            }),
            AppIdentifier::Uwp(package_family_name) => watch_list.iter().any(|app| {
                if let MonitoredApp::Uwp(uwp_app) = app {
                    uwp_app.enabled && uwp_app.package_family_name == *package_family_name
                } else {
                    false
                }
            }),
        }
    }
}

/// RAII guard for Windows snapshot handle.
#[cfg(windows)]
struct SnapshotGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SnapshotGuard {
    /// # Safety
    ///
    /// Handle from `CreateToolhelp32Snapshot`. Guard owns handle (closed once, not cloned/shared).
    #[expect(
        unsafe_code,
        reason = "Windows FFI for CloseHandle to release snapshot handle"
    )]
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// RAII guard for Windows process handle.
#[cfg(windows)]
struct ProcessHandleGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for ProcessHandleGuard {
    /// # Safety
    ///
    /// Handle from `OpenProcess`. Guard owns handle (closed once, not cloned/shared).
    #[expect(
        unsafe_code,
        reason = "Windows FFI for CloseHandle to release process handle"
    )]
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Helper to extract Win32 app identifier from process entry.
///
/// Returns `None` if the process name cannot be extracted (invalid UTF-16, etc.).
#[cfg(windows)]
fn extract_win32_app_identifier(sz_exe_file: &[u16; 260], pid: u32) -> Option<AppIdentifier> {
    use tracing::debug;

    extract_process_name(sz_exe_file).map(|name| {
        let name_lower = extract_filename_without_extension(&name);
        debug!("Found Win32 process (PID {}): {}", pid, name_lower);
        AppIdentifier::Win32(name_lower)
    })
}

/// Extract process name from szExeFile field.
#[cfg(windows)]
fn extract_process_name(sz_exe_file: &[u16; 260]) -> Option<String> {
    let len = sz_exe_file
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(sz_exe_file.len());

    String::from_utf16(&sz_exe_file[..len]).ok()
}

/// Extract filename without extension and convert to lowercase.
#[cfg_attr(
    not(windows),
    expect(
        dead_code,
        reason = "Function used only on Windows for process name matching"
    )
)]
fn extract_filename_without_extension(path: &str) -> String {
    let filename = if let Some(pos) = path.rfind('\\') {
        &path[pos + 1..]
    } else if let Some(pos) = path.rfind('/') {
        &path[pos + 1..]
    } else {
        path
    };

    let name_without_ext = if let Some(pos) = filename.rfind('.') {
        &filename[..pos]
    } else {
        filename
    };

    name_without_ext.to_lowercase()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::{MonitoredApp, Win32App};
    use std::path::PathBuf;
    use uuid::Uuid;

    /// Helper function to create a test `Win32App`
    fn create_test_win32_app(process_name: &str, display_name: &str) -> MonitoredApp {
        MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: display_name.to_string(),
            exe_path: PathBuf::from(format!("C:\\test\\{process_name}.exe")),
            process_name: process_name.to_string(),
            enabled: true,
            icon_data: None,
        })
    }

    #[test]
    fn test_extract_filename_without_extension() {
        // Test full Windows path
        assert_eq!(
            extract_filename_without_extension("C:\\Windows\\System32\\notepad.exe"),
            "notepad"
        );

        // Test simple filename
        assert_eq!(extract_filename_without_extension("game.exe"), "game");

        // Test uppercase extension (should be lowercase)
        assert_eq!(extract_filename_without_extension("MyApp.EXE"), "myapp");

        // Test mixed case
        assert_eq!(
            extract_filename_without_extension("C:\\Games\\Cyberpunk2077.exe"),
            "cyberpunk2077"
        );

        // Test no extension
        assert_eq!(extract_filename_without_extension("process"), "process");

        // Test Unix-style path (edge case)
        assert_eq!(
            extract_filename_without_extension("/usr/bin/app.exe"),
            "app"
        );

        // Test multiple dots
        assert_eq!(extract_filename_without_extension("my.app.exe"), "my.app");
    }

    #[test]
    fn test_detect_changes_started() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Set up watch list
        monitor.update_watch_list(vec![
            create_test_win32_app("notepad", "Notepad"),
            create_test_win32_app("game", "Game"),
        ]);

        // Initial state: no processes running
        monitor.running_processes = HashSet::new();

        // Current state: notepad started
        let mut current = HashSet::new();
        current.insert(AppIdentifier::Win32("notepad".to_string()));
        current.insert(AppIdentifier::Win32("explorer".to_string())); // Not monitored

        monitor.detect_changes(current);

        // Should receive Started event for notepad
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Started(AppIdentifier::Win32(name)) => assert_eq!(name, "notepad"),
            ProcessEvent::Started(AppIdentifier::Uwp(_)) => panic!("Expected Win32 Started event"),
            ProcessEvent::Stopped(_) => panic!("Expected Started event, got Stopped"),
        }

        // Should not receive event for explorer (not monitored)
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn test_detect_changes_stopped() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Set up watch list
        monitor.update_watch_list(vec![
            create_test_win32_app("notepad", "Notepad"),
            create_test_win32_app("game", "Game"),
        ]);

        // Initial state: notepad running
        let mut initial = HashSet::new();
        initial.insert(AppIdentifier::Win32("notepad".to_string()));
        initial.insert(AppIdentifier::Win32("explorer".to_string()));
        monitor.running_processes = initial;

        // Current state: notepad stopped
        let mut current = HashSet::new();
        current.insert(AppIdentifier::Win32("explorer".to_string()));

        monitor.detect_changes(current);

        // Should receive Stopped event for notepad
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Stopped(AppIdentifier::Win32(name)) => assert_eq!(name, "notepad"),
            ProcessEvent::Stopped(AppIdentifier::Uwp(_)) => panic!("Expected Win32 Stopped event"),
            ProcessEvent::Started(_) => panic!("Expected Stopped event, got Started"),
        }
    }

    #[test]
    fn test_detect_changes_case_insensitive() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Watch list has lowercase
        monitor.update_watch_list(vec![create_test_win32_app("notepad", "Notepad")]);

        // Initial state: empty
        monitor.running_processes = HashSet::new();

        // Current state: process name is already lowercase (as it should be from extraction)
        let mut current = HashSet::new();
        current.insert(AppIdentifier::Win32("notepad".to_string()));

        monitor.detect_changes(current);

        // Should receive Started event
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Started(AppIdentifier::Win32(name)) => assert_eq!(name, "notepad"),
            ProcessEvent::Started(AppIdentifier::Uwp(_)) => panic!("Expected Win32 Started event"),
            ProcessEvent::Stopped(_) => panic!("Expected Started event, got Stopped"),
        }
    }

    #[test]
    fn test_detect_changes_multiple_processes() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Watch multiple processes
        monitor.update_watch_list(vec![
            create_test_win32_app("notepad", "Notepad"),
            create_test_win32_app("game", "Game"),
            create_test_win32_app("app", "App"),
        ]);

        // Initial state: empty
        monitor.running_processes = HashSet::new();

        // Current state: multiple processes started
        let mut current = HashSet::new();
        current.insert(AppIdentifier::Win32("notepad".to_string()));
        current.insert(AppIdentifier::Win32("game".to_string()));
        current.insert(AppIdentifier::Win32("explorer".to_string())); // Not monitored

        monitor.detect_changes(current);

        // Should receive Started events for notepad and game
        let mut received = HashSet::new();
        for _ in 0..2 {
            let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
            match event {
                ProcessEvent::Started(AppIdentifier::Win32(name)) => {
                    received.insert(name);
                }
                ProcessEvent::Started(AppIdentifier::Uwp(_)) => {
                    panic!("Expected Win32 Started event")
                }
                ProcessEvent::Stopped(_) => panic!("Expected Started event, got Stopped"),
            }
        }

        assert!(received.contains("notepad"));
        assert!(received.contains("game"));
        assert_eq!(received.len(), 2);
    }

    #[test]
    fn test_process_name_extraction_comprehensive() {
        // Test various Windows path formats
        assert_eq!(
            extract_filename_without_extension("C:\\Games\\game.exe"),
            "game"
        );

        assert_eq!(
            extract_filename_without_extension("D:\\Program Files\\MyApp\\app.exe"),
            "app"
        );

        // Test with spaces in path
        assert_eq!(
            extract_filename_without_extension("C:\\Program Files (x86)\\Game Name\\game.exe"),
            "game"
        );

        // Test network path
        assert_eq!(
            extract_filename_without_extension("\\\\server\\share\\app.exe"),
            "app"
        );

        // Test relative path
        assert_eq!(
            extract_filename_without_extension("..\\..\\game.exe"),
            "game"
        );

        // Test filename only
        assert_eq!(
            extract_filename_without_extension("application.exe"),
            "application"
        );
    }

    #[test]
    fn test_case_insensitive_matching_comprehensive() {
        // Test that extraction always produces lowercase
        assert_eq!(extract_filename_without_extension("Game.exe"), "game");

        assert_eq!(extract_filename_without_extension("GAME.EXE"), "game");

        assert_eq!(extract_filename_without_extension("GaMe.ExE"), "game");

        assert_eq!(
            extract_filename_without_extension("C:\\Games\\MyGame.EXE"),
            "mygame"
        );

        // Test with mixed case in filename
        assert_eq!(
            extract_filename_without_extension("CyberPunk2077.exe"),
            "cyberpunk2077"
        );
    }

    #[test]
    fn test_multiple_state_transitions() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec![
            create_test_win32_app("app1", "App 1"),
            create_test_win32_app("app2", "App 2"),
            create_test_win32_app("app3", "App 3"),
        ]);

        // Initial state: app1 and app2 running
        let mut initial = HashSet::new();
        initial.insert(AppIdentifier::Win32("app1".to_string()));
        initial.insert(AppIdentifier::Win32("app2".to_string()));
        monitor.running_processes = initial;

        // New state: app2 and app3 running (app1 stopped, app3 started)
        let mut current = HashSet::new();
        current.insert(AppIdentifier::Win32("app2".to_string()));
        current.insert(AppIdentifier::Win32("app3".to_string()));

        monitor.detect_changes(current);

        // Should receive both Started and Stopped events
        let mut started = Vec::new();
        let mut stopped = Vec::new();

        for _ in 0..2 {
            let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
            match event {
                ProcessEvent::Started(AppIdentifier::Win32(name)) => started.push(name),
                ProcessEvent::Stopped(AppIdentifier::Win32(name)) => stopped.push(name),
                _ => panic!("Expected Win32 event"),
            }
        }

        assert_eq!(started.len(), 1);
        assert_eq!(stopped.len(), 1);
        assert!(started.contains(&"app3".to_string()));
        assert!(stopped.contains(&"app1".to_string()));
    }

    #[test]
    fn test_no_events_when_no_state_change() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec![create_test_win32_app("game", "Game")]);

        // Initial state: game running
        let mut initial = HashSet::new();
        initial.insert(AppIdentifier::Win32("game".to_string()));
        monitor.running_processes = initial.clone();

        // Current state: same as before (no change)
        monitor.detect_changes(initial);

        // Should not receive any events
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn test_only_monitored_processes_trigger_events() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Only watch "game"
        monitor.update_watch_list(vec![create_test_win32_app("game", "Game")]);

        monitor.running_processes = HashSet::new();

        // Start multiple processes, only one monitored
        let mut current = HashSet::new();
        current.insert(AppIdentifier::Win32("game".to_string())); // Monitored
        current.insert(AppIdentifier::Win32("notepad".to_string())); // Not monitored
        current.insert(AppIdentifier::Win32("explorer".to_string())); // Not monitored
        current.insert(AppIdentifier::Win32("chrome".to_string())); // Not monitored

        monitor.detect_changes(current);

        // Should only receive one event for "game"
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Started(AppIdentifier::Win32(name)) => assert_eq!(name, "game"),
            ProcessEvent::Started(AppIdentifier::Uwp(_)) => panic!("Expected Win32 Started event"),
            ProcessEvent::Stopped(_) => panic!("Expected Started event for game, got Stopped"),
        }

        // No more events should be received
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn test_empty_watch_list() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Empty watch list
        monitor.update_watch_list(vec![]);

        monitor.running_processes = HashSet::new();

        // Start some processes
        let mut current = HashSet::new();
        current.insert(AppIdentifier::Win32("game".to_string()));
        current.insert(AppIdentifier::Win32("notepad".to_string()));

        monitor.detect_changes(current);

        // Should not receive any events
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn test_process_name_with_special_characters() {
        assert_eq!(extract_filename_without_extension("my-app.exe"), "my-app");

        assert_eq!(extract_filename_without_extension("app_v2.exe"), "app_v2");

        assert_eq!(extract_filename_without_extension("app (1).exe"), "app (1)");

        assert_eq!(
            extract_filename_without_extension("app[test].exe"),
            "app[test]"
        );
    }

    #[test]
    fn test_very_long_path() {
        let long_path = "C:\\Very\\Long\\Path\\With\\Many\\Directories\\And\\Subdirectories\\That\\Goes\\On\\And\\On\\application.exe";
        assert_eq!(extract_filename_without_extension(long_path), "application");
    }

    // Property-based tests using proptest
    #[cfg(test)]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Property: Process name normalization always produces lowercase output
            #[test]
            fn process_name_is_always_lowercase(s in "[a-zA-Z0-9_-]+\\.exe") {
                let normalized = extract_filename_without_extension(&s);
                let lowercase = normalized.to_lowercase();
                prop_assert_eq!(normalized, lowercase);
            }

            /// Property: Normalization removes .exe extension
            #[test]
            #[expect(
                clippy::case_sensitive_file_extension_comparisons,
                reason = "Test specifically validates .exe extension handling on Windows where extensions are case-insensitive"
            )]
            fn normalization_removes_exe_extension(name in "[a-zA-Z0-9_-]+") {
                let input = format!("{name}.exe");
                let normalized = extract_filename_without_extension(&input);
                prop_assert!(!normalized.ends_with(".exe"));
                prop_assert_eq!(normalized, name.to_lowercase());
            }

            /// Property: Path extraction gets only the filename
            #[test]
            fn path_extraction_gets_filename_only(
                dirs in prop::collection::vec("[a-zA-Z0-9_-]+", 1..5),
                filename in "[a-zA-Z0-9_-]+"
            ) {
                let path = format!("C:\\{}\\{}.exe", dirs.join("\\"), filename);
                let normalized = extract_filename_without_extension(&path);
                prop_assert_eq!(normalized, filename.to_lowercase());
            }

            /// Property: Empty or whitespace-only input produces empty output
            #[test]
            fn empty_input_produces_empty_output(s in "\\s*") {
                let normalized = extract_filename_without_extension(&s);
                prop_assert!(normalized.is_empty() || normalized.chars().all(char::is_whitespace));
            }

            /// Property: Watch list update is idempotent
            #[test]
            fn watch_list_update_is_idempotent(
                names in prop::collection::vec("[a-zA-Z0-9_-]+(\\.exe)?", 1..10)
            ) {
                let (tx, _rx) = mpsc::sync_channel(32);
                let monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

                // Convert names to MonitoredApp objects
                let apps: Vec<MonitoredApp> = names.iter().map(|name| {
                    create_test_win32_app(name, name)
                }).collect();

                // Update once
                monitor.update_watch_list(apps.clone());
                let first_result_len = monitor.watch_state.read().apps.len();

                // Update again with the same input
                monitor.update_watch_list(apps);
                let second_result_len = monitor.watch_state.read().apps.len();

                // Results should be identical (same number of apps)
                prop_assert_eq!(first_result_len, second_result_len);
            }

            /// Property: PID cache handles reuse correctly with expiry
            ///
            /// Tests that the cache expires entries after the expiry duration,
            /// preventing stale data from being returned when PIDs are reused.
            /// This is critical for correctness on Windows where PIDs can be reused.
            #[test]
            fn pid_cache_expires_old_entries(
                pid in 100u32..10000u32,
                app_name in "[a-zA-Z0-9_-]+"
            ) {
                const CACHE_EXPIRY: Duration = Duration::from_secs(5);

                let (tx, _rx) = mpsc::sync_channel(32);
                let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

                // Create an AppIdentifier for testing
                let app_id = AppIdentifier::Win32(app_name.clone());

                // Simulate cache entry creation (as poll_processes would do)
                let initial_time = Instant::now();
                monitor.app_id_cache.insert(pid, (app_id.clone(), initial_time));

                // Verify cache contains the entry
                prop_assert!(monitor.app_id_cache.contains_key(&pid));
                prop_assert_eq!(&monitor.app_id_cache.get(&pid).unwrap().0, &app_id);

                // Simulate time passing beyond expiry (5 seconds)
                // We can't actually wait 5 seconds in a test, so we manually insert
                // an old timestamp to simulate an expired entry
                let old_time = initial_time
                    .checked_sub(Duration::from_secs(6))
                    .unwrap();
                monitor.app_id_cache.insert(pid, (app_id.clone(), old_time));

                // Simulate poll_processes expiry logic
                let now = Instant::now();
                monitor.app_id_cache.retain(|_pid, (_app_id, last_seen)| {
                    now.duration_since(*last_seen) < CACHE_EXPIRY
                });

                // After expiry, cache should not contain the PID
                prop_assert!(!monitor.app_id_cache.contains_key(&pid));

                // Verify cache size is 0 (entry was removed)
                prop_assert_eq!(monitor.app_id_cache.len(), 0);
            }
        }
    }
}
