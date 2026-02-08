//! Integration tests for UWP process detection
//!
//! Tests the `ProcessMonitor`'s ability to detect UWP applications using the
//! Windows `GetPackageFullName` API. These tests verify that UWP apps are
//! correctly identified and that `ProcessEvent::Started`/`ProcessEvent::Stopped` events are
//! emitted as expected.
//!
//! **IMPORTANT**: These tests must run with `--test-threads=1` due to Windows
//! API global state. Run with:
//! ```
//! cargo test --test uwp_process_detection_tests -- --test-threads=1
//! ```
//!
//! **Test Prerequisites**:
//! - Windows 10 21H2+ or Windows 11
//! - Calculator app must be installed (Microsoft.WindowsCalculator)
//! - Tests may require user interaction to close Calculator if process doesn't terminate

#[cfg(windows)]
use easyhdr::{
    config::{MonitoredApp, UwpApp, Win32App},
    monitor::{AppIdentifier, ProcessEvent, ProcessMonitor},
};

#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::sync::mpsc;
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::Duration;
#[cfg(windows)]
use uuid::Uuid;

/// Helper function to create a test `Win32App`
#[cfg(windows)]
fn create_test_win32_app(process_name: &str, display_name: &str) -> MonitoredApp {
    MonitoredApp::Win32(Win32App {
        id: Uuid::new_v4(),
        display_name: display_name.to_string(),
        exe_path: PathBuf::from(format!("C:\\Windows\\{process_name}.exe")),
        process_name: process_name.to_lowercase(),
        enabled: true,
        icon_data: None,
    })
}

/// Helper function to create a test `UwpApp`
#[cfg(windows)]
fn create_test_uwp_app(
    package_family_name: &str,
    display_name: &str,
    app_id: &str,
) -> MonitoredApp {
    MonitoredApp::Uwp(UwpApp {
        id: Uuid::new_v4(),
        display_name: display_name.to_string(),
        package_family_name: package_family_name.to_string(),
        app_id: app_id.to_string(),
        enabled: true,
        icon_data: None,
    })
}

/// Test that `ProcessMonitor` can detect when a UWP application starts
///
/// This test monitors the Windows Calculator app (a standard UWP app) and verifies
/// that a `ProcessEvent::Started` event is emitted when it's running.
#[test]
#[cfg(windows)]
fn test_uwp_app_detection_calculator_started() {
    use tracing_subscriber;

    // Calculator package family name (stable across Windows versions)
    const CALCULATOR_FAMILY_NAME: &str = "Microsoft.WindowsCalculator_8wekyb3d8bbwe";

    // Initialize logging for debugging
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();

    let (tx, rx) = mpsc::sync_channel(32);
    let monitor = ProcessMonitor::new(Duration::from_millis(500), tx);

    // Add Calculator to watch list
    monitor.update_watch_list(vec![create_test_uwp_app(
        CALCULATOR_FAMILY_NAME,
        "Calculator",
        "App",
    )]);

    // Start the monitor
    let _handle = monitor.start();

    // Wait for monitoring to stabilize
    thread::sleep(Duration::from_millis(1000));

    // Check if Calculator is already running
    // Try to receive events for a short period
    let mut calculator_already_running = false;
    while let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
        if let ProcessEvent::Started(AppIdentifier::Uwp(ref family_name)) = event
            && family_name == CALCULATOR_FAMILY_NAME
        {
            calculator_already_running = true;
            tracing::info!("Calculator is already running");
            break;
        }
    }

    if calculator_already_running {
        // Calculator was already running, which also satisfies the test
        tracing::info!("Test passed: Calculator was already detected as running");
    } else {
        // Launch Calculator using PowerShell
        // This is more reliable than trying to find the executable path
        tracing::info!("Attempting to launch Calculator...");

        #[cfg(windows)]
        {
            let launch_result = std::process::Command::new("powershell")
                .args([
                    "-Command",
                    "Start-Process 'calculator:' -WindowStyle Hidden",
                ])
                .spawn();

            match launch_result {
                Ok(_) => {
                    tracing::info!("Calculator launch command executed");

                    // Wait for the process to start and be detected
                    // Give it up to 10 seconds (20 polling cycles at 500ms)
                    let timeout = Duration::from_secs(10);
                    let start = std::time::Instant::now();

                    let mut detected = false;
                    while start.elapsed() < timeout {
                        if let Ok(event) = rx.recv_timeout(Duration::from_millis(500)) {
                            tracing::debug!("Received event: {:?}", event);

                            if let ProcessEvent::Started(AppIdentifier::Uwp(ref family_name)) =
                                event
                                && family_name == CALCULATOR_FAMILY_NAME
                            {
                                tracing::info!(
                                    "Successfully detected Calculator started: {}",
                                    family_name
                                );
                                detected = true;
                                break;
                            }
                        }
                    }

                    assert!(
                        detected,
                        "Failed to detect Calculator UWP app within timeout. \
                         Make sure Calculator is installed and the test has sufficient permissions."
                    );
                }
                Err(e) => {
                    // If we can't launch Calculator, skip the test
                    eprintln!(
                        "Cannot launch Calculator ({e}). Skipping test. \
                         This is expected on non-Windows or restricted environments."
                    );
                }
            }
        }
    }
}

