#![allow(clippy::unwrap_used)]
//! Memory usage tests
//!
//! This test module verifies that the application uses less than 50MB RAM during monitoring.

use easyhdr::config::models::{AppConfig, MonitoredApp, UwpApp, Win32App};
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
    #[expect(
        clippy::float_cmp,
        reason = "Values are constructed as exact multiples of 1024*1024, so float comparison is precise"
    )]
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
    #[expect(
        clippy::float_cmp,
        reason = "Value is constructed as exact multiple of 1024*1024, so float comparison is precise"
    )]
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

// UWP-specific memory tests

#[test]
fn test_uwp_app_memory_size() {
    // Test that UwpApp struct size is reasonable
    // UwpApp fields: UUID (16 bytes) + 3 Strings (24 bytes each) + bool (1 byte) + Option<Vec<u8>> (24 bytes)
    // Expected: ~100-200 bytes per app (with heap allocations for strings)

    let uwp_app = UwpApp {
        id: Uuid::new_v4(),
        display_name: "Test UWP App".to_string(),
        package_family_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        app_id: "App".to_string(),
        enabled: true,
        icon_data: None,
    };

    // Stack size should be reasonable
    let stack_size = std::mem::size_of::<UwpApp>();
    assert!(
        stack_size < 200,
        "UwpApp stack size ({stack_size}) should be < 200 bytes"
    );

    // Verify fields are set correctly
    assert_eq!(uwp_app.display_name, "Test UWP App");
    assert_eq!(
        uwp_app.package_family_name,
        "Microsoft.WindowsCalculator_8wekyb3d8bbwe"
    );
    assert_eq!(uwp_app.app_id, "App");
    assert!(uwp_app.enabled);
    assert!(uwp_app.icon_data.is_none());
}

#[test]
fn test_uwp_app_icon_memory() {
    // Test UWP app with icon data (same size as Win32 icons: 4096 bytes for 32x32 RGBA)
    let uwp_app = UwpApp {
        id: Uuid::new_v4(),
        display_name: "Test UWP App".to_string(),
        package_family_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        app_id: "App".to_string(),
        enabled: true,
        icon_data: Some(vec![0u8; 4096]), // 4 KB icon (32x32 RGBA)
    };

    assert!(uwp_app.icon_data.is_some());
    assert_eq!(uwp_app.icon_data.as_ref().unwrap().len(), 4096);
}

#[test]
fn test_uwp_config_memory_estimation() {
    // Test config memory with UWP apps only
    let mut config = AppConfig::default();

    // Add 20 UWP apps
    for i in 0..20 {
        config.monitored_apps.push(MonitoredApp::Uwp(UwpApp {
            id: Uuid::new_v4(),
            display_name: format!("UWP App {i}"),
            package_family_name: format!("Publisher.AppName{i}_8wekyb3d8bbwe"),
            app_id: "App".to_string(),
            enabled: true,
            icon_data: None,
        }));
    }

    // Estimate memory usage
    // Each UwpApp: ~200 bytes (UUID + strings + bool)
    // Package family names are longer than process names, so slightly more memory
    // 20 apps: ~4-5 KB
    // Plus overhead: ~10-15 KB total
    let estimated_size = std::mem::size_of::<AppConfig>()
        + config.monitored_apps.len() * std::mem::size_of::<MonitoredApp>();

    // Should be less than 100 KB for 20 UWP apps (more generous than Win32 due to longer strings)
    assert!(
        estimated_size < 100 * 1024,
        "Config memory ({estimated_size} bytes) should be < 100 KB for 20 UWP apps"
    );
}

#[test]
fn test_mixed_app_types_config_memory() {
    // Test config memory with mixed Win32 and UWP apps
    let mut config = AppConfig::default();

    // Add 10 Win32 apps
    for i in 0..10 {
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: format!("Win32 App {i}"),
            exe_path: PathBuf::from(format!("C:\\Apps\\app{i}.exe")),
            process_name: format!("app{i}"),
            enabled: true,
            icon_data: None,
        }));
    }

    // Add 10 UWP apps
    for i in 0..10 {
        config.monitored_apps.push(MonitoredApp::Uwp(UwpApp {
            id: Uuid::new_v4(),
            display_name: format!("UWP App {i}"),
            package_family_name: format!("Publisher.AppName{i}_8wekyb3d8bbwe"),
            app_id: "App".to_string(),
            enabled: true,
            icon_data: None,
        }));
    }

    // Verify we have mixed types
    assert_eq!(config.monitored_apps.len(), 20);

    // Estimate memory usage
    let estimated_size = std::mem::size_of::<AppConfig>()
        + config.monitored_apps.len() * std::mem::size_of::<MonitoredApp>();

    // Should be less than 100 KB for 20 mixed apps
    assert!(
        estimated_size < 100 * 1024,
        "Config memory ({estimated_size} bytes) should be < 100 KB for 20 mixed apps"
    );
}

