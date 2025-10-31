//! Benchmarks for process monitoring hot paths
//!
//! This benchmark suite focuses on Phase 0 baseline profiling to identify:
//! - Config access patterns (O(n) lookups - Phase 3.2 optimization target)
//! - Watch list cloning overhead (Phase 2.1 optimization target)
//! - String allocation patterns (Phase 1.2 optimization target)
//!
//! Note: Direct process polling benchmarks require Windows APIs and are better
//! measured via integration tests and profiling tools (cargo-flamegraph, DHAT).
//! See `docs/performance_plan.md` for profiling instructions.

#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use easyhdr::config::models::Win32App;
use easyhdr::config::{AppConfig, MonitoredApp, UserPreferences, WindowState};
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

/// Create a mock configuration with varying numbers of monitored apps
fn create_mock_config(num_monitored: usize) -> Arc<parking_lot::Mutex<AppConfig>> {
    let mut config = AppConfig {
        monitored_apps: Vec::with_capacity(num_monitored),
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

    // Add monitored apps with realistic process names
    let realistic_processes = vec![
        ("chrome.exe", "Google Chrome"),
        ("firefox.exe", "Mozilla Firefox"),
        ("msedge.exe", "Microsoft Edge"),
        ("Code.exe", "Visual Studio Code"),
        ("Photoshop.exe", "Adobe Photoshop"),
        ("Premiere Pro.exe", "Adobe Premiere Pro"),
        ("Blender.exe", "Blender"),
        ("obs64.exe", "OBS Studio"),
        ("steam.exe", "Steam"),
        ("Discord.exe", "Discord"),
    ];

    for i in 0..num_monitored {
        let (process_name, display_name) = realistic_processes[i % realistic_processes.len()];
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: display_name.to_string(),
            exe_path: PathBuf::from(format!("C:\\Program Files\\{display_name}\\{process_name}")),
            process_name: process_name.to_string(),
            enabled: true,
            icon_data: None,
        }));
    }

    Arc::new(parking_lot::Mutex::new(config))
}

/// Benchmark monitored app lookups (O(n) currently, Phase 3.2 target: O(1))
///
/// This simulates the event handling pattern where we check if a process
/// is in the monitored list. Currently uses O(n) iteration, target is O(1) `HashSet`.
fn bench_monitored_app_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("monitored_app_lookup");

    // Test with different numbers of monitored apps
    for num_apps in [1, 5, 10, 50] {
        let config = create_mock_config(num_apps);

        group.bench_with_input(BenchmarkId::new("apps", num_apps), &num_apps, |b, _| {
            let guard = config.lock();
            let apps = &guard.monitored_apps;

            b.iter(|| {
                // Simulate checking if a process is monitored (O(n) currently)
                let target_processes = ["chrome.exe", "firefox.exe", "obs64.exe"];

                for target in &target_processes {
                    black_box(apps.iter().any(|app| {
                        if let MonitoredApp::Win32(win32_app) = app {
                            win32_app.process_name == *target
                        } else {
                            false
                        }
                    }));
                }
            });
        });
    }

    group.finish();
}

/// Benchmark watch list operations
///
/// Tests the overhead of cloning the watch list (Phase 2.1 optimization target)
fn bench_watch_list_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("watch_list_clone");

    for num_apps in [1, 5, 10, 50] {
        let config = create_mock_config(num_apps);

        group.bench_with_input(BenchmarkId::new("apps", num_apps), &num_apps, |b, _| {
            b.iter(|| {
                let guard = config.lock();
                let apps = black_box(&guard.monitored_apps);
                // Simulate current implementation: clone the entire Vec
                black_box(apps.clone());
            });
        });
    }

    group.finish();
}

/// Benchmark config access patterns
///
/// Tests Mutex contention (Phase 3.1 `RwLock` optimization target)
fn bench_config_read_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_read");

    for num_apps in [1, 10, 50] {
        let config = create_mock_config(num_apps);

        group.bench_with_input(BenchmarkId::new("apps", num_apps), &num_apps, |b, _| {
            b.iter(|| {
                // Simulate frequent config reads (currently uses Mutex)
                let guard = config.lock();
                let apps = &guard.monitored_apps;
                // Simulate checking if an app is monitored (O(n) currently)
                let target = "chrome.exe";
                black_box(apps.iter().any(|app| {
                    if let MonitoredApp::Win32(win32_app) = app {
                        win32_app.process_name == target
                    } else {
                        false
                    }
                }));
            });
        });
    }

    group.finish();
}

/// Benchmark string allocation patterns
///
/// Tests allocation overhead from process name extraction (Phase 1.2 optimization target)
fn bench_string_allocations(c: &mut Criterion) {
    let mut group = c.benchmark_group("string_allocations");

    // Simulate extracting process names from paths (currently allocates per poll)
    let paths = vec![
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Windows\\System32\\svchost.exe",
        "C:\\Program Files\\Mozilla Firefox\\firefox.exe",
        "C:\\Users\\User\\AppData\\Local\\Discord\\app-1.0.9015\\Discord.exe",
    ];

    group.bench_function("process_name_extraction", |b| {
        b.iter(|| {
            for path in &paths {
                // Simulate extracting filename (allocates String)
                let filename = PathBuf::from(path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(std::string::ToString::to_string)
                    .unwrap_or_default();
                black_box(filename);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_monitored_app_lookup,
    bench_watch_list_clone,
    bench_config_read_contention,
    bench_string_allocations
);
criterion_main!(benches);
