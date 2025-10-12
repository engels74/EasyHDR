//! Process monitoring implementation
//!
//! This module implements the process monitoring subsystem that polls
//! Windows processes and detects state changes.

use std::collections::HashSet;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use parking_lot::Mutex;

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

    /// Poll processes and detect changes (placeholder implementation)
    fn poll_processes(&mut self) -> crate::error::Result<()> {
        // TODO: Implement actual Windows API process enumeration
        // For now, this is a placeholder that will be implemented in task 5
        Ok(())
    }

    /// Detect changes between current and previous snapshots
    #[allow(dead_code)]
    fn detect_changes(&mut self, current: HashSet<String>) {
        let watch_list = self.watch_list.lock();
        
        // Find started processes
        for process in current.difference(&self.running_processes) {
            if watch_list.contains(process) {
                let _ = self.event_sender.send(ProcessEvent::Started(process.clone()));
            }
        }
        
        // Find stopped processes
        for process in self.running_processes.difference(&current) {
            if watch_list.contains(process) {
                let _ = self.event_sender.send(ProcessEvent::Stopped(process.clone()));
            }
        }
        
        self.running_processes = current;
    }
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
}

