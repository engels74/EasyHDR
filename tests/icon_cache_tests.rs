//! Integration tests for icon persistence
//!
//! These tests validate the full icon caching lifecycle including:
//! - Persistence across application restarts
//! - Cache invalidation based on file modification times
//! - Mixed Win32 and UWP app icon caching
//! - Parallel loading performance with many apps
//! - Cache cleanup on application removal
//! - Graceful fallback on cache corruption
//!
//! # Running These Tests
//!
//! **IMPORTANT**: These tests MUST be run with `--test-threads=1` because:
//! - Windows icon extraction API (`ExtractIconEx`) uses global state
//! - UWP `PackageManager` has thread-safety constraints
//! - File modification time checks require sequential execution
//!
//! Run with:
//! ```bash
//! cargo test --test icon_cache_tests -- --test-threads=1
//! ```
//!
//! # Requirements
//!
//! - Requirement 10.5: Integration tests that validate full config save/load cycles with icon persistence

use easyhdr::{
    config::{
        AppConfig,
        models::{MonitoredApp, Win32App},
    },
    utils::IconCache,
};
use std::fs::File;
use std::io::Write;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

/// Test that icons persist across application restarts
///
/// This test validates the full icon persistence lifecycle:
/// 1. Create apps with icons
/// 2. Save configuration to disk
/// 3. Simulate restart by loading configuration
/// 4. Verify icons are restored from cache
///
/// # Requirements
///
/// - Requirement 1.2, 1.3: Icons saved to cache during app creation
/// - Requirement 1.5: Icons restored from cache during config load
/// - Requirement 10.5: Full config save/load cycle integration test
#[test]
fn test_icons_persist_across_restarts() {
    // Create temporary directories for config and cache
    let test_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_dir = test_dir.path().join("icon_cache");
    let config_path = test_dir.path().join("config.json");

    // Create icon cache
    let cache = IconCache::new(&cache_dir).expect("Failed to create cache");

    // Create fake source EXE files (needed for mtime validation)
    let exe_path_1 = test_dir.path().join("app1.exe");
    let exe_path_2 = test_dir.path().join("app2.exe");

    let mut exe_file_1 = File::create(&exe_path_1).expect("Failed to create exe 1");
    exe_file_1
        .write_all(b"fake exe 1")
        .expect("Failed to write exe 1");
    drop(exe_file_1);

    let mut exe_file_2 = File::create(&exe_path_2).expect("Failed to create exe 2");
    exe_file_2
        .write_all(b"fake exe 2")
        .expect("Failed to write exe 2");
    drop(exe_file_2);

    // Wait to ensure time difference
    thread::sleep(Duration::from_millis(50));

    // Create test RGBA icon data
    let test_icon_data_1 = create_test_icon_pattern(1);
    let test_icon_data_2 = create_test_icon_pattern(2);

    // Create test apps with icons
    let app_id_1 = Uuid::new_v4();
    let app_id_2 = Uuid::new_v4();

    let app_1 = Win32App {
        id: app_id_1,
        display_name: "Test App 1".to_string(),
        exe_path: exe_path_1.clone(),
        process_name: "app1".to_string(),
        enabled: true,
        icon_data: Some(test_icon_data_1.clone()),
    };

    let app_2 = Win32App {
        id: app_id_2,
        display_name: "Test App 2".to_string(),
        exe_path: exe_path_2.clone(),
        process_name: "app2".to_string(),
        enabled: true,
        icon_data: Some(test_icon_data_2.clone()),
    };

    // Save icons to cache (simulating what happens during app creation)
    cache
        .save_icon(app_id_1, &test_icon_data_1)
        .expect("Failed to save icon 1");
    cache
        .save_icon(app_id_2, &test_icon_data_2)
        .expect("Failed to save icon 2");

    // Create config with apps (icon_data field is skipped in serialization)
    let mut config = AppConfig::default();
    config.monitored_apps.push(MonitoredApp::Win32(app_1));
    config.monitored_apps.push(MonitoredApp::Win32(app_2));

    // Save config to disk
    let json = serde_json::to_string_pretty(&config).expect("Failed to serialize config");
    std::fs::write(&config_path, json).expect("Failed to write config");

    // Simulate restart: load config from disk
    let json = std::fs::read_to_string(&config_path).expect("Failed to read config");
    let mut loaded_config: AppConfig = serde_json::from_str(&json).expect("Failed to parse config");

    // Manually restore icons from cache (simulating ConfigManager::restore_icons_from_cache)
    for app in &mut loaded_config.monitored_apps {
        match app {
            MonitoredApp::Win32(win32) => {
                if let Ok(Some(icon_data)) = cache.load_icon(win32.id, Some(&win32.exe_path)) {
                    win32.icon_data = Some(icon_data);
                }
            }
            MonitoredApp::Uwp(uwp) => {
                if let Ok(Some(icon_data)) = cache.load_icon(uwp.id, None) {
                    uwp.icon_data = Some(icon_data);
                }
            }
        }
    }

    // Verify icons were restored
    assert_eq!(loaded_config.monitored_apps.len(), 2);

    if let MonitoredApp::Win32(app) = &loaded_config.monitored_apps[0] {
        assert!(
            app.icon_data.is_some(),
            "App 1 icon should be restored from cache"
        );
        assert_eq!(
            app.icon_data.as_ref().unwrap(),
            &test_icon_data_1,
            "App 1 icon data should match original"
        );
    } else {
        panic!("Expected Win32 variant for app 1");
    }

    if let MonitoredApp::Win32(app) = &loaded_config.monitored_apps[1] {
        assert!(
            app.icon_data.is_some(),
            "App 2 icon should be restored from cache"
        );
        assert_eq!(
            app.icon_data.as_ref().unwrap(),
            &test_icon_data_2,
            "App 2 icon data should match original"
        );
    } else {
        panic!("Expected Win32 variant for app 2");
    }

    // Verify cache files exist
    assert!(
        cache_dir.join(format!("{app_id_1}.png")).exists(),
        "Cache file for app 1 should exist"
    );
    assert!(
        cache_dir.join(format!("{app_id_2}.png")).exists(),
        "Cache file for app 2 should exist"
    );
}

