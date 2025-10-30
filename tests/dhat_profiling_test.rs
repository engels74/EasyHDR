//! DHAT allocation profiling test for Phase 0 baseline measurements
//!
//! This test instruments the application with DHAT to measure allocation patterns
//! in config operations, watch list cloning, and string allocations.
//!
//! Note: Direct ProcessMonitor profiling requires running the full application
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
//! # Phase 0 Success Criteria
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

    for i in 0..num_apps.min(apps.len()) {
        let (process_name, display_name, path) = apps[i];
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

/// Profile watch list cloning overhead (Phase 2.1 optimization target)
///
/// This test measures allocation overhead from repeated watch list cloning,
/// which currently occurs on every config access.
#[test]
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

/// Profile monitored app lookups with varying sizes (Phase 3.2 optimization target)
///
/// This test measures allocation and performance overhead from O(n) lookups
/// in the monitored apps list. Target is O(1) HashSet lookup.
#[test]
#[ignore] // Run explicitly with: cargo test --test dhat_profiling_test --ignored -- profile_monitored_app_lookups
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

/// Profile config access patterns (Phase 3.1 optimization target)
///
/// This test measures allocation overhead from config reads and writes.
#[test]
#[ignore] // Run explicitly with: cargo test --test dhat_profiling_test --ignored -- profile_config_access
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
