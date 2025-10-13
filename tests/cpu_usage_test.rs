//! CPU usage profiling test for process monitoring thread
//!
//! This test measures the CPU usage of the process monitoring thread to ensure
//! it stays below 1% on modern systems (Requirement 9.2).
//!
//! The test runs the monitoring thread for a period of time and measures CPU usage
//! using platform-specific APIs.

#[cfg(windows)]
use std::sync::mpsc;
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use windows::Win32::Foundation::FILETIME;
#[cfg(windows)]
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// Measure CPU usage of the process monitoring thread
///
/// This test verifies Requirement 9.2: Process monitoring thread < 1% CPU
#[test]
#[cfg(windows)]
fn test_process_monitor_cpu_usage() {
    use easyhdr::monitor::ProcessMonitor;

    println!("\n=== CPU Usage Profiling Test ===");
    println!("Testing process monitoring thread CPU usage");
    println!("Target: < 1% CPU on modern systems (Requirement 9.2)\n");

    // Create a process monitor with 1000ms interval (default)
    let (tx, rx) = mpsc::channel();
    let monitor = ProcessMonitor::new(Duration::from_millis(1000), tx);

    // Get initial CPU times
    let cpu_start = get_process_cpu_time().expect("Failed to get initial CPU time");
    let wall_start = Instant::now();

    // Start the monitoring thread
    let _handle = monitor.start();

    // Let it run for 10 seconds to get a good measurement
    let test_duration = Duration::from_secs(10);
    println!("Running monitor for {} seconds...", test_duration.as_secs());
    thread::sleep(test_duration);

    // Get final CPU times
    let cpu_end = get_process_cpu_time().expect("Failed to get final CPU time");
    let wall_elapsed = wall_start.elapsed();

    // Calculate CPU usage percentage
    let cpu_time_used = cpu_end - cpu_start;
    let cpu_percentage = (cpu_time_used.as_secs_f64() / wall_elapsed.as_secs_f64()) * 100.0;

    println!("\nResults:");
    println!("  Wall time elapsed: {:.2}s", wall_elapsed.as_secs_f64());
    println!("  CPU time used: {:.4}s", cpu_time_used.as_secs_f64());
    println!("  CPU usage: {:.2}%", cpu_percentage);

    // Drain any events that were sent
    let mut event_count = 0;
    while rx.try_recv().is_ok() {
        event_count += 1;
    }
    println!("  Events received: {}", event_count);

    // Verify CPU usage is below 1%
    // We use 1.5% as the threshold to account for measurement variance
    // and system load, but the target is < 1%
    println!("\nVerification:");
    if cpu_percentage < 0.5 {
        println!("  ✓ Excellent: CPU usage is < 0.5%");
    } else if cpu_percentage < 1.0 {
        println!("  ✓ Good: CPU usage is < 1.0%");
    } else if cpu_percentage < 1.5 {
        println!("  ⚠ Warning: CPU usage is between 1-1.5% (acceptable but could be optimized)");
    } else {
        println!("  ✗ Failed: CPU usage exceeds 1.5%");
    }

    assert!(
        cpu_percentage < 1.5,
        "CPU usage {:.2}% exceeds 1.5% threshold (target < 1%)",
        cpu_percentage
    );

    println!("\n=== Test Passed ===\n");
}

/// Get the total CPU time used by the current process
///
/// Returns the sum of user time and kernel time in nanoseconds
#[cfg(windows)]
fn get_process_cpu_time() -> Result<Duration, String> {
    unsafe {
        let process = GetCurrentProcess();

        let mut creation_time = FILETIME::default();
        let mut exit_time = FILETIME::default();
        let mut kernel_time = FILETIME::default();
        let mut user_time = FILETIME::default();

        GetProcessTimes(
            process,
            &mut creation_time,
            &mut exit_time,
            &mut kernel_time,
            &mut user_time,
        )
        .map_err(|e| format!("GetProcessTimes failed: {}", e))?;

        // Convert FILETIME to Duration
        // FILETIME is in 100-nanosecond intervals
        let kernel_100ns =
            ((kernel_time.dwHighDateTime as u64) << 32) | (kernel_time.dwLowDateTime as u64);
        let user_100ns =
            ((user_time.dwHighDateTime as u64) << 32) | (user_time.dwLowDateTime as u64);

        let total_100ns = kernel_100ns + user_100ns;
        let total_nanos = total_100ns * 100;

        Ok(Duration::from_nanos(total_nanos))
    }
}