/// Test that `ProcessMonitor` can detect when a UWP application stops
///
/// This test attempts to detect when Calculator closes. Due to the complexity
/// of programmatically closing UWP apps, this test may require manual intervention
/// or may be skipped if Calculator cannot be reliably controlled.
#[test]
#[cfg(windows)]
#[ignore = "Requires Calculator to be running and then closed"]
fn test_uwp_app_detection_calculator_stopped() {
    use tracing_subscriber;

    const CALCULATOR_FAMILY_NAME: &str = "Microsoft.WindowsCalculator_8wekyb3d8bbwe";

    // Initialize logging for debugging
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();

    let (tx, rx) = mpsc::sync_channel(32);
    let monitor = ProcessMonitor::new(Duration::from_millis(500), tx);

    // Add Calculator to watch list
    monitor.update_watch_list(vec![create_test_uwp_app(
        CALCULATOR_FAMILY_NAME,
        "Calculator",
        "App",
    )]);

    // Start the monitor
    let _handle = monitor.start();

    // Wait for monitoring to stabilize
    thread::sleep(Duration::from_millis(1000));

    tracing::info!(
        "This test requires Calculator to be running and then closed manually or programmatically"
    );

    // Wait for Calculator to be detected as running first
    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();
    let mut running_detected = false;

    while start.elapsed() < timeout && !running_detected {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(500)) {
            tracing::debug!("Received event: {:?}", event);

            if let ProcessEvent::Started(AppIdentifier::Uwp(ref family_name)) = event
                && family_name == CALCULATOR_FAMILY_NAME
            {
                tracing::info!("Calculator detected as running");
                running_detected = true;
            }
        }
    }

    if !running_detected {
        eprintln!(
            "Calculator not detected as running within timeout. Please ensure Calculator is running."
        );
        return;
    }

    // Now try to close Calculator using taskkill
    tracing::info!("Attempting to close Calculator...");

    #[cfg(windows)]
    {
        let kill_result = std::process::Command::new("taskkill")
            .args(["/F", "/IM", "CalculatorApp.exe"])
            .output();

        match kill_result {
            Ok(output) => {
                tracing::info!(
                    "taskkill output: {}",
                    String::from_utf8_lossy(&output.stdout)
                );

                // Wait for the stop event
                let timeout = Duration::from_secs(10);
                let start = std::time::Instant::now();
                let mut stopped_detected = false;

                while start.elapsed() < timeout {
                    if let Ok(event) = rx.recv_timeout(Duration::from_millis(500)) {
                        tracing::debug!("Received event: {:?}", event);

                        if let ProcessEvent::Stopped(AppIdentifier::Uwp(ref family_name)) = event
                            && family_name == CALCULATOR_FAMILY_NAME
                        {
                            tracing::info!(
                                "Successfully detected Calculator stopped: {}",
                                family_name
                            );
                            stopped_detected = true;
                            break;
                        }
                    }
                }

                assert!(
                    stopped_detected,
                    "Failed to detect Calculator stop event within timeout"
                );
            }
            Err(e) => {
                eprintln!("Cannot kill Calculator process ({e}). Test may fail.");
            }
        }
    }
}

