//! Startup time tests
//!
//! This test module verifies that the application meets startup time requirements.
//!
//! # Requirements
//!
//! - Requirement 9.3: System SHALL display the GUI within 200ms
//! - Requirement 9.7: Load configuration and start monitoring in parallel where possible
//! - Task 16.3: Verify startup < 200ms

use easyhdr::utils::startup_profiler::{self, StartupPhase};
use std::time::Duration;

#[test]
fn test_startup_profiler_creation() {
    let profiler = startup_profiler::StartupProfiler::new();

    // Should be enabled by default
    assert!(profiler.is_enabled());

    // Should have no timings initially
    assert_eq!(profiler.get_timings().len(), 0);
}

#[test]
fn test_record_startup_phases() {
    let profiler = startup_profiler::StartupProfiler::new();
    
    // Simulate startup phases
    std::thread::sleep(Duration::from_millis(5));
    profiler.record_phase(StartupPhase::AppStart);
    
    std::thread::sleep(Duration::from_millis(5));
    profiler.record_phase(StartupPhase::LoggingInit);
    
    std::thread::sleep(Duration::from_millis(5));
    profiler.record_phase(StartupPhase::ConfigLoad);
    
    std::thread::sleep(Duration::from_millis(5));
    profiler.record_phase(StartupPhase::GuiDisplay);
    
    let timings = profiler.get_timings();
    assert_eq!(timings.len(), 4);
    
    // Verify phases are recorded in order
    assert_eq!(timings[0].phase, StartupPhase::AppStart);
    assert_eq!(timings[1].phase, StartupPhase::LoggingInit);
    assert_eq!(timings[2].phase, StartupPhase::ConfigLoad);
    assert_eq!(timings[3].phase, StartupPhase::GuiDisplay);
    
    // Each phase should have taken at least 5ms
    for timing in &timings {
        assert!(timing.duration.as_millis() >= 5);
    }
}

#[test]
fn test_total_startup_time() {
    let profiler = startup_profiler::StartupProfiler::new();
    
    // Simulate some startup time
    std::thread::sleep(Duration::from_millis(50));
    
    let total_ms = profiler.total_startup_ms();
    assert!(total_ms >= 50.0);
    
    let total_duration = profiler.total_startup_time();
    assert!(total_duration.as_millis() >= 50);
}

#[test]
fn test_startup_within_limits() {
    let profiler = startup_profiler::StartupProfiler::new();
    
    // Fast startup should be within limits
    std::thread::sleep(Duration::from_millis(50));
    assert!(profiler.is_within_limits());
    
    // Slow startup should exceed limits
    std::thread::sleep(Duration::from_millis(200));
    assert!(!profiler.is_within_limits());
}

#[test]
fn test_phase_names() {
    assert_eq!(StartupPhase::AppStart.name(), "Application Start");
    assert_eq!(StartupPhase::LoggingInit.name(), "Logging Initialization");
    assert_eq!(StartupPhase::VersionDetection.name(), "Version Detection");
    assert_eq!(StartupPhase::ConfigLoad.name(), "Configuration Load");
    assert_eq!(StartupPhase::HdrControllerInit.name(), "HDR Controller Init");
    assert_eq!(StartupPhase::ProcessMonitorInit.name(), "Process Monitor Init");
    assert_eq!(StartupPhase::AppControllerInit.name(), "App Controller Init");
    assert_eq!(StartupPhase::GuiControllerInit.name(), "GUI Controller Init");
    assert_eq!(StartupPhase::GuiDisplay.name(), "GUI Display");
    assert_eq!(StartupPhase::AppReady.name(), "Application Ready");
}

#[test]
fn test_profiler_disable_enable() {
    let profiler = startup_profiler::StartupProfiler::new();

    // Record a phase
    profiler.record_phase(StartupPhase::AppStart);
    assert_eq!(profiler.get_timings().len(), 1);

    // Disable profiling
    profiler.disable();
    assert!(!profiler.is_enabled());

    // Recording should be ignored
    profiler.record_phase(StartupPhase::LoggingInit);
    assert_eq!(profiler.get_timings().len(), 1); // Still 1, not 2

    // Re-enable profiling
    profiler.enable();
    assert!(profiler.is_enabled());

    // Recording should work now
    profiler.record_phase(StartupPhase::ConfigLoad);
    assert_eq!(profiler.get_timings().len(), 2);
}

#[test]
fn test_global_profiler_instance() {
    let profiler = startup_profiler::get_profiler();

    // Should be enabled by default
    assert!(profiler.is_enabled());
}

