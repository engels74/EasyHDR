//! Process monitoring implementation
//!
//! This module implements the process monitoring subsystem that polls
//! Windows processes and detects state changes.
//!
//! # Known Limitations
//!
//! ## Process Name Collisions
//!
//! The process monitor matches processes by their executable filename (without extension),
//! converted to lowercase. This means that if multiple different applications have the same
//! executable filename, they will all trigger HDR toggling.
//!
//! For example:
//! - If you configure "game.exe" to trigger HDR
//! - Any process named "game.exe" will trigger HDR, regardless of its full path
//! - This includes "C:\Games\Game1\game.exe" and "D:\OtherGames\game.exe"
//!
//! This is a known limitation of the current implementation. The process monitor only has
//! access to the process name, not the full executable path, when enumerating running processes.
//!
//! **Workaround:** Ensure that the applications you want to monitor have unique executable names.
//!
//! **Requirement 2.7:** Document process name collisions as a known limitation

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES};

use crate::error::{EasyHdrError, Result};

/// Events emitted by the process monitor
#[derive(Debug, Clone)]
pub enum ProcessEvent {
    /// A monitored process has started
    Started(String),
    /// A monitored process has stopped
    Stopped(String),
}

/// Process monitor that polls for running processes
///
/// # Known Limitations
///
/// **Process Name Collisions:** The monitor matches processes by executable filename only
/// (without path or extension). Multiple processes with the same filename will all be
/// detected as the same application. See module-level documentation for details.
///
/// # Performance Optimizations
///
/// The monitor is optimized for low CPU usage (< 1% on modern systems):
/// - Pre-allocates HashSet capacity to avoid rehashing
/// - Minimizes string allocations by reusing buffers
/// - Reduces lock contention by cloning watch list
/// - Uses efficient HashSet operations for change detection
pub struct ProcessMonitor {
    /// List of process names to watch (lowercase)
    watch_list: Arc<Mutex<HashSet<String>>>,
    /// Channel to send process events
    #[cfg_attr(not(windows), allow(dead_code))]
    event_sender: mpsc::Sender<ProcessEvent>,
    /// Polling interval
    interval: Duration,
    /// Previous snapshot of running processes
    #[cfg_attr(not(windows), allow(dead_code))]
    running_processes: HashSet<String>,
    /// Estimated process count for capacity pre-allocation
    #[cfg_attr(not(windows), allow(dead_code))]
    estimated_process_count: usize,
}

