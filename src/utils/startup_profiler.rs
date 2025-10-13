//! Startup time profiling utilities
//!
//! This module provides utilities for profiling and measuring startup time
//! from application launch to GUI display. It helps track initialization
//! performance and identify optimization opportunities.
//!
//! # Requirements
//!
//! - Requirement 9.3: System SHALL display the GUI within 200ms
//! - Requirement 9.7: Load configuration and start monitoring in parallel where possible
//! - Task 16.3: Profile startup time and optimize if needed

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Startup phase identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupPhase {
    /// Application entry point
    AppStart,
    /// Logging system initialization
    LoggingInit,
    /// Windows version detection
    VersionDetection,
    /// Configuration loading
    ConfigLoad,
    /// HDR controller initialization
    HdrControllerInit,
    /// Process monitor creation
    ProcessMonitorInit,
    /// Application controller creation
    AppControllerInit,
    /// GUI controller creation
    GuiControllerInit,
    /// GUI window display
    GuiDisplay,
    /// Application fully initialized
    AppReady,
}

impl StartupPhase {
    /// Get a human-readable name for the phase
    pub fn name(&self) -> &'static str {
        match self {
            StartupPhase::AppStart => "Application Start",
            StartupPhase::LoggingInit => "Logging Initialization",
            StartupPhase::VersionDetection => "Version Detection",
            StartupPhase::ConfigLoad => "Configuration Load",
            StartupPhase::HdrControllerInit => "HDR Controller Init",
            StartupPhase::ProcessMonitorInit => "Process Monitor Init",
            StartupPhase::AppControllerInit => "App Controller Init",
            StartupPhase::GuiControllerInit => "GUI Controller Init",
            StartupPhase::GuiDisplay => "GUI Display",
            StartupPhase::AppReady => "Application Ready",
        }
    }
}

/// Timing information for a startup phase
#[derive(Debug, Clone)]
pub struct PhaseTimings {
    /// The startup phase
    pub phase: StartupPhase,
    /// Time when this phase started
    pub start_time: Instant,
    /// Duration of this phase
    pub duration: Duration,
}

/// Startup profiler for tracking initialization performance
pub struct StartupProfiler {
    /// Application start time
    app_start: Instant,
    /// Last phase end time (protected by mutex)
    last_phase_end: parking_lot::Mutex<Instant>,
    /// Recorded phase timings
    timings: parking_lot::Mutex<Vec<PhaseTimings>>,
    /// Whether profiling is enabled
    enabled: AtomicBool,
}

impl StartupProfiler {
    /// Create a new startup profiler
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            app_start: now,
            last_phase_end: parking_lot::Mutex::new(now),
            timings: parking_lot::Mutex::new(Vec::new()),
            enabled: AtomicBool::new(true),
        }
    }

    /// Record the start of a phase
    ///
    /// This method records the time when a startup phase begins and calculates
    /// the duration since the last phase ended.
    ///
    /// # Arguments
    ///
    /// * `phase` - The startup phase being recorded
    pub fn record_phase(&self, phase: StartupPhase) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }

        let now = Instant::now();

        // Lock last_phase_end to get the previous time and update it
        let mut last_phase_end = self.last_phase_end.lock();
        let duration = now.duration_since(*last_phase_end);
        let start_time = *last_phase_end;

        debug!(
            "Startup phase: {} completed in {:.2}ms",
            phase.name(),
            duration.as_secs_f64() * 1000.0
        );

        let timing = PhaseTimings {
            phase,
            start_time,
            duration,
        };

        // Update last phase end time for next phase
        *last_phase_end = now;
        drop(last_phase_end);

        // Record the timing
        let mut timings = self.timings.lock();
        timings.push(timing);
    }

    /// Get total startup time from app start to current time
    pub fn total_startup_time(&self) -> Duration {
        Instant::now().duration_since(self.app_start)
    }

    /// Get total startup time in milliseconds
    pub fn total_startup_ms(&self) -> f64 {
        self.total_startup_time().as_secs_f64() * 1000.0
    }

    /// Check if startup time is within acceptable limits (< 200ms)
    pub fn is_within_limits(&self) -> bool {
        self.total_startup_ms() < 200.0
    }

    /// Get all recorded phase timings
    pub fn get_timings(&self) -> Vec<PhaseTimings> {
        self.timings.lock().clone()
    }

    /// Log startup performance summary
    pub fn log_summary(&self) {
        let total_ms = self.total_startup_ms();
        let timings = self.get_timings();

        info!("=== Startup Performance Summary ===");
        info!("Total startup time: {:.2}ms", total_ms);

        for timing in &timings {
            let phase_ms = timing.duration.as_secs_f64() * 1000.0;
            let percentage = (phase_ms / total_ms) * 100.0;
            info!(
                "  {}: {:.2}ms ({:.1}%)",
                timing.phase.name(),
                phase_ms,
                percentage
            );
        }

        if self.is_within_limits() {
            info!("✓ Startup time is within target limit of 200ms");
        } else {
            warn!(
                "⚠ Startup time ({:.2}ms) exceeds target limit of 200ms!",
                total_ms
            );

            // Identify slowest phases
            let mut sorted_timings = timings.clone();
            sorted_timings.sort_by(|a, b| b.duration.cmp(&a.duration));

            warn!("Slowest phases:");
            for timing in sorted_timings.iter().take(3) {
                let phase_ms = timing.duration.as_secs_f64() * 1000.0;
                warn!("  {}: {:.2}ms", timing.phase.name(), phase_ms);
            }
        }

        info!("===================================");
    }

    /// Disable profiling (for production use)
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Relaxed);
    }

    /// Enable profiling
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::Relaxed);
    }

    /// Check if profiling is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