/// Test that cache is invalidated when executable file is updated
///
/// This test validates the mtime-based cache validation:
/// 1. Create cache file
/// 2. Update source executable (simulate app update)
/// 3. Verify cache returns miss (stale cache)
/// 4. Verify re-extraction is triggered
///
/// # Requirements
///
/// - Requirement 2.1: Compare cache mtime with executable mtime
/// - Requirement 2.2: Return cache miss if executable is newer
/// - Requirement 2.3: Re-extract icon on cache miss
#[test]
fn test_cache_invalidation_on_exe_update() {
    // Create temporary directories
    let test_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_dir = test_dir.path().join("icon_cache");

    // Create icon cache
    let cache = IconCache::new(&cache_dir).expect("Failed to create cache");

    // Create a fake source EXE file
    let source_path = test_dir.path().join("test.exe");
    let mut source_file = File::create(&source_path).expect("Failed to create source file");
    source_file
        .write_all(b"fake exe v1")
        .expect("Failed to write source file");
    drop(source_file); // Close file to ensure mtime is set

    // Wait to ensure time difference
    thread::sleep(Duration::from_millis(50));

    // Create and save test icon
    let app_id = Uuid::new_v4();
    let test_icon = create_test_icon_pattern(1);
    cache
        .save_icon(app_id, &test_icon)
        .expect("Failed to save icon");

    // Verify cache hit (cache is fresh)
    let result = cache
        .load_icon(app_id, Some(&source_path))
        .expect("Failed to load icon");
    assert!(
        result.is_some(),
        "Cache should hit when source file is older"
    );
    assert_eq!(
        result.as_ref().unwrap(),
        &test_icon,
        "Cached icon should match original"
    );

    // Wait to ensure time difference
    thread::sleep(Duration::from_millis(50));

    // Simulate EXE update by updating modification time
    let mut source_file = File::create(&source_path).expect("Failed to update source file");
    source_file
        .write_all(b"fake exe v2 (updated)")
        .expect("Failed to write updated source file");
    drop(source_file); // Close file to ensure mtime is updated

    // Verify cache miss (cache is stale)
    let result = cache
        .load_icon(app_id, Some(&source_path))
        .expect("Failed to check cache");
    assert!(
        result.is_none(),
        "Cache should miss when source file is newer (stale cache)"
    );

    // Simulate re-extraction: save new icon to cache
    let updated_icon = create_test_icon_pattern(2);
    cache
        .save_icon(app_id, &updated_icon)
        .expect("Failed to save updated icon");

    // Verify new icon is cached
    let result = cache
        .load_icon(app_id, Some(&source_path))
        .expect("Failed to load updated icon");
    assert!(result.is_some(), "Updated icon should be cached");
    assert_eq!(
        result.as_ref().unwrap(),
        &updated_icon,
        "Updated icon should match new data"
    );
}

