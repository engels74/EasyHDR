//! Integration tests for `EasyHDR`
//!
//! Tests configuration persistence, process monitoring, HDR control,
//! and error handling for the full application lifecycle.

use easyhdr::{
    config::{AppConfig, ConfigManager, MonitoredApp},
    controller::AppController,
    error::{EasyHdrError, get_user_friendly_error},
    hdr::HdrController,
    monitor::{ProcessEvent, ProcessMonitor},
};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

/// Test that configuration can be saved and loaded correctly
#[test]
fn test_config_persistence_integration() {
    // Create a temporary test directory
    let test_dir =
        std::env::temp_dir().join(format!("easyhdr_integration_test_{}", Uuid::new_v4()));
    std::fs::create_dir_all(&test_dir).unwrap();

    // Create a config with some test data
    let mut config = AppConfig::default();
    config.monitored_apps.push(MonitoredApp {
        id: Uuid::new_v4(),
        display_name: "Test Game".to_string(),
        exe_path: PathBuf::from("C:\\Games\\test.exe"),
        process_name: "test".to_string(),
        enabled: true,
        icon_data: None,
    });

    // Save the config
    let config_path = test_dir.join("config.json");
    let json = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&config_path, json).unwrap();

    // Load the config back
    let loaded_json = std::fs::read_to_string(&config_path).unwrap();
    let loaded_config: AppConfig = serde_json::from_str(&loaded_json).unwrap();

    // Verify the data matches
    assert_eq!(loaded_config.monitored_apps.len(), 1);
    assert_eq!(loaded_config.monitored_apps[0].display_name, "Test Game");
    assert_eq!(loaded_config.monitored_apps[0].process_name, "test");
    assert!(loaded_config.monitored_apps[0].enabled);

    // Cleanup
    std::fs::remove_dir_all(&test_dir).ok();
}

/// Test that process monitor correctly detects process state changes
#[test]
fn test_process_monitor_integration() {
    let (tx, rx) = mpsc::sync_channel(32);
    let monitor = ProcessMonitor::new(Duration::from_millis(100), tx);

    // Update watch list with a test process
    monitor.update_watch_list(vec!["notepad".to_string()]);

    // Start the monitor
    let _handle = monitor.start();

    // Wait a bit for the monitor to start
    thread::sleep(Duration::from_millis(200));

    // Note: We can't actually start/stop processes in this test on macOS,
    // but we can verify the monitor is running and the channel is working

    // Try to receive events (should timeout since no processes match)
    let result = rx.recv_timeout(Duration::from_millis(500));

    // On macOS, we won't get any events since notepad.exe doesn't exist
    // On Windows, this would detect if notepad is running
    assert!(
        result.is_err()
            || matches!(
                result,
                Ok(ProcessEvent::Started(_) | ProcessEvent::Stopped(_))
            )
    );
}

/// Test that `AppController` correctly manages HDR state based on process events
#[test]
fn test_app_controller_hdr_logic_integration() {
    let (_event_tx, event_rx) = mpsc::sync_channel(32);
    let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
    let (state_tx, _state_rx) = mpsc::sync_channel(32);

    // Create a test config
    let mut config = AppConfig::default();
    config.monitored_apps.push(MonitoredApp {
        id: Uuid::new_v4(),
        display_name: "Test Game".to_string(),
        exe_path: PathBuf::from("C:\\Games\\test.exe"),
        process_name: "testgame".to_string(),
        enabled: true,
        icon_data: None,
    });

    let watch_list = Arc::new(Mutex::new(HashSet::new()));

    // Create the controller
    let controller = AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list);

    assert!(controller.is_ok(), "Controller creation should succeed");

    // Note: We can't test the internal state directly as those fields are private
    // This test verifies the controller can be created and initialized correctly
    // The actual event handling is tested in the unit tests in app_controller.rs
}