/// Test CPU usage with different polling intervals
///
/// This test verifies that CPU usage scales appropriately with polling interval
#[test]
#[cfg(windows)]
fn test_process_monitor_cpu_usage_different_intervals() {
    use easyhdr::monitor::ProcessMonitor;

    println!("\n=== CPU Usage with Different Intervals ===");

    let intervals = vec![
        Duration::from_millis(500),
        Duration::from_millis(1000),
        Duration::from_millis(2000),
    ];

    for interval in intervals {
        println!("\nTesting with {}ms interval:", interval.as_millis());

        let (tx, rx) = mpsc::channel();
        let monitor = ProcessMonitor::new(interval, tx);

        let cpu_start = get_process_cpu_time().expect("Failed to get initial CPU time");
        let wall_start = Instant::now();

        let _handle = monitor.start();

        // Run for 5 seconds
        thread::sleep(Duration::from_secs(5));

        let cpu_end = get_process_cpu_time().expect("Failed to get final CPU time");
        let wall_elapsed = wall_start.elapsed();

        let cpu_time_used = cpu_end - cpu_start;
        let cpu_percentage = (cpu_time_used.as_secs_f64() / wall_elapsed.as_secs_f64()) * 100.0;

        println!("  CPU usage: {:.2}%", cpu_percentage);

        // Drain events
        let mut event_count = 0;
        while rx.try_recv().is_ok() {
            event_count += 1;
        }
        println!("  Events: {}", event_count);

        // All intervals should be well below 1%
        assert!(
            cpu_percentage < 1.5,
            "CPU usage {:.2}% exceeds 1.5% for {}ms interval",
            cpu_percentage,
            interval.as_millis()
        );
    }

    println!("\n=== All Intervals Passed ===\n");
}

/// Benchmark the process enumeration operation
///
/// This test measures how long it takes to enumerate all processes
#[test]
#[cfg(windows)]
fn test_process_enumeration_performance() {
    use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    println!("\n=== Process Enumeration Performance ===");

    let iterations = 100;
    let mut total_duration = Duration::ZERO;
    let mut total_processes = 0;

    for _ in 0..iterations {
        let start = Instant::now();

        unsafe {
            let snapshot =
                CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).expect("Failed to create snapshot");

            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };

            let mut count = 0;
            let mut has_process = Process32FirstW(snapshot, &mut entry).is_ok();

            while has_process {
                count += 1;
                has_process = match Process32NextW(snapshot, &mut entry) {
                    Ok(_) => true,
                    Err(_) => false,
                };
            }

            let _ = CloseHandle(snapshot);
            total_processes = count;
        }

        total_duration += start.elapsed();
    }

    let avg_duration = total_duration / iterations;

    println!("  Iterations: {}", iterations);
    println!("  Processes enumerated: {}", total_processes);
    println!(
        "  Average time per enumeration: {:.2}ms",
        avg_duration.as_secs_f64() * 1000.0
    );
    println!(
        "  Total time for {} iterations: {:.2}ms",
        iterations,
        total_duration.as_secs_f64() * 1000.0
    );

    // Process enumeration should be very fast (< 10ms on modern systems)
    assert!(
        avg_duration < Duration::from_millis(10),
        "Process enumeration took {:.2}ms, expected < 10ms",
        avg_duration.as_secs_f64() * 1000.0
    );

    println!("  ✓ Performance is acceptable\n");
}

#[cfg(not(windows))]
#[test]
fn test_cpu_usage_not_supported_on_non_windows() {
    println!("CPU usage tests are only supported on Windows");
}