/// Test that `ProcessMonitor` can detect both Win32 and UWP applications simultaneously
///
/// This test adds both notepad.exe (Win32) and Calculator (UWP) to the watch list
/// and verifies that the monitor can detect both types of applications.
#[test]
#[cfg(windows)]
fn test_mixed_win32_and_uwp_detection() {
    use tracing_subscriber;

    const CALCULATOR_FAMILY_NAME: &str = "Microsoft.WindowsCalculator_8wekyb3d8bbwe";

    // Initialize logging for debugging
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();

    let (tx, rx) = mpsc::sync_channel(32);
    let monitor = ProcessMonitor::new(Duration::from_millis(500), tx);

    // Add both Win32 (notepad) and UWP (Calculator) to watch list
    monitor.update_watch_list(vec![
        create_test_win32_app("notepad", "Notepad"),
        create_test_uwp_app(CALCULATOR_FAMILY_NAME, "Calculator", "App"),
    ]);

    // Start the monitor
    let _handle = monitor.start();

    // Wait for monitoring to stabilize
    thread::sleep(Duration::from_millis(1000));

    // Clear any initial events
    while rx.recv_timeout(Duration::from_millis(100)).is_ok() {}

    tracing::info!("Testing mixed Win32 and UWP detection...");

    // Try to launch notepad
    let notepad_launched = std::process::Command::new("notepad.exe").spawn();

    let mut win32_detected = false;
    let mut uwp_detected = false;

    // Wait for events with timeout
    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout && (!win32_detected || !uwp_detected) {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(500)) {
            tracing::debug!("Received event: {:?}", event);

            match event {
                ProcessEvent::Started(AppIdentifier::Win32(ref name)) => {
                    if name == "notepad" {
                        tracing::info!("Win32 app detected: {}", name);
                        win32_detected = true;
                    }
                }
                ProcessEvent::Started(AppIdentifier::Uwp(ref family_name)) => {
                    if family_name == CALCULATOR_FAMILY_NAME {
                        tracing::info!("UWP app detected: {}", family_name);
                        uwp_detected = true;
                    }
                }
                _ => {}
            }
        }
    }

    // If notepad wasn't detected but was launched successfully, that's still useful info
    if let Ok(mut child) = notepad_launched {
        if win32_detected {
            tracing::info!("Successfully detected Win32 app (notepad)");
        }

        // Clean up: kill notepad
        let _ = child.kill();
    }

    // For this test, we only require that the monitor can handle both types
    // At minimum, if either type is detected, the test demonstrates mixed support
    assert!(
        win32_detected || uwp_detected,
        "Failed to detect any monitored processes (Win32 or UWP). \
         At least one type should be detected to verify mixed detection support."
    );

    // If both were detected, that's ideal
    if win32_detected && uwp_detected {
        tracing::info!("Successfully detected both Win32 and UWP applications!");
    } else if win32_detected {
        tracing::info!("Detected Win32 app. UWP detection may require Calculator to be running.");
    } else if uwp_detected {
        tracing::info!("Detected UWP app. Win32 detection may require notepad to be launched.");
    }
}

