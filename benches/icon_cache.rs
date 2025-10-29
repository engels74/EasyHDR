//! Benchmarks for icon cache parallel loading performance
//!
//! This benchmark measures the performance of parallel icon loading using Rayon.
//! It validates that loading 10, 50, and 100 cached icons meets the performance
//! targets:
//!
//! - 10 apps: <50ms
//! - 50 apps: <150ms
//! - 100 apps: <250ms

#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use easyhdr::utils::icon_cache::IconCache;
use rayon::prelude::*;
use std::hint::black_box;
use tempfile::TempDir;
use uuid::Uuid;

/// Create a temporary icon cache pre-populated with test icons
///
/// This helper function creates a temporary cache directory and populates it
/// with the specified number of test icons. Each icon is 32x32 RGBA (4096 bytes)
/// with a unique pattern to prevent unrealistic compression.
///
/// # Arguments
///
/// * `icon_count` - Number of icons to pre-populate in the cache
///
/// # Returns
///
/// Returns a tuple of (`TempDir`, `IconCache`, `Vec<Uuid>`) where:
/// - `TempDir`: Must be kept alive to prevent directory deletion
/// - `IconCache`: The cache instance to use for benchmarks
/// - `Vec<Uuid>`: List of app IDs corresponding to cached icons
#[expect(
    clippy::cast_possible_truncation,
    reason = "Benchmark utility: modulo 256 ensures value fits in u8 range (0-255)"
)]
fn create_populated_cache(icon_count: usize) -> (TempDir, IconCache, Vec<Uuid>) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");

    // Pre-populate cache with test icons
    let app_ids: Vec<Uuid> = (0..icon_count).map(|_| Uuid::new_v4()).collect();

    for (i, app_id) in app_ids.iter().enumerate() {
        // Create unique RGBA data for each icon to prevent unrealistic compression
        // Use a pattern based on index to ensure variety
        let mut rgba_data = vec![0u8; 4096];
        for (pixel_idx, byte) in rgba_data.iter_mut().enumerate() {
            *byte = ((pixel_idx + i * 7) % 256) as u8;
        }

        cache
            .save_icon(*app_id, &rgba_data)
            .expect("Failed to save test icon");
    }

    (temp_dir, cache, app_ids)
}

/// Benchmark parallel icon loading for various app counts
///
/// This benchmark measures the time to load cached icons in parallel using Rayon.
/// It pre-populates the cache with test icons and then measures the time to load
/// all icons concurrently.
///
/// # Design
///
/// The benchmark uses Rayon's parallel iterator to decode multiple PNG files
/// concurrently. This simulates the actual startup behavior where `ConfigManager`
/// loads all cached icons in parallel using `restore_icons_from_cache()`.
///
/// # Performance Targets
///
/// - 10 apps: <50ms (3-4x faster than sequential ~100ms)
/// - 50 apps: <150ms (3-4x faster than sequential ~500ms)
/// - 100 apps: <250ms (3-4x faster than sequential ~1000ms)
fn bench_parallel_icon_loading(c: &mut Criterion) {
    let mut group = c.benchmark_group("icon_cache_parallel_load");

    // Benchmark for 10, 50, and 100 apps
    for &app_count in &[10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("parallel_load", app_count),
            &app_count,
            |b, &count| {
                // Setup: Create temp cache with test icons
                // This happens outside the benchmark timing
                let (_temp_dir, cache, app_ids) = create_populated_cache(count);

                // Benchmark: Parallel loading with Rayon
                b.iter(|| {
                    // Use par_iter() to load icons in parallel (same as ConfigManager)
                    let loaded_icons: Vec<_> = app_ids
                        .par_iter()
                        .filter_map(|app_id| {
                            // Load icon without source path validation (UWP app behavior)
                            // Use black_box to prevent compiler from optimizing away the load
                            let result = cache.load_icon(black_box(*app_id), None);
                            match result {
                                Ok(Some(icon_data)) => {
                                    // Use black_box to ensure the data is actually loaded
                                    Some(black_box(icon_data))
                                }
                                _ => None,
                            }
                        })
                        .collect();

                    // Use black_box on the result to prevent optimization
                    black_box(loaded_icons);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark sequential icon loading as baseline comparison
///
/// This benchmark measures sequential icon loading (without Rayon parallelism)
/// to demonstrate the speedup achieved by parallel loading. This is NOT the
/// actual implementation, but serves as a baseline for comparison.
///
/// # Expected Results
///
/// Sequential loading should be 3-4x slower than parallel loading:
/// - 10 apps: ~100ms (vs ~30ms parallel)
/// - 50 apps: ~500ms (vs ~150ms parallel)
/// - 100 apps: ~1000ms (vs ~250ms parallel)
fn bench_sequential_icon_loading(c: &mut Criterion) {
    let mut group = c.benchmark_group("icon_cache_sequential_load");

    for &app_count in &[10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("sequential_load", app_count),
            &app_count,
            |b, &count| {
                let (_temp_dir, cache, app_ids) = create_populated_cache(count);

                b.iter(|| {
                    // Use regular iterator (no par_iter) for sequential loading
                    let loaded_icons: Vec<_> = app_ids
                        .iter()
                        .filter_map(|app_id| {
                            let result = cache.load_icon(black_box(*app_id), None);
                            match result {
                                Ok(Some(icon_data)) => Some(black_box(icon_data)),
                                _ => None,
                            }
                        })
                        .collect();

                    black_box(loaded_icons);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark single icon load operation
///
/// This benchmark measures the time to load a single icon from cache,
/// providing a baseline for understanding the per-icon overhead.
fn bench_single_icon_load(c: &mut Criterion) {
    c.bench_function("icon_cache_single_load", |b| {
        // Setup: Create cache with one test icon
        let (_temp_dir, cache, app_ids) = create_populated_cache(1);
        let app_id = app_ids[0];

        b.iter(|| {
            let icon_data = cache
                .load_icon(black_box(app_id), None)
                .expect("Failed to load icon")
                .expect("Icon should exist");

            black_box(icon_data);
        });
    });
}

/// Benchmark icon save operation
///
/// This benchmark measures the time to save a single icon to cache,
/// including PNG encoding and atomic file write.
#[expect(
    clippy::cast_possible_truncation,
    reason = "Benchmark utility: modulo 256 ensures value fits in u8 range (0-255)"
)]
fn bench_icon_save(c: &mut Criterion) {
    c.bench_function("icon_cache_save", |b| {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");

        // Create test RGBA data with variety to prevent unrealistic compression
        let mut rgba_data = vec![0u8; 4096];
        for (i, byte) in rgba_data.iter_mut().enumerate() {
            *byte = ((i * 13 + 7) % 256) as u8;
        }

        b.iter(|| {
            let app_id = Uuid::new_v4();
            cache
                .save_icon(black_box(app_id), black_box(&rgba_data))
                .expect("Failed to save icon");
        });
    });
}

/// Benchmark cache statistics calculation
///
/// This benchmark measures the time to calculate cache statistics
/// (icon count and total size) for various cache sizes.
fn bench_cache_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("icon_cache_stats");

    for &app_count in &[10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("get_stats", app_count),
            &app_count,
            |b, &count| {
                let (_temp_dir, cache, _app_ids) = create_populated_cache(count);

                b.iter(|| {
                    let stats = cache.get_cache_stats().expect("Failed to get stats");
                    black_box(stats);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parallel_icon_loading,
    bench_sequential_icon_loading,
    bench_single_icon_load,
    bench_icon_save,
    bench_cache_stats
);
criterion_main!(benches);
