//! Process monitoring implementation
//!
//! Polls Windows processes and detects state changes. Matches by executable filename only
//! (lowercase, no extension), not full path. Name collisions trigger HDR for all matching processes.
//!
//! **Limitation:** Use unique executable names to avoid false positives.

use parking_lot::{Mutex, RwLock};
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

/// Process monitor that polls for running processes
///
/// Monitors running processes at regular intervals and detects state changes
/// for configured applications. Uses Windows Toolhelp32 API for process enumeration.
///
/// **Known Limitation:** Matches processes by executable filename only (without path or extension).
/// Multiple processes with the same filename will all be detected as the same application.
pub struct ProcessMonitor {
    /// List of monitored applications to watch (both Win32 and UWP) - Phase 2.1: Double-Arc
    ///
    /// Uses Arc<Mutex<Arc<Vec<_>>>> to eliminate per-poll cloning overhead.
    /// When reading, we clone the inner Arc (cheap pointer copy) instead of the entire Vec.
    /// Lock hold time: O(n) → O(1) since we only need the lock to get the Arc reference.
    watch_list: Arc<Mutex<Arc<Vec<MonitoredApp>>>>,
    /// Cached set of monitored app identifiers for fast filtering (Phase 1.1)
    ///
    /// Shared with `AppController` for synchronized updates. Allows O(1) lookups
    /// to skip processing unmonitored processes (~90% of enumerated processes).
    /// `RwLock` enables concurrent reads in hot path (`poll_processes()`) without blocking.
    monitored_identifiers: Arc<RwLock<HashSet<AppIdentifier>>>,
    /// PID-based `AppIdentifier` cache to avoid repeated string allocations (Phase 1.2)
    ///
    /// Maps PID → (`AppIdentifier`, `last_seen_timestamp`). Entries expire after 5s
    /// to handle PID reuse. Reduces string allocations from ~250/poll to <10/poll.
    /// Pre-allocated with capacity for 200 processes (typical system load).
    #[cfg_attr(not(windows), allow(dead_code))]
    app_id_cache: HashMap<u32, (AppIdentifier, Instant)>,
    /// Channel to send process events
    #[cfg_attr(not(windows), allow(dead_code))]
    event_sender: mpsc::SyncSender<ProcessEvent>,
    /// Polling interval
    interval: Duration,
    /// Previous snapshot of running processes (identified by `AppIdentifier`)
    #[cfg_attr(not(windows), allow(dead_code))]
    running_processes: HashSet<AppIdentifier>,
    /// Estimated process count for capacity pre-allocation
    #[cfg_attr(not(windows), allow(dead_code))]
    estimated_process_count: usize,
    /// Number of completed poll cycles (for testing/diagnostics)
    ///
    /// Used by both unit tests and integration tests (e.g., DHAT profiling).
    /// Always compiled but only accessed through `#[doc(hidden)]` test methods.
    poll_cycle_count: Arc<AtomicU64>,
}

