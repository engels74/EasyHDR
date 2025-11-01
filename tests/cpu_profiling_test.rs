//! CPU profiling test for flamegraph analysis
//!
//! This test exercises the hot paths in process monitoring and event handling
//! to generate meaningful CPU profiles with symbolicated function names.
//!
//! **Usage with cargo-flamegraph:**
//! ```cmd
//! # Generate flamegraph (automatically builds with profiling profile)
//! cargo flamegraph --profile profiling --test cpu_profiling_test --output cpu-flamegraph.svg -- --exact --nocapture profile_process_monitoring_hot_paths
//!
//! # View: Open cpu-flamegraph.svg in any browser
//! # - Click function boxes to zoom in
//! # - Use search box to find specific functions (e.g., "poll_processes")
//! # - Box width = CPU time percentage
//! ```
//!
//! **Expected flamegraph hotspots:**
//! - `poll_processes` should consume >20% CPU time (>20% flamegraph width)
//! - `handle_process_event` should show >5% CPU time (>5% flamegraph width)
//! - Windows API calls: `CreateToolhelp32Snapshot`, `Process32FirstW`, `Process32NextW`
//! - String allocations in process name extraction

#![cfg(windows)]

use easyhdr::config::{AppConfig, MonitoredApp, UserPreferences, Win32App, WindowState};
use easyhdr::controller::AppController;
use easyhdr::monitor::{ProcessMonitor, WatchState};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Profile the process monitoring hot paths with realistic workload
///
/// This test runs for 30 seconds to collect sufficient samples for profiling.
/// It exercises both `poll_processes` (process enumeration) and `handle_process_event`
/// (event handling logic) under realistic conditions.
#[test]
fn profile_process_monitoring_hot_paths() {
    println!("\n=== CPU Profiling Test ===");
    println!("This test will run for 30 seconds to collect CPU samples.");
    println!("Expected hotspots:");
    println!("  - poll_processes (>20% CPU)");
    println!("  - handle_process_event (>5% CPU)");
    println!("  - CreateToolhelp32Snapshot (Windows API)");
    println!("  - String allocations in process name extraction\n");

    // Create a realistic configuration with multiple monitored apps
    let config = create_profiling_config();

    // Set up channels for process events and HDR state events
    let (process_tx, process_rx) = mpsc::sync_channel(32);
    let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
    let (state_tx, state_rx) = mpsc::sync_channel(32);

    // Create watch list with monitored apps
    let apps = create_monitored_apps();

    // Create process monitor with aggressive polling (500ms) to maximize CPU usage
    let monitor = ProcessMonitor::new(Duration::from_millis(500), process_tx);
    monitor.update_watch_list(apps);
    let watch_state = monitor.get_watch_state_ref();

    // Create app controller
    let mut controller =
        AppController::new(config, process_rx, hdr_state_rx, state_tx, watch_state)
            .expect("Failed to create AppController");

    // Start the process monitor thread (exercises poll_processes)
    let _monitor_handle = monitor.start();

    // Start the event loop in a separate thread (exercises handle_process_event)
    let _event_handle = thread::spawn(move || {
        controller.run();
    });

    // Consume state updates to prevent channel from filling up
    let _state_consumer = thread::spawn(move || {
        while state_rx.recv().is_ok() {
            // Just drain the channel
        }
    });

    // Run for 30 seconds to collect sufficient samples
    println!("Starting profiling workload...");
    let start = Instant::now();
    let duration = Duration::from_secs(30);

    while start.elapsed() < duration {
        let elapsed = start.elapsed().as_secs();
        if elapsed > 0 && elapsed % 5 == 0 {
            println!("  Profiling... {}s / {}s", elapsed, duration.as_secs());
        }
        thread::sleep(Duration::from_secs(1));
    }

    println!("Profiling complete.");

    // Note: We can't gracefully stop the threads (they run in infinite loops)
    // but that's okay for profiling purposes - the process will exit and clean up

    println!("\n=== Profiling Test Complete ===");
    println!("Next steps:");
    println!("1. Open cpu-flamegraph.svg in any web browser");
    println!("2. Search for 'poll_processes' and 'handle_process_event'");
    println!("3. Verify poll_processes is >20% of flamegraph width");
}