/// Test mixed Win32 and UWP icon caching
///
/// This test validates that both Win32 and UWP app icons are cached correctly:
/// 1. Create Win32 apps with mtime validation
/// 2. Create UWP apps without mtime validation
/// 3. Save icons to cache
/// 4. Load icons and verify correct validation behavior
///
/// # Requirements
///
/// - Requirement 1.2: Cache Win32 app icons
/// - Requirement 1.3: Cache UWP app icons
/// - Requirement 2.4: Skip validation for UWP apps (no source path)
#[test]
fn test_mixed_win32_and_uwp_icons() {
    // Create temporary directories
    let test_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_dir = test_dir.path().join("icon_cache");

    // Create icon cache
    let cache = IconCache::new(&cache_dir).expect("Failed to create cache");

    // Create Win32 app with source file
    let win32_id = Uuid::new_v4();
    let win32_icon = create_test_icon_pattern(1);
    let win32_source = test_dir.path().join("win32app.exe");
    let mut source_file = File::create(&win32_source).expect("Failed to create Win32 source");
    source_file
        .write_all(b"win32 exe")
        .expect("Failed to write Win32 source");
    drop(source_file);

    // Create UWP app (no source file)
    let uwp_id = Uuid::new_v4();
    let uwp_icon = create_test_icon_pattern(2);

    // Save both icons to cache
    cache
        .save_icon(win32_id, &win32_icon)
        .expect("Failed to save Win32 icon");
    cache
        .save_icon(uwp_id, &uwp_icon)
        .expect("Failed to save UWP icon");

    // Wait to ensure time difference
    thread::sleep(Duration::from_millis(50));

    // Load Win32 icon with source path validation
    let win32_result = cache
        .load_icon(win32_id, Some(&win32_source))
        .expect("Failed to load Win32 icon");
    assert!(
        win32_result.is_some(),
        "Win32 icon should be loaded from cache"
    );
    assert_eq!(
        win32_result.as_ref().unwrap(),
        &win32_icon,
        "Win32 icon should match original"
    );

    // Load UWP icon without source path validation
    let uwp_result = cache
        .load_icon(uwp_id, None)
        .expect("Failed to load UWP icon");
    assert!(uwp_result.is_some(), "UWP icon should be loaded from cache");
    assert_eq!(
        uwp_result.as_ref().unwrap(),
        &uwp_icon,
        "UWP icon should match original"
    );

    // Update Win32 source file (simulate app update)
    thread::sleep(Duration::from_millis(50));
    let mut source_file = File::create(&win32_source).expect("Failed to update Win32 source");
    source_file
        .write_all(b"win32 exe v2")
        .expect("Failed to write updated Win32 source");
    drop(source_file);

    // Verify Win32 cache is invalidated
    let win32_result = cache
        .load_icon(win32_id, Some(&win32_source))
        .expect("Failed to check Win32 cache");
    assert!(
        win32_result.is_none(),
        "Win32 cache should be invalidated when source is newer"
    );

    // Verify UWP cache is still valid (no validation)
    let uwp_result = cache
        .load_icon(uwp_id, None)
        .expect("Failed to load UWP icon");
    assert!(
        uwp_result.is_some(),
        "UWP icon should still be cached (no validation)"
    );
}

