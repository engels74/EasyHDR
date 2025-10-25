//! Benchmarks for UWP detection and package family name extraction

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};

#[cfg(windows)]
use std::hint::black_box;

#[cfg(windows)]
use easyhdr::uwp::extract_package_family_name;

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

#[cfg(windows)]
criterion_group!(
    benches,
    bench_extract_package_family_name,
    bench_extract_package_family_name_single,
    bench_extract_package_family_name_complexity
);

#[cfg(not(windows))]
fn bench_noop(_c: &mut Criterion) {
    // No-op benchmark for non-Windows platforms
}

#[cfg(not(windows))]
criterion_group!(benches, bench_noop);

criterion_main!(benches);