impl ProcessMonitor {
    /// Create a new process monitor with the specified polling interval
    pub fn new(interval: Duration, event_sender: mpsc::SyncSender<ProcessEvent>) -> Self {
        // Typical Windows system has 150-250 processes
        const DEFAULT_PROCESS_COUNT: usize = 200;

        Self {
            watch_list: Arc::new(Mutex::new(Arc::new(Vec::new()))),
            monitored_identifiers: Arc::new(RwLock::new(HashSet::new())),
            app_id_cache: HashMap::with_capacity(DEFAULT_PROCESS_COUNT),
            event_sender,
            interval,
            running_processes: HashSet::with_capacity(DEFAULT_PROCESS_COUNT),
            estimated_process_count: DEFAULT_PROCESS_COUNT,
            poll_cycle_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Update the list of monitored applications to watch
    ///
    /// Replaces the entire watch list with the provided applications.
    /// Only enabled applications should be passed to this method.
    /// Also rebuilds the `monitored_identifiers` cache for fast O(1) filtering.
    pub fn update_watch_list(&self, monitored_apps: Vec<MonitoredApp>) {
        // Rebuild monitored identifiers set from the new watch list
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

        // Update both caches atomically (from caller's perspective)
        // Phase 2.1: Wrap apps in Arc to enable cheap cloning during event handling
        let mut watch_list = self.watch_list.lock();
        *watch_list = Arc::new(monitored_apps);
        drop(watch_list); // Release watch_list lock before acquiring write lock

        let mut monitored_ids = self.monitored_identifiers.write();
        *monitored_ids = identifiers;
    }

    /// Get a reference to the watch list for external updates
    pub fn get_watch_list_ref(&self) -> Arc<Mutex<Arc<Vec<MonitoredApp>>>> {
        Arc::clone(&self.watch_list)
    }

    /// Get a reference to the monitored identifiers cache
    ///
    /// Returns a shared reference to the cached set of monitored app identifiers.
    /// This enables `AppController` to share the same cache for O(1) lookups
    /// when handling process events.
    ///
    /// # Phase 1.1: Cache Synchronization
    ///
    /// Both `ProcessMonitor` and `AppController` hold references to the same
    /// `Arc<RwLock<HashSet<AppIdentifier>>>`. When the GUI modifies the app list,
    /// `AppController` calls `update_watch_list()` which rebuilds the cache,
    /// ensuring both components see consistent state.
    pub fn get_monitored_identifiers_ref(&self) -> Arc<RwLock<HashSet<AppIdentifier>>> {
        Arc::clone(&self.monitored_identifiers)
    }

    /// Get the number of completed poll cycles
    ///
    /// Used for testing and diagnostics to verify the monitor is actively polling.
    /// Returns the count using Relaxed ordering since precise synchronization
    /// is not required for diagnostic purposes.
    ///
    /// # Memory Ordering
    ///
    /// Uses `Ordering::Relaxed` because:
    /// - This is purely a diagnostic counter for test verification
    /// - No happens-before relationship needed with other operations
    /// - Approximate count is acceptable (exact synchronization not required)
    /// - Counter only increases monotonically (no complex state dependencies)
    ///
    /// # Test-Only API
    ///
    /// Hidden from documentation as this is internal test infrastructure.
    /// Integration tests are compiled as separate crates, so `cfg(test)` doesn't apply.
    /// This method is always compiled but hidden from docs to signal test-only usage.
    #[doc(hidden)]
    pub fn get_poll_cycle_count(&self) -> u64 {
        self.poll_cycle_count.load(Ordering::Relaxed)
    }

    /// Get a reference to the poll cycle counter for external monitoring
    ///
    /// Allows tests to monitor poll progress by holding an `Arc` reference
    /// to the counter and checking it periodically without borrowing the
    /// entire `ProcessMonitor` instance.
    ///
    /// # Memory Ordering
    ///
    /// Loads use `Ordering::Relaxed` for the same reasons as `get_poll_cycle_count()`.
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

    /// Poll processes and detect changes
    ///
    /// Enumerates all running processes using Windows Toolhelp32 API, extracts process names
    /// (lowercase, without extension), and detects state transitions.
    ///
    /// # Safety
    ///
    /// `CreateToolhelp32Snapshot` called with valid flags (`TH32CS_SNAPPROCESS`, PID 0).
    /// Return value validated via `map_err`; errors propagated. Handle wrapped in
    /// `SnapshotGuard` (RAII) for cleanup. `PROCESSENTRY32W` initialized with correct
    /// `dwSize` to prevent buffer overruns. `Process32FirstW`/`NextW` return codes checked
    /// before data access; `ERROR_NO_MORE_FILES` handled as iteration end. `&raw mut entry`
    /// valid (stack variable, correct size).
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
    #[allow(clippy::too_many_lines)]
    fn poll_processes(&mut self) -> Result<()> {
        #[cfg(windows)]
        {
            use tracing::{debug, warn};

            // Phase 1.2: Expire stale cache entries (older than 5 seconds) to handle PID reuse
            let now = Instant::now();
            const CACHE_EXPIRY: Duration = Duration::from_secs(5);
            self.app_id_cache
                .retain(|_pid, (_app_id, last_seen)| now.duration_since(*last_seen) < CACHE_EXPIRY);

            // Phase 1.2: Cache hit rate instrumentation
            let mut cache_hits = 0usize;
            let mut cache_misses = 0usize;

            // Take a snapshot of all running processes
            let snapshot = unsafe {
                CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| {
                    use tracing::error;
                    error!("Windows API error - CreateToolhelp32Snapshot failed: {e}");
                    // Preserve error chain by wrapping the source error
                    EasyHdrError::ProcessMonitorError(Box::new(e))
                })?
            };

            // Ensure snapshot handle is closed when we're done
            let _guard = SnapshotGuard(snapshot);

            // Build a set of currently running process identifiers
            // Pre-allocate capacity based on previous snapshot size to avoid rehashing
            let capacity = self
                .running_processes
                .len()
                .max(self.estimated_process_count);
            let mut current_processes = HashSet::with_capacity(capacity);

            // Initialize PROCESSENTRY32W structure
            #[expect(
                clippy::cast_possible_truncation,
                reason = "size_of::<PROCESSENTRY32W>() is a compile-time constant (592 bytes) that fits in u32"
            )]
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };

