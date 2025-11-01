//! DHAT allocation profiling test for baseline measurements
//!
//! This test instruments the application with DHAT to measure allocation patterns
//! in config operations, watch list cloning, and string allocations.
//!
//! Note: Direct `ProcessMonitor` profiling requires running the full application
//! with DHAT instrumentation. Use the profiling guide for full profiling instructions.
//!
//! # Usage
//!
//! ```bash
//! # From WSL2 (cross-compile)
//! cargo test --test dhat_profiling_test --release --target x86_64-pc-windows-msvc
//!
//! # From Windows (if Rust installed)
//! cargo test --test dhat_profiling_test --release
//!
//! # View results at: https://nnethercote.github.io/dh_view/dh_view.html
//! ```
//!
//! # Success Criteria
//!
//! - Watch list cloning overhead measured
//! - Config lookup (O(n)) allocation patterns documented
//! - String allocation patterns from process name operations

#![cfg(test)]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use easyhdr::config::models::Win32App;
use easyhdr::config::{AppConfig, MonitoredApp, UserPreferences, WindowState};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

// Additional imports for production workload profiling
#[cfg(windows)]
use easyhdr::controller::AppController;
#[cfg(windows)]
use easyhdr::monitor::ProcessMonitor;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::sync::mpsc;
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, Instant};

/// Create a realistic configuration for profiling
fn create_realistic_config(num_apps: usize) -> AppConfig {
    let mut config = AppConfig {
        monitored_apps: Vec::with_capacity(num_apps),
        preferences: UserPreferences {
            auto_start: false,
            monitoring_interval_ms: 1000,
            show_tray_notifications: false,
            show_update_notifications: false,
            minimize_to_tray_on_minimize: false,
            minimize_to_tray_on_close: false,
            start_minimized_to_tray: false,
            last_update_check_time: 0,
            cached_latest_version: String::new(),
        },
        window_state: WindowState {
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        },
    };

    // Realistic application paths that might be monitored
    let apps = vec![
        (
            "chrome.exe",
            "Google Chrome",
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        ),
        (
            "firefox.exe",
            "Mozilla Firefox",
            "C:\\Program Files\\Mozilla Firefox\\firefox.exe",
        ),
        (
            "msedge.exe",
            "Microsoft Edge",
            "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
        ),
        (
            "Code.exe",
            "Visual Studio Code",
            "C:\\Users\\User\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe",
        ),
        (
            "Photoshop.exe",
            "Adobe Photoshop",
            "C:\\Program Files\\Adobe\\Adobe Photoshop 2024\\Photoshop.exe",
        ),
        (
            "Premiere Pro.exe",
            "Adobe Premiere Pro",
            "C:\\Program Files\\Adobe\\Adobe Premiere Pro 2024\\Adobe Premiere Pro.exe",
        ),
        (
            "blender.exe",
            "Blender",
            "C:\\Program Files\\Blender Foundation\\Blender 4.0\\blender.exe",
        ),
        (
            "obs64.exe",
            "OBS Studio",
            "C:\\Program Files\\obs-studio\\bin\\64bit\\obs64.exe",
        ),
        (
            "steam.exe",
            "Steam",
            "C:\\Program Files (x86)\\Steam\\steam.exe",
        ),
        (
            "Discord.exe",
            "Discord",
            "C:\\Users\\User\\AppData\\Local\\Discord\\app-1.0.9015\\Discord.exe",
        ),
    ];

    for &(process_name, display_name, path) in apps.iter().take(num_apps.min(apps.len())) {
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: display_name.to_string(),
            exe_path: PathBuf::from(path),
            process_name: process_name.to_string(),
            enabled: true,
            icon_data: None,
        }));
    }

    config
}

/// Profile watch list cloning overhead
///
/// This test measures allocation overhead from repeated watch list cloning,
/// which currently occurs on every config access.
///
/// **Note:** This test is for isolated profiling of watch list cloning only.
/// For full production workload profiling, use `profile_production_allocation_patterns`.
#[test]
#[ignore = "Run explicitly for isolated watch list profiling: cargo test --test dhat_profiling_test --ignored -- profile_watch_list_cloning"]
fn profile_watch_list_cloning() {
    let _profiler = dhat::Profiler::new_heap();

    let config = create_realistic_config(50);
    let config_arc = Arc::new(parking_lot::Mutex::new(config));

    println!("DHAT Profiling: Watch list cloning (50 apps, 1000 clones)");

    // Simulate repeated watch list cloning (current implementation pattern)
    for i in 0..1000 {
        let guard = config_arc.lock();
        let _apps_clone = guard.monitored_apps.clone();
        drop(guard);

        if (i + 1) % 250 == 0 {
            println!("  Completed {} clone operations", i + 1);
        }
    }

    println!("DHAT profiling complete. Check dhat-heap.json for results.");
}

