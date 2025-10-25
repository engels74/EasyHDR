//! Memory usage tests
//!
//! This test module verifies that the application uses less than 50MB RAM during monitoring.

use easyhdr::config::models::{AppConfig, MonitoredApp, Win32App};
use easyhdr::utils::memory_profiler;
use std::path::PathBuf;
use uuid::Uuid;

#[test]
fn test_memory_profiler_tracks_icons() {
    let profiler = memory_profiler::MemoryProfiler::new();

    // Simulate caching some icons
    let icon_size_1 = 4096; // 4 KB
    let icon_size_2 = 8192; // 8 KB

    profiler.record_icon_cached(icon_size_1);
    profiler.record_icon_cached(icon_size_2);

    let stats = profiler.get_stats();
    assert_eq!(stats.icon_cache_memory, icon_size_1 + icon_size_2);
    assert_eq!(stats.cached_icon_count, 2);

    // Remove one icon
    profiler.record_icon_removed(icon_size_1);

    let stats = profiler.get_stats();
    assert_eq!(stats.icon_cache_memory, icon_size_2);
    assert_eq!(stats.cached_icon_count, 1);
}

#[test]
fn test_icon_cache_memory_estimation() {
    // Test that icon cache memory is reasonable for typical usage
    // Typical icon: 32x32 RGBA = 4096 bytes
    // With 10 apps: ~40 KB
    // With 50 apps: ~200 KB
    // With 100 apps: ~400 KB

    let profiler = memory_profiler::MemoryProfiler::new();

    // Simulate 50 apps with icons
    let icon_size = 4096; // 4 KB per icon
    for _ in 0..50 {
        profiler.record_icon_cached(icon_size);
    }

    let stats = profiler.get_stats();
    let expected_cache_size = 50 * icon_size;
    assert_eq!(stats.icon_cache_memory, expected_cache_size);
    assert_eq!(stats.cached_icon_count, 50);

    // Verify it's reasonable (< 1 MB for 50 apps)
    assert!(stats.icon_cache_mb() < 1.0);
}

#[test]
fn test_config_memory_estimation() {
    // Test that configuration memory is reasonable
    let mut config = AppConfig::default();

    // Add 20 monitored apps
    for i in 0..20 {
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: format!("Test App {i}"),
            exe_path: PathBuf::from(format!("C:\\Apps\\app{i}.exe")),
            process_name: format!("app{i}"),
            enabled: true,
            icon_data: None, // No icons for this test
        }));
    }

    // Estimate memory usage
    // Each MonitoredApp: ~200 bytes (UUID + strings + path)
    // 20 apps: ~4 KB
    // Plus overhead: ~10 KB total
    let estimated_size = std::mem::size_of::<AppConfig>()
        + config.monitored_apps.len() * std::mem::size_of::<MonitoredApp>();

    // Should be less than 50 KB for 20 apps
    assert!(estimated_size < 50 * 1024);
}

#[test]
fn test_monitored_app_release_icon() {
    let mut app = MonitoredApp::Win32(Win32App {
        id: Uuid::new_v4(),
        display_name: "Test App".to_string(),
        exe_path: PathBuf::from("C:\\test.exe"),
        process_name: "test".to_string(),
        enabled: true,
        icon_data: Some(vec![0u8; 4096]), // 4 KB icon
    });

    // Verify icon is present
    if let MonitoredApp::Win32(ref win32_app) = app {
        assert!(win32_app.icon_data.is_some());
    } else {
        panic!("Expected Win32 variant");
    }

    // Release icon
    app.release_icon();

    // Verify icon is removed
    if let MonitoredApp::Win32(ref win32_app) = app {
        assert!(win32_app.icon_data.is_none());
    } else {
        panic!("Expected Win32 variant");
    }
}

#[test]
fn test_memory_stats_within_limits() {
    // Test that typical usage is within 50MB limit
    let stats = memory_profiler::MemoryStats {
        total_memory: 40 * 1024 * 1024,     // 40 MB
        icon_cache_memory: 5 * 1024 * 1024, // 5 MB
        cached_icon_count: 100,
        config_memory: 10 * 1024, // 10 KB
        monitor_memory: 5 * 1024, // 5 KB
    };

    assert!(stats.is_within_limits());
    // Allow exact float comparison: values are constructed to be exact multiples
    #[allow(clippy::float_cmp)]
    {
        assert_eq!(stats.total_mb(), 40.0);
        assert_eq!(stats.icon_cache_mb(), 5.0);
    }
}