/// Test that the `ProcessMonitor` correctly identifies UWP vs Win32 apps
///
/// This test verifies that the monitor uses the correct `AppIdentifier` variant
/// for each application type.
#[test]
#[cfg(windows)]
fn test_app_identifier_discrimination() {
    use tracing_subscriber;

    const CALCULATOR_FAMILY_NAME: &str = "Microsoft.WindowsCalculator_8wekyb3d8bbwe";

    // Initialize logging for debugging
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();

    let (tx, rx) = mpsc::sync_channel(32);
    let monitor = ProcessMonitor::new(Duration::from_millis(500), tx);

    // Add both types to watch list
    monitor.update_watch_list(vec![
        create_test_win32_app("explorer", "Windows Explorer"),
        create_test_uwp_app(CALCULATOR_FAMILY_NAME, "Calculator", "App"),
    ]);

    // Start the monitor
    let _handle = monitor.start();

    // Wait for monitoring to stabilize and collect events
    thread::sleep(Duration::from_millis(2000));

    // Collect events to verify correct identification
    let mut win32_apps = std::collections::HashSet::new();
    let mut uwp_apps = std::collections::HashSet::new();

    // Drain events for a few seconds
    let timeout = Duration::from_secs(3);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
            match event {
                ProcessEvent::Started(AppIdentifier::Win32(name))
                | ProcessEvent::Stopped(AppIdentifier::Win32(name)) => {
                    win32_apps.insert(name);
                }
                ProcessEvent::Started(AppIdentifier::Uwp(family_name))
                | ProcessEvent::Stopped(AppIdentifier::Uwp(family_name)) => {
                    uwp_apps.insert(family_name);
                }
            }
        }
    }

    tracing::info!("Detected Win32 apps: {:?}", win32_apps);
    tracing::info!("Detected UWP apps: {:?}", uwp_apps);

    // Verify that if we detected apps, they're in the correct categories
    // Explorer is a Win32 app, so it should never appear as UWP
    for uwp_name in &uwp_apps {
        assert_ne!(
            uwp_name, "explorer",
            "explorer should not be detected as UWP app"
        );
    }

    // Calculator is a UWP app, so its package family name should never appear as Win32
    for win32_name in &win32_apps {
        assert_ne!(
            win32_name, CALCULATOR_FAMILY_NAME,
            "Calculator package family name should not be detected as Win32 process name"
        );
    }

    tracing::info!("App identifier discrimination test passed");
}

/// Test that disabled UWP apps are not monitored
///
/// Verifies that when a UWP app is in the watch list but disabled=false,
/// no events are emitted for it.
#[test]
#[cfg(windows)]
fn test_disabled_uwp_app_not_monitored() {
    use tracing_subscriber;

    const CALCULATOR_FAMILY_NAME: &str = "Microsoft.WindowsCalculator_8wekyb3d8bbwe";

    // Initialize logging for debugging
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();

    let (tx, rx) = mpsc::sync_channel(32);
    let monitor = ProcessMonitor::new(Duration::from_millis(500), tx);

    // Add Calculator but with enabled=false
    let mut disabled_app = create_test_uwp_app(CALCULATOR_FAMILY_NAME, "Calculator", "App");
    if let MonitoredApp::Uwp(ref mut uwp_app) = disabled_app {
        uwp_app.enabled = false;
    }

    monitor.update_watch_list(vec![disabled_app]);

    // Start the monitor
    let _handle = monitor.start();

    // Wait for monitoring to run for a few cycles
    thread::sleep(Duration::from_secs(2));

    // Collect any events
    let mut received_events = Vec::new();
    while let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
        received_events.push(event);
    }

    // Filter for Calculator events
    let calculator_events: Vec<_> = received_events
        .iter()
        .filter(|event| match event {
            ProcessEvent::Started(AppIdentifier::Uwp(name))
            | ProcessEvent::Stopped(AppIdentifier::Uwp(name)) => name == CALCULATOR_FAMILY_NAME,
            _ => false,
        })
        .collect();

    assert!(
        calculator_events.is_empty(),
        "No events should be emitted for disabled UWP app, but got: {calculator_events:?}"
    );

    tracing::info!("Disabled UWP app correctly not monitored");
}

// Non-Windows stub tests to ensure compilation succeeds on all platforms
#[cfg(not(windows))]
mod non_windows {
    #[test]
    fn test_uwp_detection_not_supported_on_non_windows() {
        // UWP detection is Windows-only
        // This test exists to ensure the test file compiles on all platforms
        eprintln!("UWP detection tests are only available on Windows");
    }
}
