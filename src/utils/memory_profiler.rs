//! Memory profiling utilities
//!
//! Tracks memory usage during application operation to ensure the 50MB RAM target is met.

use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{debug, info};

/// Memory statistics for the application
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// Total memory used by the process (in bytes)
    pub total_memory: usize,
    /// Memory used by icon cache (estimated, in bytes)
    pub icon_cache_memory: usize,
    /// Number of cached icons
    pub cached_icon_count: usize,
    /// Memory used by configuration (estimated, in bytes)
    pub config_memory: usize,
    /// Memory used by process monitoring (estimated, in bytes)
    pub monitor_memory: usize,
}

impl MemoryStats {
    /// Get total memory in megabytes
    #[expect(
        clippy::cast_precision_loss,
        reason = "Conversion to f64 for display purposes; precision loss is acceptable for human-readable memory values"
    )]
    pub fn total_mb(&self) -> f64 {
        self.total_memory as f64 / 1024.0 / 1024.0
    }

    /// Get icon cache memory in megabytes
    #[expect(
        clippy::cast_precision_loss,
        reason = "Conversion to f64 for display purposes; precision loss is acceptable for human-readable memory values"
    )]
    pub fn icon_cache_mb(&self) -> f64 {
        self.icon_cache_memory as f64 / 1024.0 / 1024.0
    }

    /// Check if memory usage is within acceptable limits (< 50MB)
    pub fn is_within_limits(&self) -> bool {
        self.total_mb() < 50.0
    }
}

/// Global memory profiler for tracking application memory usage
pub struct MemoryProfiler {
    /// Estimated icon cache size in bytes
    icon_cache_size: AtomicUsize,
    /// Number of cached icons
    icon_count: AtomicUsize,
}

impl MemoryProfiler {
    /// Create a new memory profiler
    pub fn new() -> Self {
        Self {
            icon_cache_size: AtomicUsize::new(0),
            icon_count: AtomicUsize::new(0),
        }
    }

    /// Record an icon being cached
    ///
    /// # Arguments
    ///
    /// * `icon_size` - Size of the icon data in bytes
    pub fn record_icon_cached(&self, icon_size: usize) {
        self.icon_cache_size.fetch_add(icon_size, Ordering::Relaxed);
        self.icon_count.fetch_add(1, Ordering::Relaxed);
        debug!(
            "Icon cached: {} bytes, total cache: {} bytes, count: {}",
            icon_size,
            self.icon_cache_size.load(Ordering::Relaxed),
            self.icon_count.load(Ordering::Relaxed)
        );
    }

    /// Record an icon being removed from cache
    ///
    /// # Arguments
    ///
    /// * `icon_size` - Size of the icon data in bytes
    pub fn record_icon_removed(&self, icon_size: usize) {
        self.icon_cache_size.fetch_sub(icon_size, Ordering::Relaxed);
        self.icon_count.fetch_sub(1, Ordering::Relaxed);
        debug!(
            "Icon removed: {} bytes, total cache: {} bytes, count: {}",
            icon_size,
            self.icon_cache_size.load(Ordering::Relaxed),
            self.icon_count.load(Ordering::Relaxed)
        );
    }

    /// Get current memory statistics from process memory, icon cache, and estimated config/monitor usage
    pub fn get_stats(&self) -> MemoryStats {
        let icon_cache_memory = self.icon_cache_size.load(Ordering::Relaxed);
        let cached_icon_count = self.icon_count.load(Ordering::Relaxed);

        // Get process memory usage (Windows-specific)
        let total_memory = Self::get_process_memory();

        // Estimate other memory usage
        // Config: ~1-2KB per app + base overhead
        let config_memory = 10 * 1024; // Estimated 10KB

        // Monitor: HashSet overhead + process names
        let monitor_memory = 5 * 1024; // Estimated 5KB

        MemoryStats {
            total_memory,
            icon_cache_memory,
            cached_icon_count,
            config_memory,
            monitor_memory,
        }
    }

    /// Get process memory usage in bytes (Windows only, returns 0 on other platforms)
    ///
    /// # Safety
    ///
    /// Uses Windows FFI (`GetProcessMemoryInfo`). Structure is correctly sized and aligned.
    #[cfg(windows)]
    #[expect(
        unsafe_code,
        reason = "Windows FFI for GetProcessMemoryInfo to retrieve process memory usage"
    )]
    fn get_process_memory() -> usize {
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows::Win32::System::Threading::GetCurrentProcess;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "size_of::<PROCESS_MEMORY_COUNTERS>() is a compile-time constant (72 bytes) well within u32::MAX"
        )]
        unsafe {
            let process = GetCurrentProcess();
            let mut pmc = PROCESS_MEMORY_COUNTERS {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };

            match GetProcessMemoryInfo(process, &raw mut pmc, pmc.cb) {
                Ok(()) => pmc.WorkingSetSize,
                Err(e) => {
                    tracing::warn!("Failed to get process memory info: {}", e);
                    0
                }
            }
        }
    }

    #[cfg(not(windows))]
    fn get_process_memory() -> usize {
        // Stub for non-Windows platforms
        0
    }

    /// Log current memory statistics
    pub fn log_stats(&self) {
        let stats = self.get_stats();
        info!(
            "Memory usage: {:.2} MB total, {:.2} MB icon cache ({} icons), {:.2} KB config, {:.2} KB monitor",
            stats.total_mb(),
            stats.icon_cache_mb(),
            stats.cached_icon_count,
            stats.config_memory / 1024,
            stats.monitor_memory / 1024
        );

        if !stats.is_within_limits() {
            tracing::warn!(
                "Memory usage ({:.2} MB) exceeds target limit of 50 MB!",
                stats.total_mb()
            );
        }
    }
}

impl Default for MemoryProfiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Global memory profiler instance
static MEMORY_PROFILER: std::sync::LazyLock<MemoryProfiler> =
    std::sync::LazyLock::new(MemoryProfiler::new);

/// Get the global memory profiler instance
pub fn get_profiler() -> &'static MemoryProfiler {
    &MEMORY_PROFILER
}

/// Records cached icon size in bytes (Windows only, no-op on other platforms)
#[inline]
pub fn record_icon_cached_safe(size: usize) {
    #[cfg(windows)]
    {
        get_profiler().record_icon_cached(size);
    }
    #[cfg(not(windows))]
    {
        let _ = size; // Suppress unused warning
    }
}

/// Records removed icon size in bytes (Windows only, no-op on other platforms)
#[inline]
pub fn record_icon_removed_safe(size: usize) {
    #[cfg(windows)]
    {
        get_profiler().record_icon_removed(size);
    }
    #[cfg(not(windows))]
    {
        let _ = size; // Suppress unused warning
    }
}