/// Test parallel loading performance with 50+ apps
///
/// This test validates parallel icon loading performance:
/// 1. Create 50+ test apps with icons
/// 2. Save icons to cache
/// 3. Load icons in parallel using Rayon
/// 4. Verify all icons are loaded correctly
/// 5. Measure loading time (should be <150ms for 50 apps)
///
/// # Requirements
///
/// - Requirement 3.1: Decode PNG files using parallel loading with Rayon
/// - Requirement 3.3: Load 50 icons in <150ms
/// - Requirement 3.5: Gracefully degrade to sequential on single-core systems
#[test]
#[expect(
    clippy::cast_possible_truncation,
    reason = "Test utility: modulo 256 ensures value fits in u8 range (0-255)"
)]
fn test_parallel_loading_with_many_apps() {
    use rayon::prelude::*;
    use std::time::Instant;

    // Create temporary directories
    let test_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_dir = test_dir.path().join("icon_cache");

    // Create icon cache
    let cache = IconCache::new(&cache_dir).expect("Failed to create cache");

    // Create 50 test apps with icons
    let app_count = 50;
    let mut app_ids = Vec::with_capacity(app_count);
    let mut icon_data = Vec::with_capacity(app_count);

    for i in 0..app_count {
        let app_id = Uuid::new_v4();
        let icon = create_test_icon_pattern(i as u8);

        // Save icon to cache
        cache.save_icon(app_id, &icon).expect("Failed to save icon");

        app_ids.push(app_id);
        icon_data.push(icon);
    }

    // Measure parallel loading time
    let start = Instant::now();

    let loaded_icons: Vec<(Uuid, Vec<u8>)> = app_ids
        .par_iter()
        .filter_map(|app_id| match cache.load_icon(*app_id, None) {
            Ok(Some(icon)) => Some((*app_id, icon)),
            _ => None,
        })
        .collect();

    let duration = start.elapsed();

    // Verify all icons were loaded
    assert_eq!(
        loaded_icons.len(),
        app_count,
        "All icons should be loaded from cache"
    );

    // Verify loading time (Requirement 3.3: <150ms for 50 apps)
    // Note: This is a soft check since performance varies by system
    println!("Loaded {app_count} icons in {duration:?} (target: <150ms)");

    if duration.as_millis() > 150 {
        eprintln!(
            "WARNING: Parallel loading took {}ms, exceeding 150ms target. \
                   This may indicate performance issues or slow test environment.",
            duration.as_millis()
        );
    }

    // Verify icon data matches
    for (app_id, loaded_icon) in &loaded_icons {
        let index = app_ids
            .iter()
            .position(|id| id == app_id)
            .expect("App ID should exist");
        assert_eq!(
            loaded_icon, &icon_data[index],
            "Loaded icon should match original data"
        );
    }
}

/// Test cache cleanup on app removal
///
/// This test validates that cached icons are deleted when apps are removed:
/// 1. Create apps with cached icons
/// 2. Remove apps
/// 3. Verify cached icon files are deleted
///
/// # Requirements
///
/// - Requirement 4.4: Delete corresponding cached icon file on app removal
#[test]
fn test_cache_cleanup_on_app_removal() {
    // Create temporary directories
    let test_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_dir = test_dir.path().join("icon_cache");

    // Create icon cache
    let cache = IconCache::new(&cache_dir).expect("Failed to create cache");

    // Create test apps with icons
    let app_id_1 = Uuid::new_v4();
    let app_id_2 = Uuid::new_v4();
    let app_id_3 = Uuid::new_v4();

    let icon_1 = create_test_icon_pattern(1);
    let icon_2 = create_test_icon_pattern(2);
    let icon_3 = create_test_icon_pattern(3);

    // Save icons to cache
    cache
        .save_icon(app_id_1, &icon_1)
        .expect("Failed to save icon 1");
    cache
        .save_icon(app_id_2, &icon_2)
        .expect("Failed to save icon 2");
    cache
        .save_icon(app_id_3, &icon_3)
        .expect("Failed to save icon 3");

    // Verify all cache files exist
    let cache_file_1 = cache_dir.join(format!("{app_id_1}.png"));
    let cache_file_2 = cache_dir.join(format!("{app_id_2}.png"));
    let cache_file_3 = cache_dir.join(format!("{app_id_3}.png"));

    assert!(cache_file_1.exists(), "Cache file 1 should exist");
    assert!(cache_file_2.exists(), "Cache file 2 should exist");
    assert!(cache_file_3.exists(), "Cache file 3 should exist");

    // Simulate app removal: delete cached icons
    cache
        .remove_icon(app_id_1)
        .expect("Failed to remove icon 1");
    cache
        .remove_icon(app_id_2)
        .expect("Failed to remove icon 2");

    // Verify removed cache files are deleted
    assert!(
        !cache_file_1.exists(),
        "Cache file 1 should be deleted after removal"
    );
    assert!(
        !cache_file_2.exists(),
        "Cache file 2 should be deleted after removal"
    );

    // Verify remaining cache file still exists
    assert!(
        cache_file_3.exists(),
        "Cache file 3 should still exist (not removed)"
    );

    // Verify idempotent removal (removing already-removed icon)
    cache
        .remove_icon(app_id_1)
        .expect("Removing non-existent icon should succeed (idempotent)");
}

