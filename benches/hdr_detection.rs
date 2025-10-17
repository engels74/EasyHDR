//! Benchmarks for HDR state detection
//!
//! Note: These benchmarks are Windows-only and will be skipped on other platforms.

#![allow(missing_docs)]

use criterion::{criterion_group, criterion_main};

#[cfg(windows)]
mod windows_benches {
    use criterion::{Criterion, black_box};
    use easyhdr::hdr::HdrController;

    pub fn bench_hdr_controller_creation(c: &mut Criterion) {
        c.bench_function("hdr_controller_new", |b| {
            b.iter(|| {
                let controller = HdrController::new();
                black_box(controller);
            });
        });
    }

    pub fn bench_hdr_state_detection(c: &mut Criterion) {
        // Only run if we can create an HDR controller
        if let Ok(controller) = HdrController::new() {
            let displays = controller.get_display_cache();

            if let Some(display) = displays.first() {
                c.bench_function("hdr_is_enabled_check", |b| {
                    b.iter(|| {
                        let enabled = controller.is_hdr_enabled(black_box(display));
                        black_box(enabled);
                    });
                });
            }

            c.bench_function("hdr_get_display_cache", |b| {
                b.iter(|| {
                    let displays = controller.get_display_cache();
                    black_box(displays);
                });
            });
        }
    }
}

#[cfg(not(windows))]
mod windows_benches {
    use criterion::Criterion;

    pub fn bench_hdr_controller_creation(_c: &mut Criterion) {
        // Stub for non-Windows platforms
    }

    pub fn bench_hdr_state_detection(_c: &mut Criterion) {
        // Stub for non-Windows platforms
    }
}

criterion_group!(
    benches,
    windows_benches::bench_hdr_controller_creation,
    windows_benches::bench_hdr_state_detection
);
criterion_main!(benches);
