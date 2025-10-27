//! Icon cache module for persistent icon storage
//!
//! This module provides disk-based icon caching with PNG encoding/decoding,
//! cache validation via file modification times, and parallel loading support.
//!
//! # Design Principles
//!
//! - **Thread Safety**: `IconCache` is `Send + Sync` for concurrent access (Requirement 6.1)
//! - **Immutable API**: All methods use `&self` to enable lock-free concurrent reads (Requirement 9.2)
//! - **Graceful Degradation**: Cache failures never prevent application operation (Requirement 5.2)
//! - **Structured Errors**: Uses `IconCacheError` with `thiserror` for matchable errors (Requirement 5.1)
//!
//! # Architecture
//!
//! Icons are stored as PNG files in `%APPDATA%\EasyHDR\icon_cache\{uuid}.png`.
//! Each icon is 32x32 pixels in RGBA format (4096 bytes uncompressed).
//! Cache validation uses file modification time comparison for Win32 apps.
//!
//! # Example
//!
//! ```no_run
//! use easyhdr::utils::icon_cache::IconCache;
//! use uuid::Uuid;
//!
//! let cache = IconCache::new(IconCache::default_cache_dir())?;
//! let app_id = Uuid::new_v4();
//! let rgba_data = vec![0u8; 4096]; // 32x32 RGBA
//!
//! // Save icon
//! cache.save_icon(app_id, &rgba_data)?;
//!
//! // Load icon (with validation)
//! let loaded = cache.load_icon(app_id, None)?;
//! assert!(loaded.is_some());
//! # Ok::<(), easyhdr::error::EasyHdrError>(())
//! ```

use crate::error::{EasyHdrError, IconCacheError};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Result type alias for icon cache operations
pub type Result<T> = std::result::Result<T, EasyHdrError>;

/// Icon cache manager for persistent icon storage
///
/// Manages a disk-based cache of application icons stored as PNG files.
/// Thread-safe (`Send + Sync`) for concurrent access from multiple threads.
///
/// # Thread Safety
///
/// - All methods use `&self` (immutable references) for lock-free concurrent reads
/// - Concurrent writes use unique file paths (UUID-based) to avoid conflicts
/// - Atomic writes via `tempfile::NamedTempFile::persist()` prevent partial writes
///
/// # Requirements
///
/// - Requirement 6.1: Marked as `Send + Sync` for safe concurrent access
/// - Requirement 9.2: All methods use immutable `&self` references
/// - Requirement 9.4: Implements `Debug` trait for diagnostics
#[derive(Debug)]
pub struct IconCache {
    /// Cache directory path (typically `%APPDATA%\EasyHDR\icon_cache`)
    cache_dir: PathBuf,
}

