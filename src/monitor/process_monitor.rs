//! Process monitoring implementation
//!
//! This module implements the process monitoring subsystem that polls
//! Windows processes and detects state changes.

use std::collections::HashSet;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use parking_lot::Mutex;

#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW,
    PROCESSENTRY32W, TH32CS_SNAPPROCESS,
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
pub struct ProcessMonitor {
    /// List of process names to watch (lowercase)
    watch_list: Arc<Mutex<HashSet<String>>>,
    /// Channel to send process events
    event_sender: mpsc::Sender<ProcessEvent>,
    /// Polling interval
    interval: Duration,
    /// Previous snapshot of running processes
    running_processes: HashSet<String>,
}

impl ProcessMonitor {
    /// Create a new process monitor
    pub fn new(interval: Duration, event_sender: mpsc::Sender<ProcessEvent>) -> Self {
        Self {
            watch_list: Arc::new(Mutex::new(HashSet::new())),
            event_sender,
            interval,
            running_processes: HashSet::new(),
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
    /// Uses Windows API snapshot enumeration to retrieve all active process names,
    /// extracts filenames without extensions, converts to lowercase, and detects
    /// state changes compared to the previous snapshot.
    ///
    /// # Requirements
    ///
    /// - Requirement 2.2: Use Windows API snapshot enumeration
    /// - Requirement 2.3: Perform case-insensitive process name matching
    /// - Requirement 2.9: Handle errors gracefully
    fn poll_processes(&mut self) -> Result<()> {
        #[cfg(windows)]
        {
            use tracing::{debug, warn};

            // Take a snapshot of all running processes
            let snapshot = unsafe {
                CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
                    .map_err(|e| {
                        EasyHdrError::ProcessMonitorError(format!(
                            "Failed to create process snapshot: {}",
                            e
                        ))
                    })?
            };

            // Ensure snapshot handle is closed when we're done
            let _guard = SnapshotGuard(snapshot);

            // Build a set of currently running process names
            let mut current_processes = HashSet::new();

            // Initialize PROCESSENTRY32W structure
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };

            // Get the first process
            let mut has_process = unsafe {
                Process32FirstW(snapshot, &mut entry)
                    .is_ok()
            };

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
                "Process monitoring is only supported on Windows".to_string()
            ))
        }
    }

    /// Detect changes between current and previous snapshots
    fn detect_changes(&mut self, current: HashSet<String>) {
        use tracing::info;

        let watch_list = self.watch_list.lock();

        // Find started processes
        for process in current.difference(&self.running_processes) {
            if watch_list.contains(process) {
                info!("Detected process started: {}", process);
                let _ = self.event_sender.send(ProcessEvent::Started(process.clone()));
            }
        }

        // Find stopped processes
        for process in self.running_processes.difference(&current) {
            if watch_list.contains(process) {
                info!("Detected process stopped: {}", process);
                let _ = self.event_sender.send(ProcessEvent::Stopped(process.clone()));
            }
        }

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
    let len = sz_exe_file.iter()
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
        assert_eq!(
            extract_filename_without_extension("game.exe"),
            "game"
        );

        // Test uppercase extension (should be lowercase)
        assert_eq!(
            extract_filename_without_extension("MyApp.EXE"),
            "myapp"
        );

        // Test mixed case
        assert_eq!(
            extract_filename_without_extension("C:\\Games\\Cyberpunk2077.exe"),
            "cyberpunk2077"
        );

        // Test no extension
        assert_eq!(
            extract_filename_without_extension("process"),
            "process"
        );

        // Test Unix-style path (edge case)
        assert_eq!(
            extract_filename_without_extension("/usr/bin/app.exe"),
            "app"
        );

        // Test multiple dots
        assert_eq!(
            extract_filename_without_extension("my.app.exe"),
            "my.app"
        );
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
}