/// Test graceful fallback on cache corruption
///
/// This test validates that corrupted cache files are handled gracefully:
/// 1. Create valid cache file
/// 2. Corrupt cache file (write invalid PNG data)
/// 3. Attempt to load icon
/// 4. Verify error is handled gracefully (returns error, doesn't crash)
/// 5. Verify app can continue operation
///
/// # Requirements
///
/// - Requirement 5.3: Re-extract icon on cache load failure
/// - Requirement 5.5: Return cache miss on cache corruption
/// - Requirement 5.6: Preserve error source chains
#[test]
fn test_graceful_fallback_on_cache_corruption() {
    // Create temporary directories
    let test_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_dir = test_dir.path().join("icon_cache");

    // Create icon cache
    let cache = IconCache::new(&cache_dir).expect("Failed to create cache");

    // Create valid cache file
    let app_id = Uuid::new_v4();
    let valid_icon = create_test_icon_pattern(1);
    cache
        .save_icon(app_id, &valid_icon)
        .expect("Failed to save valid icon");

    // Verify cache hit with valid data
    let result = cache.load_icon(app_id, None).expect("Failed to load icon");
    assert!(result.is_some(), "Valid cache should load successfully");

    // Corrupt the cache file by writing invalid PNG data
    let cache_file_path = cache_dir.join(format!("{app_id}.png"));
    let mut corrupted_file =
        File::create(&cache_file_path).expect("Failed to open cache file for corruption");
    corrupted_file
        .write_all(b"This is not a valid PNG file! Corrupted data!")
        .expect("Failed to write corrupted data");
    drop(corrupted_file);

    // Attempt to load corrupted cache
    let result = cache.load_icon(app_id, None);

    // Should return error (not panic) with proper error chain
    assert!(
        result.is_err(),
        "Loading corrupted cache should return error"
    );

    // Verify error type is IconCacheError with proper context
    match result {
        Err(easyhdr::error::EasyHdrError::IconCache(
            easyhdr::error::IconCacheError::PngDecodingError { app_id: err_id, .. },
        )) => {
            assert_eq!(err_id, app_id, "Error should include correct app UUID");
        }
        _ => panic!("Expected PngDecodingError for corrupted cache"),
    }

    // Simulate graceful recovery: re-extract icon and save to cache
    let recovered_icon = create_test_icon_pattern(2);
    cache
        .save_icon(app_id, &recovered_icon)
        .expect("Failed to save recovered icon");

    // Verify cache is working again
    let result = cache
        .load_icon(app_id, None)
        .expect("Failed to load recovered icon");
    assert!(
        result.is_some(),
        "Cache should work after recovering from corruption"
    );
    assert_eq!(
        result.as_ref().unwrap(),
        &recovered_icon,
        "Recovered icon should match new data"
    );
}

/// Helper function to create a test icon with a specific pattern
///
/// Creates a 32x32 RGBA image (4096 bytes) with a repeating pattern
/// based on the input seed. Different seeds produce different patterns
/// for testing cache correctness.
#[expect(
    clippy::cast_possible_truncation,
    reason = "Test utility: wrapping arithmetic on u8 is intentional for pattern generation"
)]
fn create_test_icon_pattern(seed: u8) -> Vec<u8> {
    let mut icon = vec![0u8; 4096]; // 32x32 pixels × 4 channels (RGBA)

    for (i, item) in icon.iter_mut().enumerate().take(4096) {
        // Create a pattern that varies with seed and position
        // This ensures different icons are distinguishable
        *item = ((i as u8).wrapping_mul(seed)).wrapping_add(seed);
    }

    icon
}