impl IconCache {
    /// Create a new icon cache manager
    ///
    /// Creates the cache directory if it does not exist. Accepts flexible path types
    /// via `impl Into<PathBuf>` for ergonomic API design (Rust guideline: API Design).
    ///
    /// # Arguments
    ///
    /// * `cache_dir` - Directory path for icon cache storage
    ///
    /// # Returns
    ///
    /// Returns `Ok(IconCache)` on success, or `Err` if directory creation fails.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError::CacheDirectoryCreationFailed` if the cache directory
    /// cannot be created (e.g., permission denied, disk full).
    ///
    /// # Requirements
    ///
    /// - Requirement 1.1: Creates cache directory if it does not exist
    /// - Requirement 5.1: Returns structured error on failure
    /// - Requirement 9.1: Accepts `impl Into<PathBuf>` for flexibility
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::utils::icon_cache::IconCache;
    ///
    /// let cache = IconCache::new("/path/to/cache")?;
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    pub fn new(cache_dir: impl Into<PathBuf>) -> Result<Self> {
        let cache_dir = cache_dir.into();

        // Create cache directory if it doesn't exist (Requirement 1.1)
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir).map_err(|source| {
                EasyHdrError::IconCache(IconCacheError::CacheDirectoryCreationFailed {
                    path: cache_dir.clone(),
                    source,
                })
            })?;
        }

        Ok(Self { cache_dir })
    }

    /// Get the default cache directory path
    ///
    /// Returns `%APPDATA%\EasyHDR\icon_cache` on Windows.
    ///
    /// # Returns
    ///
    /// Returns the default cache directory path.
    ///
    /// # Panics
    ///
    /// Panics if `%APPDATA%` environment variable is not set (should never happen on Windows).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::utils::icon_cache::IconCache;
    ///
    /// let cache_dir = IconCache::default_cache_dir();
    /// println!("Cache directory: {}", cache_dir.display());
    /// ```
    pub fn default_cache_dir() -> PathBuf {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        let mut path = PathBuf::from(appdata);
        path.push("EasyHDR");
        path.push("icon_cache");
        path
    }

    /// Load an icon from cache with validation
    ///
    /// Loads a cached icon for the specified application. For Win32 apps, validates
    /// cache freshness by comparing file modification times. Returns `Ok(None)` on
    /// cache miss or stale cache.
    ///
    /// # Arguments
    ///
    /// * `app_id` - Unique identifier for the application
    /// * `source_path` - Optional source file path for cache validation (Win32 apps only)
    ///
    /// # Returns
    ///
    /// - `Ok(Some(Vec<u8>))` - Icon data loaded successfully (32x32 RGBA, 4096 bytes)
    /// - `Ok(None)` - Cache miss or stale cache (re-extraction needed)
    /// - `Err` - I/O error or PNG decoding error
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError` if:
    /// - Cache file cannot be read (I/O error)
    /// - PNG decoding fails (corrupted cache file)
    /// - File metadata cannot be accessed
    ///
    /// # Requirements
    ///
    /// - Requirement 2.1: Compares cache mtime with executable mtime
    /// - Requirement 2.2: Returns Ok(None) if executable is newer
    /// - Requirement 2.4: Skips validation for UWP apps (no source path)
    /// - Requirement 2.5: Returns Ok(None) if cache file does not exist
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::utils::icon_cache::IconCache;
    /// use uuid::Uuid;
    /// use std::path::Path;
    ///
    /// let cache = IconCache::new(IconCache::default_cache_dir())?;
    /// let app_id = Uuid::new_v4();
    /// let exe_path = Path::new("C:\\Program Files\\App\\app.exe");
    ///
    /// match cache.load_icon(app_id, Some(exe_path))? {
    ///     Some(icon_data) => println!("Icon loaded from cache"),
    ///     None => println!("Cache miss, need to extract icon"),
    /// }
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    pub fn load_icon(&self, app_id: Uuid, source_path: Option<&Path>) -> Result<Option<Vec<u8>>> {
        // Stub implementation - will be completed in task 2.4
        let _ = (app_id, source_path);
        Ok(None)
    }

    /// Save an icon to cache with atomic write
    ///
    /// Encodes RGBA data to PNG format and saves to cache using atomic write operations.
    /// The icon data must be exactly 4096 bytes (32x32 pixels × 4 channels).
    ///
    /// # Arguments
    ///
    /// * `app_id` - Unique identifier for the application
    /// * `rgba_bytes` - RGBA pixel data (must be exactly 4096 bytes)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or `Err` if validation or write fails.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError` if:
    /// - Input size is not 4096 bytes (`InvalidIconSize`)
    /// - PNG encoding fails (`PngEncodingError`)
    /// - Temporary file creation fails (`TempFileCreationFailed`)
    /// - Atomic persist fails (`AtomicPersistFailed`)
    ///
    /// # Requirements
    ///
    /// - Requirement 1.2, 1.3: Encodes RGBA to PNG and saves to cache
    /// - Requirement 1.4: Uses atomic write operations
    /// - Requirement 7.1: Validates input size is exactly 4096 bytes
    /// - Requirement 7.3: Uses `tempfile::NamedTempFile::persist()` for atomic writes
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::utils::icon_cache::IconCache;
    /// use uuid::Uuid;
    ///
    /// let cache = IconCache::new(IconCache::default_cache_dir())?;
    /// let app_id = Uuid::new_v4();
    /// let rgba_data = vec![0u8; 4096]; // 32x32 RGBA
    ///
    /// cache.save_icon(app_id, &rgba_data)?;
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    pub fn save_icon(&self, app_id: Uuid, rgba_bytes: &[u8]) -> Result<()> {
        // Stub implementation - will be completed in task 2.3
        let _ = (app_id, rgba_bytes);
        Ok(())
    }

    /// Remove a single icon from cache
    ///
    /// Deletes the cached icon file for the specified application.
    ///
    /// # Arguments
    ///
    /// * `app_id` - Unique identifier for the application
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or `Err` if deletion fails.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError::IconRemovalFailed` if the file cannot be deleted.
    /// Returns `Ok(())` if the file does not exist (idempotent operation).
    ///
    /// # Requirements
    ///
    /// - Requirement 4.4: Deletes the corresponding cached icon file
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::utils::icon_cache::IconCache;
    /// use uuid::Uuid;
    ///
    /// let cache = IconCache::new(IconCache::default_cache_dir())?;
    /// let app_id = Uuid::new_v4();
    ///
    /// cache.remove_icon(app_id)?;
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    pub fn remove_icon(&self, app_id: Uuid) -> Result<()> {
        // Stub implementation - will be completed in task 2.5
        let _ = app_id;
        Ok(())
    }

    /// Clear entire cache directory
    ///
    /// Removes all PNG files from the cache directory. The directory itself is not deleted.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or `Err` if directory traversal or deletion fails.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError::CacheClearFailed` if:
    /// - Directory cannot be read
    /// - Any file cannot be deleted
    ///
    /// # Requirements
    ///
    /// - Requirement 4.2, 4.3: Removes all PNG files from cache directory
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::utils::icon_cache::IconCache;
    ///
    /// let cache = IconCache::new(IconCache::default_cache_dir())?;
    /// cache.clear_cache()?;
    /// println!("Cache cleared successfully");
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    pub fn clear_cache(&self) -> Result<()> {
        // Stub implementation - will be completed in task 2.5
        Ok(())
    }

    /// Get cache statistics
    ///
    /// Calculates the number of cached icons and total size in bytes.
    ///
    /// # Returns
    ///
    /// Returns `Ok(CacheStats)` with icon count and total size, or `Err` if traversal fails.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError::CacheStatsFailed` if the cache directory cannot be read.
    ///
    /// # Requirements
    ///
    /// - Requirement 4.1: Retrieves cache statistics (count and total size)
    /// - Requirement 9.3: Returns concrete type (not `impl Trait`)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::utils::icon_cache::IconCache;
    ///
    /// let cache = IconCache::new(IconCache::default_cache_dir())?;
    /// let stats = cache.get_cache_stats()?;
    /// println!("Cached icons: {} ({})", stats.count, stats.size_human_readable());
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    pub fn get_cache_stats(&self) -> Result<CacheStats> {
        // Stub implementation - will be completed in task 2.5
        Ok(CacheStats {
            count: 0,
            size_bytes: 0,
        })
    }

    /// Get the cache file path for an application
    ///
    /// Returns the full path to the cached icon file for the specified application.
    /// This is a helper method used internally by other cache operations.
    ///
    /// # Arguments
    ///
    /// * `app_id` - Unique identifier for the application
    ///
    /// # Returns
    ///
    /// Returns the cache file path (`{cache_dir}/{uuid}.png`).
    fn cache_path(&self, app_id: Uuid) -> PathBuf {
        self.cache_dir.join(format!("{app_id}.png"))
    }
}