/// Profile monitored app lookups with varying sizes
///
/// This test measures allocation and performance overhead from O(n) lookups
/// in the monitored apps list. Target is O(1) `HashSet` lookup.
#[test]
#[ignore = "Run explicitly with: cargo test --test dhat_profiling_test --ignored -- profile_monitored_app_lookups"]
fn profile_monitored_app_lookups() {
    let _profiler = dhat::Profiler::new_heap();

    let config = create_realistic_config(50);
    let config_arc = Arc::new(parking_lot::Mutex::new(config));

    println!("DHAT Profiling: Monitored app lookups (50 apps, 1000 lookups)");

    // Simulate repeated lookups (event handling pattern)
    let target_processes = vec!["chrome.exe", "firefox.exe", "obs64.exe", "nonexistent.exe"];

    for i in 0..1000 {
        let guard = config_arc.lock();
        let apps = &guard.monitored_apps;

        for target in &target_processes {
            let _found = apps.iter().any(|app| {
                if let MonitoredApp::Win32(win32_app) = app {
                    win32_app.process_name == *target
                } else {
                    false
                }
            });
        }

        drop(guard);

        if (i + 1) % 250 == 0 {
            println!("  Completed {} lookup cycles", i + 1);
        }
    }

    println!("DHAT profiling complete. Check dhat-heap.json for results.");
}

/// Profile config access patterns
///
/// This test measures allocation overhead from config reads and writes.
#[test]
#[ignore = "Run explicitly with: cargo test --test dhat_profiling_test --ignored -- profile_config_access"]
fn profile_config_access() {
    let _profiler = dhat::Profiler::new_heap();

    let config = create_realistic_config(10);
    let config_arc = Arc::new(parking_lot::Mutex::new(config));

    println!("DHAT Profiling: Config access patterns (10 apps, 1000 reads)");

    // Simulate frequent config reads (event handling pattern)
    for i in 0..1000 {
        let guard = config_arc.lock();
        let target = "chrome.exe";

        // O(n) lookup in current implementation
        let _found = guard.monitored_apps.iter().any(|app| {
            if let MonitoredApp::Win32(win32_app) = app {
                win32_app.process_name == target
            } else {
                false
            }
        });

        drop(guard);

        if (i + 1) % 250 == 0 {
            println!("  Completed {} config reads", i + 1);
        }
    }

    println!("DHAT profiling complete. Check dhat-heap.json for results.");
}

