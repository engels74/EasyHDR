//! Benchmarks for UWP detection and package family name extraction
//!
//! These benchmarks validate performance requirements:
//! - Requirement 2.6: UWP detection overhead <2ms per polling cycle
//! - Requirement 7.3: UWP detection for 150-250 processes within 1.25ms

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};

#[cfg(windows)]
use criterion::BenchmarkId;

#[cfg(windows)]
use std::hint::black_box;

#[cfg(windows)]
use easyhdr::uwp::extract_package_family_name;

#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
#[cfg(windows)]
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// Benchmark package family name extraction from full name
#[cfg(windows)]
fn bench_extract_package_family_name(c: &mut Criterion) {
    let test_cases = vec![
        "Microsoft.WindowsCalculator_10.2103.8.0_x64__8wekyb3d8bbwe",
        "Microsoft.WindowsStore_12011.1001.1.0_x64__8wekyb3d8bbwe",
        "Microsoft.Photos_2023.11110.8002.0_arm64__8wekyb3d8bbwe",
        "Microsoft.DesktopAppInstaller_1.21.3133.0_neutral__8wekyb3d8bbwe",
        "Microsoft.MicrosoftEdge_44.19041.1266.0_neutral__8wekyb3d8bbwe",
        "Microsoft.Xbox.TCUI_1.24.10001.0_x64__8wekyb3d8bbwe",
        "Microsoft.WindowsTerminal_1.18.3181.0_x64__8wekyb3d8bbwe",
    ];

    c.bench_function("extract_package_family_name", |b| {
        b.iter(|| {
            for full_name in &test_cases {
                let _ = black_box(extract_package_family_name(black_box(full_name)));
            }
        });
    });
}

/// Benchmark package family name extraction with a single typical case
#[cfg(windows)]
fn bench_extract_package_family_name_single(c: &mut Criterion) {
    let full_name = "Microsoft.WindowsCalculator_10.2103.8.0_x64__8wekyb3d8bbwe";

    c.bench_function("extract_package_family_name_single", |b| {
        b.iter(|| {
            let _ = black_box(extract_package_family_name(black_box(full_name)));
        });
    });
}

/// Benchmark package family name extraction with varying complexity
#[cfg(windows)]
fn bench_extract_package_family_name_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("extract_package_family_name_complexity");

    // Simple case (5 parts)
    let simple = "Microsoft.WindowsCalculator_10.2103.8.0_x64__8wekyb3d8bbwe";
    group.bench_function("simple_5_parts", |b| {
        b.iter(|| {
            let _ = black_box(extract_package_family_name(black_box(simple)));
        });
    });

    // Complex case (more parts due to extra underscores)
    let complex = "Microsoft.WindowsCalculator_10.2103.8.0_x64_extra_part__8wekyb3d8bbwe";
    group.bench_function("complex_7_parts", |b| {
        b.iter(|| {
            let _ = black_box(extract_package_family_name(black_box(complex)));
        });
    });

    group.finish();
}

/// Benchmark UWP detection overhead on real processes
///
/// This benchmark measures the overhead of calling `detect_uwp_process` on actual
/// running processes. It validates Requirement 2.6 and 7.3.
#[cfg(windows)]
#[allow(unsafe_code)]
fn bench_uwp_detection_on_real_processes(c: &mut Criterion) {
    use easyhdr::uwp::detect_uwp_process;

    // Take a snapshot of running processes
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_err() {
        eprintln!("Warning: Could not create process snapshot for benchmark");
        return;
    }
    let snapshot = snapshot.unwrap();

    // Collect a sample of process IDs (up to 50 for faster benchmarking)
    let mut process_ids = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>()
            .try_into()
            .expect("PROCESSENTRY32W size fits in u32"),
        ..Default::default()
    };

    if unsafe { Process32FirstW(snapshot, &raw mut entry) }.is_ok() {
        loop {
            process_ids.push(entry.th32ProcessID);
            if process_ids.len() >= 50 {
                break;
            }
            if unsafe { Process32NextW(snapshot, &raw mut entry) }.is_err() {
                break;
            }
        }
    }

    if process_ids.is_empty() {
        eprintln!("Warning: No processes found for benchmark");
        return;
    }

    c.bench_function("uwp_detection_real_processes", |b| {
        b.iter(|| {
            let mut uwp_count = 0;
            let mut win32_count = 0;

            for &pid in black_box(&process_ids) {
                // Try to open the process handle
                let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };

                if let Ok(handle) = handle {
                    // Detect if it's a UWP process
                    match unsafe { detect_uwp_process(handle) } {
                        Ok(Some(_)) => uwp_count += 1,
                        Ok(None) => win32_count += 1,
                        Err(_) => {}
                    }
                }
            }

            black_box((uwp_count, win32_count))
        });
    });
}