            // Get the first process
            let mut has_process = unsafe { Process32FirstW(snapshot, &raw mut entry).is_ok() };

            // Iterate through all processes
            while has_process {
                let pid = entry.th32ProcessID;

                // Try to open process handle for UWP detection
                // Use PROCESS_QUERY_LIMITED_INFORMATION for minimal access rights
                let handle_result =
                    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };

                // Phase 1.2: Check cache first before creating AppIdentifier
                if let Some((cached_app_id, _)) = self.app_id_cache.get(&pid) {
                    cache_hits += 1;
                    // Cache hit - reuse existing AppIdentifier
                    if self.monitored_identifiers.read().contains(cached_app_id) {
                        current_processes.insert(cached_app_id.clone());
                        // Update timestamp for this entry
                        self.app_id_cache.insert(pid, (cached_app_id.clone(), now));
                    }
                } else {
                    // Cache miss - need to create new AppIdentifier
                    cache_misses += 1;

                    match handle_result {
                        Ok(handle) => {
                            // Ensure handle is closed when we're done
                            let _guard = ProcessHandleGuard(handle);

                            // Try UWP detection first
                            match unsafe { crate::uwp::detect_uwp_process(handle) } {
                                Ok(Some(family_name)) => {
                                    // UWP app detected - check if monitored before inserting
                                    let app_id = AppIdentifier::Uwp(family_name);

                                    // Cache the AppIdentifier for this PID
                                    self.app_id_cache.insert(pid, (app_id.clone(), now));

                                    if self.monitored_identifiers.read().contains(&app_id) {
                                        debug!(
                                            "Found monitored UWP process (PID {}): {}",
                                            pid, app_id
                                        );
                                        current_processes.insert(app_id);
                                    }
                                    // Early exit: skip unmonitored UWP apps (Phase 1.1 optimization)
                                }
                                Ok(None) => {
                                    // Win32 app - extract process name and check if monitored
                                    if let Some(app_id) =
                                        extract_win32_app_identifier(&entry.szExeFile, pid)
                                    {
                                        // Cache the AppIdentifier for this PID
                                        self.app_id_cache.insert(pid, (app_id.clone(), now));

                                        if self.monitored_identifiers.read().contains(&app_id) {
                                            current_processes.insert(app_id);
                                        }
                                        // Early exit: skip unmonitored Win32 apps (Phase 1.1 optimization)
                                    }
                                }
                                Err(e) => {
                                    // UWP detection failed - log error with context but continue
                                    // This is non-fatal; we'll treat it as Win32 fallback
                                    warn!(
                                        "Failed to detect UWP package for process ID {}: {:#}",
                                        pid, e
                                    );

                                    // Fallback to Win32 detection
                                    if let Some(app_id) =
                                        extract_win32_app_identifier(&entry.szExeFile, pid)
                                    {
                                        // Cache the AppIdentifier for this PID
                                        self.app_id_cache.insert(pid, (app_id.clone(), now));

                                        if self.monitored_identifiers.read().contains(&app_id) {
                                            current_processes.insert(app_id);
                                        }
                                        // Early exit: skip unmonitored Win32 apps (Phase 1.1 optimization)
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            // Failed to open process handle - this is common for system processes
                            // or processes with higher privileges. Log at debug level and continue.
                            debug!("Failed to open process handle for PID {}: {}", pid, e);

                            // Fallback to Win32 detection using process name
                            if let Some(app_id) =
                                extract_win32_app_identifier(&entry.szExeFile, pid)
                            {
                                // Cache the AppIdentifier for this PID
                                self.app_id_cache.insert(pid, (app_id.clone(), now));

                                if self.monitored_identifiers.read().contains(&app_id) {
                                    current_processes.insert(app_id);
                                }
                                // Early exit: skip unmonitored Win32 apps (Phase 1.1 optimization)
                            }
                        }
                    }
                }

                // Get the next process
                has_process = unsafe {
                    match Process32NextW(snapshot, &raw mut entry) {
                        Ok(()) => true,
                        Err(e) => {
                            // ERROR_NO_MORE_FILES is expected at the end
                            if e.code() == ERROR_NO_MORE_FILES.to_hresult() {
                                false
                            } else {
                                // Log other errors but continue
                                warn!("Error iterating processes: {e}");
                                false
                            }
                        }
                    }
                };
            }

            debug!("Found {} running processes", current_processes.len());

            // Phase 1.2: Log cache hit rate for performance monitoring
            let total_lookups = cache_hits + cache_misses;
            if total_lookups > 0 {
                let hit_rate = (cache_hits as f64 / total_lookups as f64) * 100.0;
                debug!(
                    "AppIdentifier cache: {} hits, {} misses ({:.1}% hit rate)",
                    cache_hits, cache_misses, hit_rate
                );
            }

            // Detect changes and send events
            self.detect_changes(current_processes);

            // Increment poll cycle counter for diagnostic purposes
            // Relaxed ordering is sufficient - this is just a diagnostic counter
            self.poll_cycle_count.fetch_add(1, Ordering::Relaxed);

            Ok(())
        }

        #[cfg(not(windows))]
        {
            // Non-Windows platforms not supported
            Err(EasyHdrError::ProcessMonitorError(
                crate::error::StringError::new("Process monitoring is only supported on Windows"),
            ))
        }
    }

