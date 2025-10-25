//! Process monitoring implementation
//!
//! Polls Windows processes and detects state changes. Matches by executable filename only
//! (lowercase, no extension), not full path. Name collisions trigger HDR for all matching processes.
//!
//! **Limitation:** Use unique executable names to avoid false positives.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES};

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
    /// List of process names to watch (lowercase)
    watch_list: Arc<Mutex<HashSet<String>>>,
    /// Channel to send process events
    #[cfg_attr(not(windows), allow(dead_code))]
    event_sender: mpsc::SyncSender<ProcessEvent>,
    /// Polling interval
    interval: Duration,
    /// Previous snapshot of running processes (identified by AppIdentifier)
    #[cfg_attr(not(windows), allow(dead_code))]
    running_processes: HashSet<AppIdentifier>,
    /// Estimated process count for capacity pre-allocation
    #[cfg_attr(not(windows), allow(dead_code))]
    estimated_process_count: usize,
}

impl ProcessMonitor {
    /// Create a new process monitor with the specified polling interval
    pub fn new(interval: Duration, event_sender: mpsc::SyncSender<ProcessEvent>) -> Self {
        // Typical Windows system has 150-250 processes
        const DEFAULT_PROCESS_COUNT: usize = 200;

        Self {
            watch_list: Arc::new(Mutex::new(HashSet::new())),
            event_sender,
            interval,
            running_processes: HashSet::with_capacity(DEFAULT_PROCESS_COUNT),
            estimated_process_count: DEFAULT_PROCESS_COUNT,
        }
    }

    /// Update the list of processes to watch
    pub fn update_watch_list(&self, process_names: Vec<String>) {
        let mut watch_list = self.watch_list.lock();
        watch_list.clear();
        for name in process_names {
            watch_list.insert(name.to_lowercase());
        }
    }