/// Create a realistic configuration for profiling
fn create_profiling_config() -> AppConfig {
    let preferences = UserPreferences {
        monitoring_interval_ms: 500, // Aggressive polling for profiling
        auto_start: false,
        ..Default::default()
    };

    AppConfig {
        monitored_apps: create_monitored_apps(),
        preferences,
        window_state: WindowState::default(),
    }
}

/// Create a list of monitored applications for profiling
///
/// Uses common Windows applications that might be running during profiling
fn create_monitored_apps() -> Vec<MonitoredApp> {
    vec![
        MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Google Chrome".to_string(),
            exe_path: PathBuf::from("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"),
            process_name: "chrome".to_string(),
            enabled: true,
            icon_data: None,
        }),
        MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Mozilla Firefox".to_string(),
            exe_path: PathBuf::from("C:\\Program Files\\Mozilla Firefox\\firefox.exe"),
            process_name: "firefox".to_string(),
            enabled: true,
            icon_data: None,
        }),
        MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "OBS Studio".to_string(),
            exe_path: PathBuf::from("C:\\Program Files\\obs-studio\\bin\\64bit\\obs64.exe"),
            process_name: "obs64".to_string(),
            enabled: true,
            icon_data: None,
        }),
        MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Visual Studio Code".to_string(),
            exe_path: PathBuf::from("C:\\Program Files\\Microsoft VS Code\\Code.exe"),
            process_name: "code".to_string(),
            enabled: true,
            icon_data: None,
        }),
        MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: "Notepad".to_string(),
            exe_path: PathBuf::from("C:\\Windows\\System32\\notepad.exe"),
            process_name: "notepad".to_string(),
            enabled: true,
            icon_data: None,
        }),
    ]
}

/// Benchmark-style test to measure `poll_processes` throughput
///
/// This test focuses specifically on the process enumeration hot path
#[test]
fn profile_poll_processes_throughput() {
    println!("\n=== poll_processes Throughput Test ===");

    let (tx, _rx) = mpsc::sync_channel(32);
    let monitor = ProcessMonitor::new(Duration::from_millis(100), tx);
    monitor.update_watch_list(create_monitored_apps());

    // Start monitoring
    let _handle = monitor.start();

    // Run for 30 seconds
    println!("Profiling poll_processes for 30 seconds...");
    thread::sleep(Duration::from_secs(30));

    println!("poll_processes profiling complete");
}

/// Benchmark-style test to measure `handle_process_event` throughput
///
/// This test focuses specifically on the event handling hot path by directly
/// calling the internal `handle_process_event` method (via reflection/testing API)
#[test]
fn profile_handle_process_event_throughput() {
    use easyhdr::monitor::{AppIdentifier, ProcessEvent};

    println!("\n=== handle_process_event Throughput Test ===");

    let config = create_profiling_config();
    let (process_tx, process_rx) = mpsc::sync_channel(32);
    let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
    let (state_tx, state_rx) = mpsc::sync_channel(32);
    // Use double-Arc pattern to match production implementation
    let apps = create_monitored_apps();
    let watch_state = Arc::new(RwLock::new(WatchState::new()));

    let mut controller =
        AppController::new(config, process_rx, hdr_state_rx, state_tx, watch_state)
            .expect("Failed to create AppController");

    // Start event loop to process events in a separate thread
    let _event_handle = thread::spawn(move || {
        controller.run();
    });

    // Consume state updates
    let _state_consumer = thread::spawn(move || while state_rx.recv().is_ok() {});

    // Simulate rapid event generation
    println!("Generating 10,000 events...");
    let start = Instant::now();

    for i in 0..10_000 {
        let app_id = if i % 2 == 0 {
            AppIdentifier::Win32("chrome".to_string())
        } else {
            AppIdentifier::Win32("firefox".to_string())
        };

        let event = if i % 4 < 2 {
            ProcessEvent::Started(app_id)
        } else {
            ProcessEvent::Stopped(app_id)
        };

        // Send event through the channel (will be processed by event loop)
        let _ = process_tx.send(event);
    }

    // Wait a bit for events to be processed
    thread::sleep(Duration::from_secs(2));

    let elapsed = start.elapsed();
    println!(
        "Generated 10,000 events in {:.2}s ({:.0} events/sec)",
        elapsed.as_secs_f64(),
        10_000.0 / elapsed.as_secs_f64()
    );
}
