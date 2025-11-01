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
/// Phase 3.1: Returns `RwLock` instead of `Mutex` for concurrent reads
fn create_mock_config(num_monitored: usize) -> Arc<parking_lot::RwLock<AppConfig>> {
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

    Arc::new(parking_lot::RwLock::new(config))
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
            // Phase 3.1: Use read lock for concurrent access
            let guard = config.read();
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
                // Phase 3.1: Use read lock for concurrent access
                let guard = config.read();
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
                // Simulate frequent config reads (Phase 3.1: now uses RwLock)
                let guard = config.read();
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

/// Benchmark simulated `poll_processes()` with varying workloads (Phase 4.1 requirement)
///
/// This benchmark simulates the complete poll cycle to test algorithmic complexity
/// with different process counts and monitored app counts. Tests the O(n) vs O(1)
/// lookup patterns that Phase 1-3 optimizations will address.
///
/// **Test Matrix:** 3 process counts × 4 app counts = 12 combinations
/// - Process counts: 100, 250, 500 (realistic Windows workloads)
/// - Monitored apps: 1, 5, 10, 50
///
/// **What this measures:**
/// - O(n) monitored app lookup (current implementation)
/// - String allocation overhead from process name extraction
/// - `HashSet` operations for process diffing
///
/// **Guideline compliance:**
/// - Line 114: Pre-allocate with `Vec::with_capacity`
/// - Line 120: Use iterator adapters
/// - Line 148: Use `black_box()` to prevent optimizer elision
#[expect(
    clippy::too_many_lines,
    reason = "Benchmark function includes comprehensive test matrix (12 combinations) with setup/teardown code"
)]
fn bench_poll_processes_simulation(c: &mut Criterion) {
    use std::collections::HashSet;

    let mut group = c.benchmark_group("poll_processes_simulation");

    // Test matrix: varying process counts and monitored app counts
    for num_processes in [100, 250, 500] {
        for num_apps in [1, 5, 10, 50] {
            let id = format!("{num_processes}_procs_{num_apps}_apps");

            group.bench_with_input(
                BenchmarkId::from_parameter(id),
                &(num_processes, num_apps),
                |b, &(procs, apps)| {
                    // Setup: Pre-allocate per Guideline Line 114
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "Benchmark procs parameter is limited to [100, 250, 500], well within u32 range"
                    )]
                    let mock_pids: Vec<u32> = (1000..1000 + procs as u32).collect();
                    let monitored_apps = create_mock_config(apps);
                    let mut current_processes = HashSet::with_capacity(procs);
                    let mut previous_processes = HashSet::new();

                    // Simulate realistic process names (Windows 10/11 common processes)
                    let process_names = [
                        "svchost",
                        "explorer",
                        "chrome",
                        "firefox",
                        "msedge",
                        "Code",
                        "Discord",
                        "Teams",
                        "Outlook",
                        "Excel",
                        "Word",
                        "PowerPoint",
                        "notepad",
                        "cmd",
                        "powershell",
                        "WinStore.App",
                        "Calculator",
                        "Photos",
                        "Mail",
                        "Calendar",
                        "GameBar",
                        "Xbox",
                        "Spotify",
                        "Steam",
                        "obs64",
                        "Photoshop",
                        "Premiere Pro",
                        "Blender",
                        "Unity",
                        "UnrealEngine",
                        // Fill remaining with generic names
                        "process_a",
                        "process_b",
                        "process_c",
                        "process_d",
                        "process_e",
                        "process_f",
                        "process_g",
                        "process_h",
                        "process_i",
                        "process_j",
                        "service_a",
                        "service_b",
                        "service_c",
                        "service_d",
                        "service_e",
                        "app_a",
                        "app_b",
                        "app_c",
                        "app_d",
                        "app_e",
                    ];

                    b.iter(|| {
                        current_processes.clear();

                        // Simulate poll cycle (Guideline Line 120: iterator adapters)
                        for &pid in &mock_pids {
                            // Simulate process name extraction (uses modulo for variety)
                            let process_name = process_names[(pid as usize) % process_names.len()];

                            // Simulate O(n) monitored app lookup (current implementation)
                            // This is what Phase 3.2 will optimize to O(1) HashSet lookup
                            // Phase 3.1: Use read lock for concurrent access
                            let guard = monitored_apps.read();
                            let is_monitored = guard.monitored_apps.iter().any(|app| {
                                if let MonitoredApp::Win32(win32_app) = app {
                                    // Simulate string comparison overhead
                                    win32_app.process_name == process_name
                                } else {
                                    false
                                }
                            });
                            drop(guard);

                            if is_monitored {
                                // Simulate inserting into current_processes HashSet
                                black_box(current_processes.insert(pid));
                            }
                        }

                        // Simulate process diffing (new vs previous)
                        let new_processes: Vec<_> = current_processes
                            .difference(&previous_processes)
                            .copied()
                            .collect();
                        let removed_processes: Vec<_> = previous_processes
                            .difference(&current_processes)
                            .copied()
                            .collect();

                        // Use black_box to prevent optimizer from eliding work
                        black_box(&new_processes);
                        black_box(&removed_processes);

                        // Swap sets for next iteration
                        std::mem::swap(&mut current_processes, &mut previous_processes);
                    });
                },
            );
        }
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_monitored_app_lookup,
    bench_watch_list_clone,
    bench_config_read_contention,
    bench_string_allocations,
    bench_poll_processes_simulation
);
criterion_main!(benches);