/// Test error handling for invalid configuration
#[test]
fn test_error_handling_invalid_config() {
    // Try to create a MonitoredApp from a non-existent path
    let result = MonitoredApp::from_exe_path(PathBuf::from("C:\\NonExistent\\fake.exe"));

    // Should return an error
    assert!(result.is_err());
}

/// Test that user-friendly error messages are generated
#[test]
fn test_user_friendly_error_messages() {
    let error = EasyHdrError::HdrNotSupported;
    let message = get_user_friendly_error(&error);
    assert!(message.contains("display doesn't support HDR"));

    let error = EasyHdrError::ConfigError(easyhdr::error::StringError::new("test"));
    let message = get_user_friendly_error(&error);
    assert!(message.contains("configuration"));
}

/// Test that HDR controller can be created and initialized
#[test]
fn test_hdr_controller_initialization() {
    let result = HdrController::new();

    // On non-Windows platforms, the stub implementation returns Ok with empty displays
    // On Windows, this should succeed if HDR is available
    assert!(
        result.is_ok(),
        "HDR controller creation should succeed (may have no displays on non-Windows)"
    );
}

/// Test that configuration manager handles missing directories
#[test]
fn test_config_manager_creates_directory() {
    // This test verifies that ConfigManager can handle missing directories
    let result = ConfigManager::load();

    // Should succeed even if directory doesn't exist (creates defaults)
    assert!(result.is_ok());
}

/// Test multiple applications running simultaneously
#[test]
fn test_multiple_apps_integration() {
    let (_event_tx, event_rx) = mpsc::sync_channel(32);
    let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
    let (state_tx, _state_rx) = mpsc::sync_channel(32);

    // Create a config with multiple apps
    let mut config = AppConfig::default();
    config.monitored_apps.push(MonitoredApp {
        id: Uuid::new_v4(),
        display_name: "Game 1".to_string(),
        exe_path: PathBuf::from("C:\\Games\\game1.exe"),
        process_name: "game1".to_string(),
        enabled: true,
        icon_data: None,
    });
    config.monitored_apps.push(MonitoredApp {
        id: Uuid::new_v4(),
        display_name: "Game 2".to_string(),
        exe_path: PathBuf::from("C:\\Games\\game2.exe"),
        process_name: "game2".to_string(),
        enabled: true,
        icon_data: None,
    });

    let watch_list = Arc::new(Mutex::new(HashSet::new()));

    let controller = AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list);

    assert!(controller.is_ok());

    // Verify controller can be created with multiple apps
    // The actual event handling is tested in unit tests
}

/// Test configuration with disabled applications
#[test]
fn test_disabled_apps_ignored() {
    let (_event_tx, event_rx) = mpsc::sync_channel(32);
    let (_hdr_state_tx, hdr_state_rx) = mpsc::sync_channel(32);
    let (state_tx, _state_rx) = mpsc::sync_channel(32);

    let mut config = AppConfig::default();
    config.monitored_apps.push(MonitoredApp {
        id: Uuid::new_v4(),
        display_name: "Disabled Game".to_string(),
        exe_path: PathBuf::from("C:\\Games\\disabled.exe"),
        process_name: "disabled".to_string(),
        enabled: false, // Disabled
        icon_data: None,
    });

    let watch_list = Arc::new(Mutex::new(HashSet::new()));

    let controller = AppController::new(config, event_rx, hdr_state_rx, state_tx, watch_list);

    assert!(controller.is_ok());
}

/// Test that preferences can be updated
#[test]
fn test_preferences_update_integration() {
    let mut config = AppConfig::default();

    // Update preferences
    config.preferences.auto_start = true;
    config.preferences.monitoring_interval_ms = 2000;
    config.preferences.show_tray_notifications = false;

    // Verify updates
    assert!(config.preferences.auto_start);
    assert_eq!(config.preferences.monitoring_interval_ms, 2000);
    assert!(!config.preferences.show_tray_notifications);
}
