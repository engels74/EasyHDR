#![expect(
    clippy::unwrap_used,
    reason = "Test utilities use .unwrap() for brevity"
)]

//! Shared test utilities for `EasyHDR` unit tests.
//!
//! This module provides common test infrastructure used across multiple test modules.
//! It is only compiled during testing (`#[cfg(test)]`).

use std::sync::Mutex;
use tempfile::TempDir;

/// Global mutex to serialize tests that modify the APPDATA environment variable.
/// This prevents race conditions when multiple tests run in parallel and try to
/// set different APPDATA values.
static APPDATA_LOCK: Mutex<()> = Mutex::new(());

/// Helper function to create a temporary test directory using tempfile.
/// Returns a `TempDir` that automatically cleans up when dropped.
pub fn create_test_dir() -> TempDir {
    tempfile::tempdir().expect("Failed to create temp directory")
}

/// RAII guard that sets the APPDATA environment variable for a test scope
/// and restores the original value when dropped.
///
/// # Safety Considerations
///
/// This guard uses `std::env::set_var` and `std::env::remove_var`, which are marked
/// unsafe because they can cause data races when other threads are reading environment
/// variables concurrently.
///
/// **Safety Invariants:**
/// 1. Each test gets its own unique `TempDir`, so parallel tests write to different paths
/// 2. The guard is RAII-based and restores the original value on drop, preventing
///    environment pollution between tests
/// 3. The `APPDATA_LOCK` mutex ensures tests modify APPDATA serially, not concurrently
/// 4. Each test runs in its own thread with isolated stack frame
///
/// **Why this is safe in parallel test execution:**
/// - While `std::env::set_var` is unsafe, the actual risk is when threads read env vars
///   while another thread modifies them
/// - Each test function runs in its own thread with its own stack frame
/// - The guard ensures cleanup even on panic via Drop
/// - The modification is scoped to the test function's lifetime
/// - Tests can safely run in parallel (`cargo test --lib`) without `--test-threads=1`
///
/// **Note:** While these tests CAN run in parallel, they can also run single-threaded
/// if needed for other reasons (e.g., debugging, Miri analysis).
pub struct AppdataGuard {
    original: Option<String>,
    // Lock guard must be held for the lifetime of this struct to ensure exclusive
    // access to APPDATA environment variable across parallel tests
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[expect(
    unsafe_code,
    reason = "Test-only code that modifies environment variables with documented safety invariants. Safe in parallel test execution."
)]
impl AppdataGuard {
    /// Create a new guard that sets APPDATA to the given temp directory path.
    pub fn new(temp_dir: &TempDir) -> Self {
        // Acquire lock to serialize APPDATA modifications across parallel tests
        let lock = APPDATA_LOCK.lock().unwrap();

        let original = std::env::var("APPDATA").ok();
        // SAFETY: This is safe because:
        // 1. Each test gets its own unique TempDir path (no shared state between tests)
        // 2. The guard is RAII-based and restores the original value on drop
        // 3. The APPDATA_LOCK mutex ensures tests modify APPDATA serially, not concurrently
        // 4. Each test runs in its own thread with isolated stack frame
        // See struct-level documentation for full safety invariants.
        unsafe {
            std::env::set_var("APPDATA", temp_dir.path());
        }
        Self {
            original,
            _lock: lock,
        }
    }
}

#[expect(
    unsafe_code,
    reason = "Test-only code that restores environment variables with documented safety invariants. Safe in parallel test execution."
)]
impl Drop for AppdataGuard {
    fn drop(&mut self) {
        // SAFETY: This is safe because:
        // 1. Each test has its own guard instance (no shared state)
        // 2. We're restoring the original state, preventing test pollution
        // 3. No other threads are accessing environment variables within this test
        // 4. Drop runs in the same thread that created the guard
        // See struct-level documentation for full safety invariants.
        if let Some(ref original) = self.original {
            unsafe {
                std::env::set_var("APPDATA", original);
            }
        } else {
            unsafe {
                std::env::remove_var("APPDATA");
            }
        }
    }
}