    /// Get a reference to the watch list for external updates
    pub fn get_watch_list_ref(&self) -> Arc<Mutex<HashSet<String>>> {
        Arc::clone(&self.watch_list)
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
    fn poll_processes(&mut self) -> Result<()> {
        #[cfg(windows)]
        {
            use tracing::{debug, warn};

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
                // Extract the process name from szExeFile
                // szExeFile is a null-terminated wide string
                let process_name = extract_process_name(&entry.szExeFile);

                if let Some(name) = process_name {
                    // Extract filename without extension and convert to lowercase
                    let name_lower = extract_filename_without_extension(&name);
                    debug!("Found process: {}", name_lower);
                    // For now, all processes are Win32 (UWP detection will be added in future task)
                    current_processes.insert(AppIdentifier::Win32(name_lower));
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

            // Detect changes and send events
            self.detect_changes(current_processes);

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

        // Clone watch list to minimize lock hold time
        let watch_list = {
            let guard = self.watch_list.lock();
            guard.clone()
        }; // Lock is released here

        // Find started processes
        for app_id in current.difference(&self.running_processes) {
            // Check if this app identifier is monitored
            let is_monitored = match app_id {
                AppIdentifier::Win32(process_name) => watch_list.contains(process_name),
                AppIdentifier::Uwp(_package_family_name) => {
                    // UWP apps are not yet supported in watch list (will be added in future task)
                    false
                }
            };

            if is_monitored {
                info!("Detected process started: {:?}", app_id);
                if let Err(e) = self.event_sender.send(ProcessEvent::Started(app_id.clone())) {
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
            let is_monitored = match app_id {
                AppIdentifier::Win32(process_name) => watch_list.contains(process_name),
                AppIdentifier::Uwp(_package_family_name) => {
                    // UWP apps are not yet supported in watch list (will be added in future task)
                    false
                }
            };

            if is_monitored {
                info!("Detected process stopped: {:?}", app_id);
                if let Err(e) = self.event_sender.send(ProcessEvent::Stopped(app_id.clone())) {
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

    #[test]
    fn test_process_monitor_creation() {
        let (tx, _rx) = mpsc::sync_channel(32);
        let monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);
        assert_eq!(monitor.interval, Duration::from_millis(1000));
    }

    #[test]
    fn test_update_watch_list() {
        let (tx, _rx) = mpsc::sync_channel(32);
        let monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec!["test.exe".to_string(), "game.exe".to_string()]);

        let watch_list = monitor.watch_list.lock();
        assert!(watch_list.contains("test.exe"));
        assert!(watch_list.contains("game.exe"));
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
        monitor.update_watch_list(vec!["notepad".to_string(), "game".to_string()]);

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
            ProcessEvent::Stopped(_) => panic!("Expected Started event, got Stopped"),
            _ => panic!("Expected Win32 Started event"),
        }

        // Should not receive event for explorer (not monitored)
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn test_detect_changes_stopped() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Set up watch list
        monitor.update_watch_list(vec!["notepad".to_string(), "game".to_string()]);

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
            ProcessEvent::Started(_) => panic!("Expected Stopped event, got Started"),
            _ => panic!("Expected Win32 Stopped event"),
        }
    }

    #[test]
    fn test_detect_changes_case_insensitive() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Watch list has lowercase
        monitor.update_watch_list(vec!["notepad".to_string()]);

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
            ProcessEvent::Stopped(_) => panic!("Expected Started event, got Stopped"),
            _ => panic!("Expected Win32 Started event"),
        }
    }

    #[test]
    fn test_detect_changes_multiple_processes() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Watch multiple processes
        monitor.update_watch_list(vec![
            "notepad".to_string(),
            "game".to_string(),
            "app".to_string(),
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
                ProcessEvent::Stopped(_) => panic!("Expected Started event, got Stopped"),
                _ => panic!("Expected Win32 Started event"),
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
    fn test_watch_list_case_insensitive() {
        let (tx, _rx) = mpsc::sync_channel(32);
        let monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Add watch list with mixed case - should be converted to lowercase
        monitor.update_watch_list(vec!["Game".to_string(), "APP".to_string()]);

        let watch_list = monitor.watch_list.lock();
        // Watch list should contain lowercase versions
        assert!(watch_list.contains("game"));
        assert!(watch_list.contains("app"));
        assert!(!watch_list.contains("Game"));
        assert!(!watch_list.contains("APP"));
    }

    #[test]
    fn test_state_transition_not_running_to_running() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec!["game".to_string()]);

        // Initial state: process not running
        monitor.running_processes = HashSet::new();

        // Transition: process starts
        let mut current = HashSet::new();
        current.insert(AppIdentifier::Win32("game".to_string()));

        monitor.detect_changes(current);

        // Verify Started event is sent
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Started(AppIdentifier::Win32(name)) => {
                assert_eq!(name, "game");
            }
            ProcessEvent::Stopped(_) => {
                panic!("Expected Started event for state transition, got Stopped")
            }
            _ => panic!("Expected Win32 Started event"),
        }
    }

    #[test]
    fn test_state_transition_running_to_not_running() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec!["game".to_string()]);

        // Initial state: process running
        let mut initial = HashSet::new();
        initial.insert(AppIdentifier::Win32("game".to_string()));
        monitor.running_processes = initial;

        // Transition: process stops
        let current = HashSet::new();

        monitor.detect_changes(current);

        // Verify Stopped event is sent
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Stopped(AppIdentifier::Win32(name)) => {
                assert_eq!(name, "game");
            }
            ProcessEvent::Started(_) => {
                panic!("Expected Stopped event for state transition, got Started")
            }
            _ => panic!("Expected Win32 Stopped event"),
        }
    }

    #[test]
    fn test_multiple_state_transitions() {
        let (tx, rx) = mpsc::sync_channel(32);
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec![
            "app1".to_string(),
            "app2".to_string(),
            "app3".to_string(),
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

        monitor.update_watch_list(vec!["game".to_string()]);

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
        monitor.update_watch_list(vec!["game".to_string()]);

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
            ProcessEvent::Stopped(_) => panic!("Expected Started event for game, got Stopped"),
            _ => panic!("Expected Win32 Started event"),
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

    #[test]
    fn test_process_name_normalization() {
        let (tx, _rx) = mpsc::sync_channel(32);
        let monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Add processes with various cases
        monitor.update_watch_list(vec![
            "Game.exe".to_string(),
            "NOTEPAD".to_string(),
            "MyApp.EXE".to_string(),
        ]);

        let watch_list = monitor.watch_list.lock();

        // All should be normalized to lowercase
        assert!(watch_list.contains("game.exe"));
        assert!(watch_list.contains("notepad"));
        assert!(watch_list.contains("myapp.exe"));
        assert_eq!(watch_list.len(), 3);
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

            /// Property: Watch list normalization is idempotent
            #[test]
            fn watch_list_normalization_is_idempotent(
                names in prop::collection::vec("[a-zA-Z0-9_-]+(\\.exe)?", 1..10)
            ) {
                let (tx, _rx) = mpsc::sync_channel(32);
                let monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

                // Normalize once
                monitor.update_watch_list(names.clone());
                let first_result = monitor.watch_list.lock().clone();

                // Normalize again with the same input
                monitor.update_watch_list(names);
                let second_result = monitor.watch_list.lock().clone();

                // Results should be identical
                prop_assert_eq!(first_result, second_result);
            }
        }
    }
}