/// Profile production allocation patterns
///
/// This test exercises the full `ProcessMonitor` and `AppController` workload
/// for 30 seconds to capture realistic allocation patterns from production code.
///
/// **Test steps:**
/// 1. **Thread verification (up to 10s)**: Waits for `ProcessMonitor` to complete
///    at least 3 poll cycles to ensure threads are healthy before profiling
/// 2. **DHAT profiling (30s)**: Captures allocations from production workload
/// 3. **Shutdown**: Signals threads to stop and waits for cleanup
///
/// **Expected to profile:**
/// - `poll_processes()` string allocations (~250 per poll)
/// - Process name extraction from Windows APIs
/// - `AppIdentifier` creation and cloning
/// - Watch list clones in event handling (`AppController`)
/// - UWP detection allocations
///
/// **Success Criteria:**
/// - Allocation rate: 200-500 allocs/sec
/// - Runtime: ~30 seconds DHAT profiling (plus ~5-10s verification)
/// - Stack traces show `poll_processes`, `detect_uwp_process`, `String::from`
/// - At least 60 poll cycles completed (30s ÷ 500ms = 60)
///
/// **Implementation notes:**
/// - Thread verification prevents capturing failed/non-running threads
/// - Uses atomic counter to verify `ProcessMonitor` is actively polling
/// - Test panics if threads don't start within 10 seconds (fail-fast)
/// - Guideline compliance: Relaxed ordering for diagnostic counter (Line 96-100)
#[test]
#[cfg(windows)]
#[allow(clippy::too_many_lines)]
fn profile_production_allocation_patterns() {
    println!("\n=== DHAT Allocation Profiling Test ===");
    println!("This test will run for ~40 seconds total (up to 10s verification + 30s profiling)");
    println!("Expected allocation patterns:");
    println!("  - Process name extraction (String::from)");
    println!("  - AppIdentifier creation in poll_processes");
    println!("  - Watch list cloning overhead (AppController)");
    println!("  - UWP detection allocations\n");

    // Create a realistic configuration with multiple monitored apps
    let config = create_profiling_config();

    // Set up channels for process events and HDR state events
    let (process_tx, process_rx) = mpsc::sync_channel(32);
    let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
    let (state_tx, state_rx) = mpsc::sync_channel(32);

    // Create watch list with monitored apps
    let apps = create_monitored_apps();

    // Create process monitor with aggressive polling (500ms) to maximize allocations
    let monitor = ProcessMonitor::new(Duration::from_millis(500), process_tx);
    monitor.update_watch_list(apps);

    // Get reference to poll cycle counter BEFORE moving monitor into thread
    // This allows us to verify the monitor is actually running before profiling
    let poll_counter = monitor.get_poll_cycle_count_ref();
    let watch_state = monitor.get_watch_state_ref();

    // Create app controller using mock HDR controller (test-only)
    // The profiling test measures allocation patterns in ProcessMonitor and AppController,
    // but doesn't exercise HDR functionality. Using new_with_mock_hdr() avoids Windows API
    // failures in CI environments while still profiling the complete production workload.
    let mut controller =
        AppController::new_with_mock_hdr(config, process_rx, hdr_state_rx, state_tx, watch_state)
            .expect("Failed to create AppController with mock HDR");

    // Create shutdown signal for graceful thread coordination
    // Guideline Line 96: std::sync::atomic for non-async shutdown signaling
    let shutdown = Arc::new(AtomicBool::new(false));
    let _monitor_shutdown = shutdown.clone();
    let _controller_shutdown = shutdown.clone();
    let consumer_shutdown = shutdown.clone();

    // Start the process monitor thread (exercises poll_processes)
    // Note: Handle intentionally unused - thread runs in infinite loop and will be cleaned up on process exit
    let _monitor_handle = monitor.start();

    // Start the event loop in a separate thread (exercises handle_process_event)
    // Note: Handle intentionally unused - thread runs in infinite loop and will be cleaned up on process exit
    let _event_handle = thread::spawn(move || {
        // Note: AppController::run() has infinite loop - we'll let it run until process exits
        // Thread will be cleaned up when process terminates
        controller.run();
    });

    // Consume state updates to prevent channel from filling up
    let state_consumer = thread::spawn(move || {
        while !consumer_shutdown.load(Ordering::Relaxed) {
            // Drain the channel with timeout to check shutdown signal periodically
            let _ = state_rx.recv_timeout(Duration::from_millis(100));
        }
    });

    // Step 1: Thread verification (wait for threads to start and complete polls)
    println!("Step 1: Verifying threads are running...");
    println!("  Waiting for ProcessMonitor to complete at least 3 poll cycles...\n");

    let verification_start = Instant::now();
    let verification_timeout = Duration::from_secs(10); // 10 seconds max

    loop {
        let cycle_count = poll_counter.load(Ordering::Relaxed);

        if cycle_count >= 3 {
            println!("  ✓ ProcessMonitor verified: {cycle_count} poll cycles completed");
            println!("  ✓ Threads are healthy and allocating\n");
            break;
        }

        assert!(
            verification_start.elapsed() <= verification_timeout,
            "ProcessMonitor failed to complete 3 poll cycles within 10 seconds. \
                Completed: {cycle_count} cycles. This indicates threads are not running or poll_processes() is failing."
        );

        if cycle_count > 0 {
            println!("  In progress... {cycle_count} poll cycles completed");
        }

        thread::sleep(Duration::from_millis(500));
    }

    println!("Step 2: Starting DHAT allocation profiling (30 seconds)...");
    println!("  Threads verified healthy - now capturing allocation patterns\n");

    // NOW start DHAT profiler (after warmup completes)
    let _profiler = dhat::Profiler::new_heap();

    // Run profiling for 30 seconds to collect allocation samples
    let profile_start = Instant::now();
    let profile_duration = Duration::from_secs(30);

    let mut last_print = 0;
    while profile_start.elapsed() < profile_duration {
        let elapsed = profile_start.elapsed().as_secs();
        if elapsed > 0 && elapsed >= last_print + 5 {
            println!(
                "  Profiling... {}s / {}s",
                elapsed,
                profile_duration.as_secs()
            );
            last_print = elapsed;
        }
        thread::sleep(Duration::from_secs(1));
    }

    let final_cycle_count = poll_counter.load(Ordering::Relaxed);
    println!("\nStep 3: Profiling complete, shutting down threads...");
    println!("  Total poll cycles during profiling: {final_cycle_count}");
    println!(
        "  Expected allocations: {} to {} (200-300 allocs/poll)",
        final_cycle_count * 200,
        final_cycle_count * 300
    );

    // Signal shutdown to threads
    // Ordering::Relaxed is sufficient - this is just a shutdown flag (Guideline Line 96)
    shutdown.store(true, Ordering::Relaxed);

    // Give threads a moment to see shutdown signal
    thread::sleep(Duration::from_millis(500));

    // Note: ProcessMonitor and AppController run in infinite loops
    // We cannot gracefully join them, but DHAT will flush data on process exit
    // The state_consumer thread should exit cleanly
    drop(state_consumer);

    println!("DHAT profiling data collected (dhat-heap.json will be written on process exit)");

    println!("\n=== DHAT Profiling Test Complete ===");
    println!("Next steps:");
    println!("1. Open https://nnethercote.github.io/dh_view/dh_view.html");
    println!("2. Load dhat-heap.json from this test run");
    println!("3. Look for allocation hotspots:");
    println!("   - poll_processes string allocations");
    println!("   - detect_uwp_process overhead");
    println!("   - AppIdentifier creation patterns");
    println!("4. Verify allocation rate is 200-500 allocs/sec");
    println!("5. Check that warmup allocations are excluded from profile\n");
}

/// Create a realistic configuration for profiling (matches `cpu_profiling_test.rs`)
#[cfg(windows)]
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
#[cfg(windows)]
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