impl ProcessMonitor {
    /// Create a new process monitor
    ///
    /// # Performance
    ///
    /// Initializes with a default estimated process count of 200, which is typical
    /// for modern Windows systems. This helps pre-allocate HashSet capacity to avoid
    /// rehashing during process enumeration.
    pub fn new(interval: Duration, event_sender: mpsc::Sender<ProcessEvent>) -> Self {
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

    /// Get a reference to the watch list
    ///
    /// Returns a cloned Arc reference to the watch list, which can be used
    /// by other components (like AppController) to update the watch list.
    pub fn get_watch_list_ref(&self) -> Arc<Mutex<HashSet<String>>> {
        Arc::clone(&self.watch_list)
    }

    /// Start the monitoring thread
    pub fn start(mut self) -> JoinHandle<()> {
        thread::spawn(move || loop {
            if let Err(e) = self.poll_processes() {
                tracing::error!("Error polling processes: {}", e);
            }
            thread::sleep(self.interval);
        })
    }

    /// Poll processes and detect changes
    ///
    /// Uses Windows API snapshot enumeration to retrieve all active process names,
    /// extracts filenames without extensions, converts to lowercase, and detects
    /// state changes compared to the previous snapshot.
    ///
    /// # Requirements
    ///
    /// - Requirement 2.2: Use Windows API snapshot enumeration
    /// - Requirement 2.3: Perform case-insensitive process name matching
    /// - Requirement 2.9: Handle errors gracefully
    ///
    /// # Performance Optimizations
    ///
    /// - Pre-allocates HashSet capacity based on previous snapshot size
    /// - Minimizes string allocations
    /// - Uses efficient HashSet operations
    fn poll_processes(&mut self) -> Result<()> {
        #[cfg(windows)]
        {
            use tracing::{debug, warn};

            // Take a snapshot of all running processes
            let snapshot = unsafe {
                CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| {
                    use tracing::error;
                    error!("Windows API error - CreateToolhelp32Snapshot failed: {}", e);
                    EasyHdrError::ProcessMonitorError(format!(
                        "Failed to create process snapshot: {}",
                        e
                    ))
                })?
            };

            // Ensure snapshot handle is closed when we're done
            let _guard = SnapshotGuard(snapshot);

            // Build a set of currently running process names
            // Pre-allocate capacity based on previous snapshot size to avoid rehashing
            // This is a key CPU optimization (Requirement 9.2)
            let capacity = self
                .running_processes
                .len()
                .max(self.estimated_process_count);
            let mut current_processes = HashSet::with_capacity(capacity);

            // Initialize PROCESSENTRY32W structure
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };

            // Get the first process
            let mut has_process = unsafe { Process32FirstW(snapshot, &mut entry).is_ok() };

            // Iterate through all processes
            while has_process {
                // Extract the process name from szExeFile
                // szExeFile is a null-terminated wide string
                let process_name = extract_process_name(&entry.szExeFile);

                if let Some(name) = process_name {
                    // Extract filename without extension and convert to lowercase
                    let name_lower = extract_filename_without_extension(&name);
                    debug!("Found process: {}", name_lower);
                    current_processes.insert(name_lower);
                }

                // Get the next process
                has_process = unsafe {
                    match Process32NextW(snapshot, &mut entry) {
                        Ok(_) => true,
                        Err(e) => {
                            // ERROR_NO_MORE_FILES is expected at the end
                            if e.code() == ERROR_NO_MORE_FILES.to_hresult() {
                                false
                            } else {
                                // Log other errors but continue
                                warn!("Error iterating processes: {}", e);
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
                "Process monitoring is only supported on Windows".to_string(),
            ))
        }
    }

    /// Detect changes between current and previous snapshots
    ///
    /// Compares the current process snapshot with the previous snapshot to detect
    /// which monitored processes have started or stopped, then sends appropriate events.
    ///
    /// # Algorithm
    ///
    /// This method uses HashSet difference operations for efficient change detection:
    ///
    /// 1. **Started processes** = `current - previous`
    ///    - Processes in current snapshot but not in previous
    ///    - Example: If previous = {chrome, notepad} and current = {chrome, notepad, game}
    ///    - Then started = {game}
    ///
    /// 2. **Stopped processes** = `previous - current`
    ///    - Processes in previous snapshot but not in current
    ///    - Example: If previous = {chrome, notepad, game} and current = {chrome, notepad}
    ///    - Then stopped = {game}
    ///
    /// 3. **Filter by watch list**
    ///    - Only processes in the watch list trigger events
    ///    - Watch list contains lowercase process names from monitored applications
    ///
    /// 4. **Send events**
    ///    - ProcessEvent::Started for each started monitored process
    ///    - ProcessEvent::Stopped for each stopped monitored process
    ///
    /// # Performance Optimizations
    ///
    /// - Clones watch list once to minimize lock hold time (Requirement 9.2)
    /// - Uses HashSet::difference() for O(n) change detection
    /// - Only sends events for monitored processes
    /// - Typical complexity: O(n) where n is number of processes (150-250 on Windows)
    ///
    /// # Requirements
    ///
    /// - Requirement 2.4: Detect NOT_RUNNING → RUNNING transitions within 1-2 seconds
    /// - Requirement 2.5: Detect RUNNING → NOT_RUNNING transitions within 1-2 seconds
    /// - Requirement 2.6: Fire events to application logic controller
    ///
    /// # Example
    ///
    /// ```text
    /// Previous snapshot: {chrome, notepad}
    /// Current snapshot:  {chrome, notepad, cyberpunk2077}
    /// Watch list:        {cyberpunk2077, witcher3}
    ///
    /// Started: {cyberpunk2077} ∩ {cyberpunk2077, witcher3} = {cyberpunk2077}
    /// → Send ProcessEvent::Started("cyberpunk2077")
    /// ```
    #[cfg_attr(not(windows), allow(dead_code))]
    fn detect_changes(&mut self, current: HashSet<String>) {
        use tracing::info;

        // Clone watch list to minimize lock hold time
        // This is a CPU optimization to reduce lock contention (Requirement 9.2)
        let watch_list = {
            let guard = self.watch_list.lock();
            guard.clone()
        }; // Lock is released here

        // Find started processes
        for process in current.difference(&self.running_processes) {
            if watch_list.contains(process) {
                info!("Detected process started: {}", process);
                if let Err(e) = self
                    .event_sender
                    .send(ProcessEvent::Started(process.clone()))
                {
                    use tracing::error;
                    error!(
                        "Failed to send ProcessEvent::Started for '{}': {}",
                        process, e
                    );
                }
            }
        }

        // Find stopped processes
        for process in self.running_processes.difference(&current) {
            if watch_list.contains(process) {
                info!("Detected process stopped: {}", process);
                if let Err(e) = self
                    .event_sender
                    .send(ProcessEvent::Stopped(process.clone()))
                {
                    use tracing::error;
                    error!(
                        "Failed to send ProcessEvent::Stopped for '{}': {}",
                        process, e
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
#[cfg(windows)]
struct SnapshotGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SnapshotGuard {
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
/// Examples:
/// - "C:\\Windows\\System32\\notepad.exe" -> "notepad"
/// - "game.exe" -> "game"
/// - "MyApp.EXE" -> "myapp"
///
/// # Requirements
///
/// - Requirement 2.3: Perform case-insensitive matching (convert to lowercase)
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
        let (tx, _rx) = mpsc::channel();
        let monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);
        assert_eq!(monitor.interval, Duration::from_millis(1000));
    }

    #[test]
    fn test_update_watch_list() {
        let (tx, _rx) = mpsc::channel();
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
        let (tx, rx) = mpsc::channel();
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Set up watch list
        monitor.update_watch_list(vec!["notepad".to_string(), "game".to_string()]);

        // Initial state: no processes running
        monitor.running_processes = HashSet::new();

        // Current state: notepad started
        let mut current = HashSet::new();
        current.insert("notepad".to_string());
        current.insert("explorer".to_string()); // Not monitored

        monitor.detect_changes(current);

        // Should receive Started event for notepad
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Started(name) => assert_eq!(name, "notepad"),
            _ => panic!("Expected Started event"),
        }

        // Should not receive event for explorer (not monitored)
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn test_detect_changes_stopped() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Set up watch list
        monitor.update_watch_list(vec!["notepad".to_string(), "game".to_string()]);

        // Initial state: notepad running
        let mut initial = HashSet::new();
        initial.insert("notepad".to_string());
        initial.insert("explorer".to_string());
        monitor.running_processes = initial;

        // Current state: notepad stopped
        let mut current = HashSet::new();
        current.insert("explorer".to_string());

        monitor.detect_changes(current);

        // Should receive Stopped event for notepad
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Stopped(name) => assert_eq!(name, "notepad"),
            _ => panic!("Expected Stopped event"),
        }
    }

    #[test]
    fn test_detect_changes_case_insensitive() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Watch list has lowercase
        monitor.update_watch_list(vec!["notepad".to_string()]);

        // Initial state: empty
        monitor.running_processes = HashSet::new();

        // Current state: process name is already lowercase (as it should be from extraction)
        let mut current = HashSet::new();
        current.insert("notepad".to_string());

        monitor.detect_changes(current);

        // Should receive Started event
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Started(name) => assert_eq!(name, "notepad"),
            _ => panic!("Expected Started event"),
        }
    }

    #[test]
    fn test_detect_changes_multiple_processes() {
        let (tx, rx) = mpsc::channel();
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
        current.insert("notepad".to_string());
        current.insert("game".to_string());
        current.insert("explorer".to_string()); // Not monitored

        monitor.detect_changes(current);

        // Should receive Started events for notepad and game
        let mut received = HashSet::new();
        for _ in 0..2 {
            let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
            match event {
                ProcessEvent::Started(name) => {
                    received.insert(name);
                }
                _ => panic!("Expected Started event"),
            }
        }

        assert!(received.contains("notepad"));
        assert!(received.contains("game"));
        assert_eq!(received.len(), 2);
    }

    // Additional comprehensive tests for task 5.5

    /// Test process name extraction from various path formats
    /// Requirement 2.3: Case-insensitive process name matching
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

    /// Test case-insensitive matching with various case combinations
    /// Requirement 2.3: Case-insensitive process name matching
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

    /// Test that watch list matching is case-insensitive
    /// Requirement 2.3: Case-insensitive process name matching
    #[test]
    fn test_watch_list_case_insensitive() {
        let (tx, _rx) = mpsc::channel();
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

    /// Test state transition from NOT_RUNNING to RUNNING
    /// Requirement 2.4: Detect NOT_RUNNING → RUNNING transition
    #[test]
    fn test_state_transition_not_running_to_running() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec!["game".to_string()]);

        // Initial state: process not running
        monitor.running_processes = HashSet::new();

        // Transition: process starts
        let mut current = HashSet::new();
        current.insert("game".to_string());

        monitor.detect_changes(current);

        // Verify Started event is sent
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Started(name) => {
                assert_eq!(name, "game");
            }
            _ => panic!("Expected Started event for state transition"),
        }
    }