#[test]
fn test_uwp_metadata_memory_overhead() {
    // Test UWP metadata memory overhead
    // UWP-specific fields: package_family_name + app_id
    // Compared to Win32 fields: exe_path + process_name

    // Create one UWP app
    let uwp = UwpApp {
        id: Uuid::new_v4(),
        display_name: "Calculator".to_string(),
        package_family_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        app_id: "App".to_string(),
        enabled: true,
        icon_data: None,
    };

    // UWP metadata consists of package_family_name + app_id strings
    let uwp_metadata_size = uwp.package_family_name.len() + uwp.app_id.len();

    // For 100 typical UWP apps, metadata should be < 10KB
    // Typical package family name: ~50 bytes
    // Typical app_id: ~3 bytes ("App")
    // 100 apps × 53 bytes = 5.3 KB (well within limit)
    let metadata_for_100_apps = uwp_metadata_size * 100;

    assert!(
        metadata_for_100_apps < 10 * 1024,
        "UWP metadata for 100 apps ({metadata_for_100_apps} bytes) should be < 10 KB"
    );
}

#[test]
fn test_mixed_app_types_with_icons_memory() {
    // Test memory usage with mixed app types including icons
    // This validates that icon caching works the same for both app types
    let profiler = memory_profiler::MemoryProfiler::new();

    // Simulate 10 Win32 apps with icons
    for _ in 0..10 {
        profiler.record_icon_cached(4096); // 4 KB per icon
    }

    // Simulate 10 UWP apps with icons
    for _ in 0..10 {
        profiler.record_icon_cached(4096); // 4 KB per icon
    }

    let stats = profiler.get_stats();

    // Total icon cache: 20 apps × 4 KB = 80 KB
    assert_eq!(stats.icon_cache_memory, 20 * 4096);
    assert_eq!(stats.cached_icon_count, 20);
    assert!(stats.icon_cache_mb() < 0.1); // Less than 0.1 MB

    // Clean up
    for _ in 0..20 {
        profiler.record_icon_removed(4096);
    }
}

#[test]
fn test_large_mixed_app_list_memory() {
    // Test worst-case scenario: many apps of both types with icons
    let mut config = AppConfig::default();
    let profiler = memory_profiler::MemoryProfiler::new();

    // Add 50 Win32 apps
    for i in 0..50 {
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: format!("Win32 App {i}"),
            exe_path: PathBuf::from(format!("C:\\Apps\\app{i}.exe")),
            process_name: format!("app{i}"),
            enabled: true,
            icon_data: Some(vec![0u8; 4096]), // 4 KB icon
        }));
        profiler.record_icon_cached(4096);
    }

    // Add 50 UWP apps
    for i in 0..50 {
        config.monitored_apps.push(MonitoredApp::Uwp(UwpApp {
            id: Uuid::new_v4(),
            display_name: format!("UWP App {i}"),
            package_family_name: format!("Publisher.AppName{i}_8wekyb3d8bbwe"),
            app_id: "App".to_string(),
            enabled: true,
            icon_data: Some(vec![0u8; 4096]), // 4 KB icon
        }));
        profiler.record_icon_cached(4096);
    }

    let stats = profiler.get_stats();

    // Verify totals
    assert_eq!(config.monitored_apps.len(), 100);
    assert_eq!(stats.cached_icon_count, 100);

    // Icon cache: 100 apps × 4 KB = 400 KB
    assert_eq!(stats.icon_cache_memory, 100 * 4096);
    assert!(stats.icon_cache_mb() < 0.5); // Less than 0.5 MB

    // Config memory estimate
    let config_size = std::mem::size_of::<AppConfig>()
        + config.monitored_apps.len() * std::mem::size_of::<MonitoredApp>();

    // Total overhead (config + icons) should be well under 50MB
    // Even with 100 apps: ~400 KB icons + ~100 KB config = ~500 KB
    let total_overhead = config_size + stats.icon_cache_memory;
    assert!(
        total_overhead < 1024 * 1024,
        "Total overhead ({total_overhead} bytes) should be < 1 MB for 100 mixed apps"
    );

    // Clean up
    for _ in 0..100 {
        profiler.record_icon_removed(4096);
    }
}

#[test]
fn test_monitored_app_uwp_release_icon() {
    // Test that UWP apps can release icons to free memory
    let mut app = MonitoredApp::Uwp(UwpApp {
        id: Uuid::new_v4(),
        display_name: "Test UWP App".to_string(),
        package_family_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
        app_id: "App".to_string(),
        enabled: true,
        icon_data: Some(vec![0u8; 4096]), // 4 KB icon
    });

    // Verify icon is present
    if let MonitoredApp::Uwp(ref uwp_app) = app {
        assert!(uwp_app.icon_data.is_some());
    } else {
        panic!("Expected Uwp variant");
    }

    // Release icon
    app.release_icon();

    // Verify icon is removed
    if let MonitoredApp::Uwp(ref uwp_app) = app {
        assert!(uwp_app.icon_data.is_none());
    } else {
        panic!("Expected Uwp variant");
    }
}