/// Benchmark polling cycle overhead: Win32-only vs Win32+UWP
///
/// This benchmark compares the performance of Win32-only process detection
/// against Win32+UWP detection to validate Requirement 2.6 (<2ms overhead).
#[cfg(windows)]
#[allow(unsafe_code)]
fn bench_polling_cycle_comparison(c: &mut Criterion) {
    use easyhdr::uwp::detect_uwp_process;

    let mut group = c.benchmark_group("polling_cycle_overhead");

    // Collect process PIDs for benchmarking
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_err() {
        eprintln!("Warning: Could not create process snapshot for benchmark");
        return;
    }
    let snapshot = snapshot.unwrap();

    let mut process_ids = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>()
            .try_into()
            .expect("PROCESSENTRY32W size fits in u32"),
        ..Default::default()
    };

    if unsafe { Process32FirstW(snapshot, &raw mut entry) }.is_ok() {
        loop {
            process_ids.push(entry.th32ProcessID);
            if process_ids.len() >= 200 {
                // Simulate typical system with 200 processes
                break;
            }
            if unsafe { Process32NextW(snapshot, &raw mut entry) }.is_err() {
                break;
            }
        }
    }

    if process_ids.is_empty() {
        eprintln!("Warning: No processes found for benchmark");
        return;
    }

    // Benchmark Win32-only detection (just opening handles)
    group.bench_function("win32_only", |b| {
        b.iter(|| {
            let mut count = 0;
            for &pid in black_box(&process_ids) {
                if let Ok(_handle) =
                    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                {
                    count += 1;
                    // Handle is automatically closed when it goes out of scope
                }
            }
            black_box(count)
        });
    });

    // Benchmark Win32+UWP detection (with GetPackageFullName)
    group.bench_function("win32_plus_uwp", |b| {
        b.iter(|| {
            let mut uwp_count = 0;
            let mut win32_count = 0;

            for &pid in black_box(&process_ids) {
                if let Ok(handle) =
                    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                {
                    match unsafe { detect_uwp_process(handle) } {
                        Ok(Some(_)) => uwp_count += 1,
                        Ok(None) => win32_count += 1,
                        Err(_) => {}
                    }
                }
            }

            black_box((uwp_count, win32_count))
        });
    });

    group.finish();
}

/// Benchmark UWP detection at varying process counts
///
/// This benchmark measures detection performance at different scales to validate
/// Requirement 7.3 (150-250 processes within 1.25ms).
#[cfg(windows)]
#[allow(unsafe_code)]
fn bench_uwp_detection_scaling(c: &mut Criterion) {
    use easyhdr::uwp::detect_uwp_process;

    // Collect all available process PIDs
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_err() {
        eprintln!("Warning: Could not create process snapshot for benchmark");
        return;
    }
    let snapshot = snapshot.unwrap();

    let mut all_process_ids = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>()
            .try_into()
            .expect("PROCESSENTRY32W size fits in u32"),
        ..Default::default()
    };

    if unsafe { Process32FirstW(snapshot, &raw mut entry) }.is_ok() {
        loop {
            all_process_ids.push(entry.th32ProcessID);
            if unsafe { Process32NextW(snapshot, &raw mut entry) }.is_err() {
                break;
            }
        }
    }

    if all_process_ids.is_empty() {
        eprintln!("Warning: No processes found for benchmark");
        return;
    }

    let mut group = c.benchmark_group("uwp_detection_scaling");

    // Test at different scales
    for count in &[50, 100, 150, 200, 250] {
        let process_sample: Vec<_> = all_process_ids.iter().take(*count).copied().collect();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{count}_processes")),
            &process_sample,
            |b, pids| {
                b.iter(|| {
                    let mut uwp_count = 0;
                    let mut win32_count = 0;

                    for &pid in black_box(pids) {
                        if let Ok(handle) =
                            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                        {
                            match unsafe { detect_uwp_process(handle) } {
                                Ok(Some(_)) => uwp_count += 1,
                                Ok(None) => win32_count += 1,
                                Err(_) => {}
                            }
                        }
                    }

                    black_box((uwp_count, win32_count))
                });
            },
        );
    }

    group.finish();
}

#[cfg(windows)]
criterion_group!(
    benches,
    bench_extract_package_family_name,
    bench_extract_package_family_name_single,
    bench_extract_package_family_name_complexity,
    bench_uwp_detection_on_real_processes,
    bench_polling_cycle_comparison,
    bench_uwp_detection_scaling
);

#[cfg(not(windows))]
fn bench_noop(_c: &mut Criterion) {
    // No-op benchmark for non-Windows platforms
}

#[cfg(not(windows))]
criterion_group!(benches, bench_noop);

criterion_main!(benches);