/// Cache statistics
///
/// Contains metadata about the icon cache including count and total size.
///
/// # Requirements
///
/// - Requirement 9.3: Concrete return type for cache statistics
/// - Requirement 9.4: Implements `Debug` trait
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// Number of cached icons
    pub count: usize,
    /// Total size of all cached icons in bytes
    pub size_bytes: u64,
}

impl CacheStats {
    /// Format size as human-readable string
    ///
    /// Converts byte size to KB or MB format for display in UI.
    ///
    /// # Returns
    ///
    /// Returns a formatted string like "42 KB" or "1.5 MB".
    ///
    /// # Requirements
    ///
    /// - Requirement 4.5: Display human-readable size in UI
    ///
    /// # Example
    ///
    /// ```
    /// use easyhdr::utils::icon_cache::CacheStats;
    ///
    /// let stats = CacheStats { count: 10, size_bytes: 40960 };
    /// assert_eq!(stats.size_human_readable(), "40 KB");
    ///
    /// let stats = CacheStats { count: 100, size_bytes: 2_097_152 };
    /// assert_eq!(stats.size_human_readable(), "2.0 MB");
    /// ```
    pub fn size_human_readable(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;

        if self.size_bytes >= MB {
            format!("{:.1} MB", self.size_bytes as f64 / MB as f64)
        } else if self.size_bytes >= KB {
            format!("{} KB", self.size_bytes / KB)
        } else {
            format!("{} bytes", self.size_bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time assertion that `IconCache` is `Send + Sync`
    ///
    /// This test ensures thread safety requirements are met at compile time.
    /// If `IconCache` ever becomes non-Send or non-Sync, this test will fail
    /// to compile, preventing regressions.
    ///
    /// # Requirements
    ///
    /// - Requirement 6.1: `IconCache` must be marked as `Send + Sync`
    /// - Requirement 6.6: Include compile-time thread safety assertions
    #[test]
    fn icon_cache_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<IconCache>();
        assert_sync::<IconCache>();
    }

    #[test]
    fn cache_stats_size_human_readable_bytes() {
        let stats = CacheStats {
            count: 1,
            size_bytes: 512,
        };
        assert_eq!(stats.size_human_readable(), "512 bytes");
    }

    #[test]
    fn cache_stats_size_human_readable_kb() {
        let stats = CacheStats {
            count: 10,
            size_bytes: 40960,
        };
        assert_eq!(stats.size_human_readable(), "40 KB");
    }

    #[test]
    fn cache_stats_size_human_readable_mb() {
        let stats = CacheStats {
            count: 100,
            size_bytes: 2_097_152,
        };
        assert_eq!(stats.size_human_readable(), "2.0 MB");
    }

    #[test]
    fn cache_stats_is_copy() {
        let stats = CacheStats {
            count: 5,
            size_bytes: 1024,
        };
        let _copy = stats; // Should be Copy, not Move
        let _another_copy = stats; // This should also work
    }

    #[test]
    fn cache_stats_is_debug() {
        let stats = CacheStats {
            count: 5,
            size_bytes: 1024,
        };
        let debug_str = format!("{stats:?}");
        assert!(debug_str.contains("CacheStats"));
        assert!(debug_str.contains("count"));
        assert!(debug_str.contains("size_bytes"));
    }

    #[test]
    fn icon_cache_is_debug() {
        let temp_dir = std::env::temp_dir();
        let cache = IconCache::new(temp_dir).unwrap();
        let debug_str = format!("{cache:?}");
        assert!(debug_str.contains("IconCache"));
        assert!(debug_str.contains("cache_dir"));
    }
}