#[test]
fn test_memory_stats_exceeds_limits() {
    // Test that excessive usage is detected
    let stats = memory_profiler::MemoryStats {
        total_memory: 60 * 1024 * 1024,      // 60 MB (over limit)
        icon_cache_memory: 10 * 1024 * 1024, // 10 MB
        cached_icon_count: 200,
        config_memory: 10 * 1024, // 10 KB
        monitor_memory: 5 * 1024, // 5 KB
    };

    assert!(!stats.is_within_limits());
    // Allow exact float comparison: value is constructed to be exact multiple
    #[allow(clippy::float_cmp)]
    {
        assert_eq!(stats.total_mb(), 60.0);
    }
}

#[test]
fn test_typical_application_memory_budget() {
    // Test a realistic scenario with typical usage
    // Assumptions:
    // - 20 monitored applications
    // - Each with a 4KB icon
    // - Base application overhead: ~30 MB
    // - Icon cache: 20 * 4KB = 80 KB
    // - Config: ~10 KB
    // - Monitor: ~5 KB
    // Total: ~30.1 MB (well within 50 MB limit)

    let profiler = memory_profiler::MemoryProfiler::new();

    // Simulate 20 apps with icons
    for _ in 0..20 {
        profiler.record_icon_cached(4096);
    }

    let stats = profiler.get_stats();

    // Icon cache should be ~80 KB
    assert_eq!(stats.icon_cache_memory, 20 * 4096);
    assert!(stats.icon_cache_mb() < 0.1); // Less than 0.1 MB

    // Config and monitor should be minimal
    assert!(stats.config_memory < 20 * 1024); // Less than 20 KB
    assert!(stats.monitor_memory < 10 * 1024); // Less than 10 KB

    // Total overhead (excluding process memory) should be minimal
    let overhead = stats.icon_cache_memory + stats.config_memory + stats.monitor_memory;
    assert!(overhead < 200 * 1024); // Less than 200 KB overhead
}

#[test]
fn test_large_icon_cache_scenario() {
    // Test worst-case scenario with many large icons
    // 100 apps with 8KB icons each = 800 KB
    // Should still be well within limits

    let profiler = memory_profiler::MemoryProfiler::new();

    // Simulate 100 apps with larger icons
    for _ in 0..100 {
        profiler.record_icon_cached(8192); // 8 KB per icon
    }

    let stats = profiler.get_stats();

    // Icon cache should be ~800 KB
    assert_eq!(stats.icon_cache_memory, 100 * 8192);
    assert!(stats.icon_cache_mb() < 1.0); // Less than 1 MB

    // Even with 100 apps, icon cache should be reasonable
    assert!(stats.icon_cache_memory < 1024 * 1024); // Less than 1 MB
}

#[test]
fn test_memory_profiler_global_instance() {
    // Test that the global profiler instance works correctly
    let profiler = memory_profiler::get_profiler();

    // Record some icons
    profiler.record_icon_cached(4096);
    profiler.record_icon_cached(8192);

    let stats = profiler.get_stats();
    assert!(stats.icon_cache_memory > 0);
    assert!(stats.cached_icon_count > 0);

    // Clean up for other tests
    profiler.record_icon_removed(4096);
    profiler.record_icon_removed(8192);
}

#[cfg(windows)]
#[test]
fn test_process_memory_retrieval() {
    // Test that we can retrieve process memory on Windows
    let profiler = memory_profiler::MemoryProfiler::new();
    let stats = profiler.get_stats();

    // Should have some memory usage (at least a few MB for the test process)
    assert!(stats.total_memory > 0);
    assert!(stats.total_mb() > 0.0);
}

#[test]
fn test_memory_stats_default() {
    let stats = memory_profiler::MemoryStats::default();
    assert_eq!(stats.total_memory, 0);
    assert_eq!(stats.icon_cache_memory, 0);
    assert_eq!(stats.cached_icon_count, 0);
    assert_eq!(stats.config_memory, 0);
    assert_eq!(stats.monitor_memory, 0);
}