    /// Detect changes between current and previous snapshots
    ///
    /// Compares the current process snapshot with the previous one to identify
    /// which monitored processes have started or stopped, then sends appropriate events.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn detect_changes(&mut self, current: HashSet<AppIdentifier>) {
        use tracing::info;

        // Phase 2.1: Clone inner Arc (cheap pointer copy) instead of entire Vec
        // Lock hold time: O(1) - just copying an Arc pointer, not the Vec contents
        let watch_list = {
            let guard = self.watch_list.lock();
            Arc::clone(&*guard)
        }; // Lock is released here

        // Find started processes
        for app_id in current.difference(&self.running_processes) {
            // Check if this app identifier is monitored
            if Self::is_monitored(app_id, &watch_list) {
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

        // Find stopped processes
        for app_id in self.running_processes.difference(&current) {
            // Check if this app identifier is monitored
            if Self::is_monitored(app_id, &watch_list) {
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

        // Update estimated process count for next iteration's capacity hint
        // Use exponential moving average to smooth out variations
        self.estimated_process_count = (self.estimated_process_count * 3 + current.len()) / 4;

        self.running_processes = current;
    }

    /// Check if an app identifier is monitored
    ///
    /// Pattern matches on the `AppIdentifier` and checks against the `MonitoredApp` enum.
    /// For Win32 apps, matches against `Win32App` `process_name` (case-insensitive).
    /// For UWP apps, matches against `UwpApp` `package_family_name` (exact match).
    #[cfg_attr(not(windows), allow(dead_code))]
    fn is_monitored(app_id: &AppIdentifier, watch_list: &[MonitoredApp]) -> bool {
        match app_id {
            AppIdentifier::Win32(process_name) => {
                // Match against Win32App process_name (case-insensitive)
                watch_list.iter().any(|app| {
                    if let MonitoredApp::Win32(win32_app) = app {
                        win32_app.enabled
                            && win32_app.process_name.eq_ignore_ascii_case(process_name)
                    } else {
                        false
                    }
                })
            }
            AppIdentifier::Uwp(package_family_name) => {
                // Match against UwpApp package_family_name (exact match)
                watch_list.iter().any(|app| {
                    if let MonitoredApp::Uwp(uwp_app) = app {
                        uwp_app.enabled && uwp_app.package_family_name == *package_family_name
                    } else {
                        false
                    }
                })
            }
        }
    }
}

/// RAII guard for Windows snapshot handle
///
/// Ensures the snapshot handle is properly closed when the guard goes out of scope.
#[cfg(windows)]
struct SnapshotGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SnapshotGuard {
    /// Closes the snapshot handle
    ///
    /// # Safety
    ///
    /// Handle from `CreateToolhelp32Snapshot` (valid or error; only valid stored). Guard
    /// owns handle (closed once, not cloned/shared). `CloseHandle` safe on valid snapshot
    /// handles; result ignored (no destructor recovery).
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

/// RAII guard for Windows process handle
///
/// Ensures the process handle is properly closed when the guard goes out of scope.
#[cfg(windows)]
struct ProcessHandleGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for ProcessHandleGuard {
    /// Closes the process handle
    ///
    /// # Safety
    ///
    /// Handle from `OpenProcess` (valid or error; only valid stored). Guard owns handle
    /// (closed once, not cloned/shared). `CloseHandle` safe on valid process handles;
    /// result ignored (no destructor recovery).
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

/// Helper to extract Win32 app identifier from process entry
///
/// Extracts the process name from szExeFile, converts it to lowercase without extension,
/// and returns it as an `AppIdentifier`. Logs a debug message with PID for traceability.
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

/// Extract process name from szExeFile field
///
/// Converts a null-terminated wide string to a Rust String.
#[cfg(windows)]
fn extract_process_name(sz_exe_file: &[u16; 260]) -> Option<String> {
    // Find the null terminator
    let len = sz_exe_file
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(sz_exe_file.len());

    // Convert to String
    String::from_utf16(&sz_exe_file[..len]).ok()
}

/// Extract filename without extension and convert to lowercase
///
/// Normalizes process names for case-insensitive matching.
///
/// Examples:
/// - "C:\\Windows\\System32\\notepad.exe" -> "notepad"
/// - "game.exe" -> "game"
/// - "MyApp.EXE" -> "myapp"
#[cfg_attr(not(windows), allow(dead_code))]
fn extract_filename_without_extension(path: &str) -> String {
    // Extract filename from path
    let filename = if let Some(pos) = path.rfind('\\') {
        &path[pos + 1..]
    } else if let Some(pos) = path.rfind('/') {
        &path[pos + 1..]
    } else {
        path
    };

    // Remove extension
    let name_without_ext = if let Some(pos) = filename.rfind('.') {
        &filename[..pos]
    } else {
        filename
    };

    // Convert to lowercase for case-insensitive matching
    name_without_ext.to_lowercase()
}

#[cfg(test)]
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
                let first_result = monitor.watch_list.lock().clone();

                // Update again with the same input
                monitor.update_watch_list(apps);
                let second_result = monitor.watch_list.lock().clone();

                // Results should be identical (same number of apps)
                prop_assert_eq!(first_result.len(), second_result.len());
            }

            /// Property: PID cache handles reuse correctly with expiry (Phase 1.2)
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