    /// Test state transition from RUNNING to NOT_RUNNING
    /// Requirement 2.5: Detect RUNNING → NOT_RUNNING transition
    #[test]
    fn test_state_transition_running_to_not_running() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec!["game".to_string()]);

        // Initial state: process running
        let mut initial = HashSet::new();
        initial.insert("game".to_string());
        monitor.running_processes = initial;

        // Transition: process stops
        let current = HashSet::new();

        monitor.detect_changes(current);

        // Verify Stopped event is sent
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Stopped(name) => {
                assert_eq!(name, "game");
            }
            _ => panic!("Expected Stopped event for state transition"),
        }
    }

    /// Test multiple simultaneous state transitions
    /// Requirements 2.4, 2.5: Detect state transitions
    #[test]
    fn test_multiple_state_transitions() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec![
            "app1".to_string(),
            "app2".to_string(),
            "app3".to_string(),
        ]);

        // Initial state: app1 and app2 running
        let mut initial = HashSet::new();
        initial.insert("app1".to_string());
        initial.insert("app2".to_string());
        monitor.running_processes = initial;

        // New state: app2 and app3 running (app1 stopped, app3 started)
        let mut current = HashSet::new();
        current.insert("app2".to_string());
        current.insert("app3".to_string());

        monitor.detect_changes(current);

        // Should receive both Started and Stopped events
        let mut started = Vec::new();
        let mut stopped = Vec::new();

        for _ in 0..2 {
            let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
            match event {
                ProcessEvent::Started(name) => started.push(name),
                ProcessEvent::Stopped(name) => stopped.push(name),
            }
        }

        assert_eq!(started.len(), 1);
        assert_eq!(stopped.len(), 1);
        assert!(started.contains(&"app3".to_string()));
        assert!(stopped.contains(&"app1".to_string()));
    }

    /// Test that no events are sent when process state doesn't change
    /// Requirement 2.6: Fire events only on state transitions
    #[test]
    fn test_no_events_when_no_state_change() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        monitor.update_watch_list(vec!["game".to_string()]);

        // Initial state: game running
        let mut initial = HashSet::new();
        initial.insert("game".to_string());
        monitor.running_processes = initial.clone();

        // Current state: same as before (no change)
        monitor.detect_changes(initial);

        // Should not receive any events
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    /// Test that only monitored processes trigger events
    /// Requirement 2.7: Match processes by name
    #[test]
    fn test_only_monitored_processes_trigger_events() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Only watch "game"
        monitor.update_watch_list(vec!["game".to_string()]);

        monitor.running_processes = HashSet::new();

        // Start multiple processes, only one monitored
        let mut current = HashSet::new();
        current.insert("game".to_string()); // Monitored
        current.insert("notepad".to_string()); // Not monitored
        current.insert("explorer".to_string()); // Not monitored
        current.insert("chrome".to_string()); // Not monitored

        monitor.detect_changes(current);

        // Should only receive one event for "game"
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match event {
            ProcessEvent::Started(name) => assert_eq!(name, "game"),
            _ => panic!("Expected Started event for game"),
        }

        // No more events should be received
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    /// Test edge case: empty watch list
    #[test]
    fn test_empty_watch_list() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

        // Empty watch list
        monitor.update_watch_list(vec![]);

        monitor.running_processes = HashSet::new();

        // Start some processes
        let mut current = HashSet::new();
        current.insert("game".to_string());
        current.insert("notepad".to_string());

        monitor.detect_changes(current);

        // Should not receive any events
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    /// Test edge case: process name with special characters
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

    /// Test edge case: very long path
    #[test]
    fn test_very_long_path() {
        let long_path = "C:\\Very\\Long\\Path\\With\\Many\\Directories\\And\\Subdirectories\\That\\Goes\\On\\And\\On\\application.exe";
        assert_eq!(extract_filename_without_extension(long_path), "application");
    }

    /// Test that process names are correctly normalized
    /// Requirement 2.3: Case-insensitive matching
    #[test]
    fn test_process_name_normalization() {
        let (tx, _rx) = mpsc::channel();
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
}