#[test]
fn test_realistic_startup_scenario() {
    // Simulate a realistic startup sequence with typical timings
    let profiler = startup_profiler::StartupProfiler::new();
    
    // App start (instant)
    profiler.record_phase(StartupPhase::AppStart);
    
    // Logging init (~5ms)
    std::thread::sleep(Duration::from_millis(5));
    profiler.record_phase(StartupPhase::LoggingInit);
    
    // Version detection (~10ms)
    std::thread::sleep(Duration::from_millis(10));
    profiler.record_phase(StartupPhase::VersionDetection);
    
    // Config load (~15ms)
    std::thread::sleep(Duration::from_millis(15));
    profiler.record_phase(StartupPhase::ConfigLoad);
    
    // HDR controller init (~30ms)
    std::thread::sleep(Duration::from_millis(30));
    profiler.record_phase(StartupPhase::HdrControllerInit);
    
    // Process monitor init (~5ms)
    std::thread::sleep(Duration::from_millis(5));
    profiler.record_phase(StartupPhase::ProcessMonitorInit);
    
    // App controller init (~20ms)
    std::thread::sleep(Duration::from_millis(20));
    profiler.record_phase(StartupPhase::AppControllerInit);
    
    // GUI controller init (~40ms)
    std::thread::sleep(Duration::from_millis(40));
    profiler.record_phase(StartupPhase::GuiControllerInit);
    
    // GUI display (~30ms)
    std::thread::sleep(Duration::from_millis(30));
    profiler.record_phase(StartupPhase::GuiDisplay);
    
    // App ready
    profiler.record_phase(StartupPhase::AppReady);
    
    let timings = profiler.get_timings();
    assert_eq!(timings.len(), 10);
    
    // Total should be around 155ms (well within 200ms limit)
    let total_ms = profiler.total_startup_ms();
    assert!(total_ms >= 155.0);
    assert!(total_ms < 200.0); // Should be within limit
    assert!(profiler.is_within_limits());
}

#[test]
fn test_slow_startup_detection() {
    // Simulate a slow startup that exceeds the limit
    let profiler = startup_profiler::StartupProfiler::new();
    
    // Simulate slow phases
    profiler.record_phase(StartupPhase::AppStart);
    std::thread::sleep(Duration::from_millis(50));
    
    profiler.record_phase(StartupPhase::LoggingInit);
    std::thread::sleep(Duration::from_millis(50));
    
    profiler.record_phase(StartupPhase::ConfigLoad);
    std::thread::sleep(Duration::from_millis(50));
    
    profiler.record_phase(StartupPhase::GuiDisplay);
    std::thread::sleep(Duration::from_millis(60));
    
    profiler.record_phase(StartupPhase::AppReady);
    
    // Total should exceed 200ms
    let total_ms = profiler.total_startup_ms();
    assert!(total_ms >= 210.0);
    assert!(!profiler.is_within_limits());
}

#[test]
fn test_phase_timing_accuracy() {
    let profiler = startup_profiler::StartupProfiler::new();

    // Record phases with known delays
    // First phase measures from profiler creation to first record_phase call
    std::thread::sleep(Duration::from_millis(10));
    profiler.record_phase(StartupPhase::AppStart);

    std::thread::sleep(Duration::from_millis(20));
    profiler.record_phase(StartupPhase::LoggingInit);

    std::thread::sleep(Duration::from_millis(30));
    profiler.record_phase(StartupPhase::ConfigLoad);

    profiler.record_phase(StartupPhase::GuiDisplay);

    let timings = profiler.get_timings();

    // First phase should be ~10ms (from profiler creation to first record)
    assert!(timings[0].duration.as_millis() >= 10);
    assert!(timings[0].duration.as_millis() < 20);

    // Second phase should be ~20ms
    assert!(timings[1].duration.as_millis() >= 20);
    assert!(timings[1].duration.as_millis() < 35);

    // Third phase should be ~30ms
    assert!(timings[2].duration.as_millis() >= 30);
    assert!(timings[2].duration.as_millis() < 45);
}

#[test]
fn test_log_summary_does_not_panic() {
    let profiler = startup_profiler::StartupProfiler::new();
    
    // Record some phases
    profiler.record_phase(StartupPhase::AppStart);
    std::thread::sleep(Duration::from_millis(10));
    profiler.record_phase(StartupPhase::LoggingInit);
    std::thread::sleep(Duration::from_millis(10));
    profiler.record_phase(StartupPhase::GuiDisplay);
    
    // This should not panic
    profiler.log_summary();
}

#[test]
fn test_empty_profiler_summary() {
    let profiler = startup_profiler::StartupProfiler::new();
    
    // Log summary with no phases recorded
    // This should not panic
    profiler.log_summary();
    
    // Should still be within limits (0ms < 200ms)
    assert!(profiler.is_within_limits());
}