impl Default for StartupProfiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Global startup profiler instance
static STARTUP_PROFILER: once_cell::sync::Lazy<StartupProfiler> =
    once_cell::sync::Lazy::new(StartupProfiler::new);

/// Get the global startup profiler instance
pub fn get_profiler() -> &'static StartupProfiler {
    &STARTUP_PROFILER
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_startup_profiler_creation() {
        let profiler = StartupProfiler::new();
        assert!(profiler.enabled.load(Ordering::Relaxed));
        assert_eq!(profiler.get_timings().len(), 0);
    }

    #[test]
    fn test_record_phase() {
        let profiler = StartupProfiler::new();

        // Record some phases
        thread::sleep(Duration::from_millis(10));
        profiler.record_phase(StartupPhase::LoggingInit);

        thread::sleep(Duration::from_millis(10));
        profiler.record_phase(StartupPhase::ConfigLoad);

        let timings = profiler.get_timings();
        assert_eq!(timings.len(), 2);
        assert_eq!(timings[0].phase, StartupPhase::LoggingInit);
        assert_eq!(timings[1].phase, StartupPhase::ConfigLoad);

        // Each phase should have taken at least 10ms
        assert!(timings[0].duration.as_millis() >= 10);
        assert!(timings[1].duration.as_millis() >= 10);
    }

    #[test]
    fn test_total_startup_time() {
        let profiler = StartupProfiler::new();

        thread::sleep(Duration::from_millis(50));

        let total_ms = profiler.total_startup_ms();
        assert!(total_ms >= 50.0);
    }

    #[test]
    fn test_is_within_limits() {
        let profiler = StartupProfiler::new();

        // Should be within limits initially
        assert!(profiler.is_within_limits());

        // Simulate slow startup
        thread::sleep(Duration::from_millis(250));

        // Should exceed limits now
        assert!(!profiler.is_within_limits());
    }

    #[test]
    fn test_phase_names() {
        assert_eq!(StartupPhase::AppStart.name(), "Application Start");
        assert_eq!(StartupPhase::LoggingInit.name(), "Logging Initialization");
        assert_eq!(StartupPhase::GuiDisplay.name(), "GUI Display");
    }

    #[test]
    fn test_disable_enable() {
        let profiler = StartupProfiler::new();

        // Disable profiling
        profiler.disable();
        assert!(!profiler.enabled.load(Ordering::Relaxed));

        // Recording should be ignored
        profiler.record_phase(StartupPhase::ConfigLoad);
        assert_eq!(profiler.get_timings().len(), 0);

        // Re-enable profiling
        profiler.enable();
        assert!(profiler.enabled.load(Ordering::Relaxed));

        // Recording should work now
        profiler.record_phase(StartupPhase::ConfigLoad);
        assert_eq!(profiler.get_timings().len(), 1);
    }

    #[test]
    fn test_global_profiler() {
        let profiler = get_profiler();

        // Should be enabled by default
        assert!(profiler.enabled.load(Ordering::Relaxed));
    }
}
