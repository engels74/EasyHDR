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
use image::{ImageFormat, ImageReader, imageops::FilterType};
use std::io::{Cursor, Write};
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
        let cache_path = self.cache_path(app_id);

        // Requirement 2.5: Return Ok(None) if cache file does not exist
        if !cache_path.exists() {
            tracing::debug!("Cache miss for app {}: file does not exist", app_id);
            return Ok(None);
        }

        // Requirements 2.1, 2.2, 2.4: Cache validation for Win32 apps
        if let Some(source) = source_path {
            // Get cache file metadata
            let cache_metadata = std::fs::metadata(&cache_path).map_err(|source| {
                EasyHdrError::IconCache(IconCacheError::MetadataError {
                    path: cache_path.clone(),
                    source,
                })
            })?;

            // Get source file metadata
            let source_metadata = std::fs::metadata(source).map_err(|source_err| {
                EasyHdrError::IconCache(IconCacheError::MetadataError {
                    path: source.to_path_buf(),
                    source: source_err,
                })
            })?;

            // Compare modification times (Requirement 2.1)
            let cache_mtime = cache_metadata.modified().map_err(|cache_err| {
                EasyHdrError::IconCache(IconCacheError::MetadataError {
                    path: cache_path.clone(),
                    source: cache_err,
                })
            })?;

            let source_mtime = source_metadata.modified().map_err(|source_err| {
                EasyHdrError::IconCache(IconCacheError::MetadataError {
                    path: source.to_path_buf(),
                    source: source_err,
                })
            })?;

            // Requirement 2.2: Return Ok(None) if executable is newer than cache
            if source_mtime > cache_mtime {
                tracing::debug!(
                    "Cache miss for app {}: source file is newer (source: {:?}, cache: {:?})",
                    app_id,
                    source_mtime,
                    cache_mtime
                );
                return Ok(None);
            }
        } else {
            // Requirement 2.4: Skip validation for UWP apps (no source path)
            tracing::trace!("Loading cached icon for UWP app {} (no validation)", app_id);
        }

        // Read PNG file from disk
        let png_bytes = std::fs::read(&cache_path).map_err(|source| {
            EasyHdrError::IconCache(IconCacheError::CacheReadError {
                app_id,
                path: cache_path.clone(),
                source,
            })
        })?;

        // Decode PNG to RGBA (this already returns proper errors)
        let rgba_bytes = Self::decode_png_to_rgba(&png_bytes, app_id)?;

        tracing::debug!(
            "Loaded icon for app {} from cache ({} bytes PNG -> {} bytes RGBA)",
            app_id,
            png_bytes.len(),
            rgba_bytes.len()
        );

        Ok(Some(rgba_bytes))
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
        // Step 1 & 2: Validate size and encode RGBA to PNG
        // (Requirement 7.1: validation happens inside encode_rgba_to_png)
        let png_bytes = Self::encode_rgba_to_png(rgba_bytes, app_id)?;

        // Get the final cache file path
        let cache_file_path = self.cache_path(app_id);

        // Step 3: Use tempfile::NamedTempFile::persist() for atomic write
        // (Requirement 1.4, 7.3: Atomic writes via tempfile)
        //
        // Create temporary file in the same directory as the cache file.
        // This is critical for atomic rename to work correctly on Windows:
        // - Temp file and target must be on the same filesystem
        // - persist() uses MoveFileEx on Windows which is atomic
        let mut temp_file = tempfile::Builder::new()
            .prefix(&format!("{app_id}_"))
            .suffix(".tmp")
            .tempfile_in(&self.cache_dir)
            .map_err(|source| {
                EasyHdrError::IconCache(IconCacheError::TempFileCreationFailed { app_id, source })
            })?;

        // Write PNG data to temporary file
        temp_file.write_all(&png_bytes).map_err(|source| {
            EasyHdrError::IconCache(IconCacheError::CacheWriteError {
                app_id,
                path: cache_file_path.clone(),
                source,
            })
        })?;

        // Atomically persist the temporary file
        // This performs an atomic rename operation that either:
        // - Succeeds completely (file is visible with all data)
        // - Fails completely (no partial file visible)
        // This prevents corruption from crashes or power loss during write
        temp_file.persist(&cache_file_path).map_err(|e| {
            EasyHdrError::IconCache(IconCacheError::AtomicPersistFailed {
                app_id,
                path: cache_file_path.clone(),
                source: e,
            })
        })?;

        // Step 4: Log success with icon size
        tracing::debug!(
            "Saved icon for app {} to cache ({} bytes PNG, from {} bytes RGBA)",
            app_id,
            png_bytes.len(),
            rgba_bytes.len()
        );

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

    /// Encode RGBA data to PNG format
    ///
    /// Encodes raw RGBA pixel data (32x32 pixels) to PNG format for disk storage.
    /// Pre-allocates output buffer with 8192 bytes capacity for efficient encoding
    /// (Requirement 7.5: Pre-allocate PNG buffers).
    ///
    /// # Arguments
    ///
    /// * `rgba_bytes` - Raw RGBA pixel data (must be exactly 4096 bytes)
    /// * `app_id` - Application UUID for error context
    ///
    /// # Returns
    ///
    /// Returns `Ok(Vec<u8>)` containing PNG-encoded data on success.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError` if:
    /// - Input size is not exactly 4096 bytes (`InvalidIconSize`)
    /// - PNG encoding fails (`PngEncodingError`)
    ///
    /// # Requirements
    ///
    /// - Requirement 7.1: Validates input size is exactly 4096 bytes
    /// - Requirement 7.4: Returns structured error with app UUID context
    /// - Requirement 7.5: Pre-allocates with `Vec::with_capacity(8192)`
    ///
    /// # Design
    ///
    /// Pre-allocation of 8192 bytes is based on measured PNG sizes for 32x32 RGBA icons:
    /// - Typical compressed size: 2-6 KB
    /// - 8KB capacity avoids reallocation in most cases
    /// - Follows Rust guideline: "Pre-allocate (Vec::with_capacity)"
    fn encode_rgba_to_png(rgba_bytes: &[u8], app_id: Uuid) -> Result<Vec<u8>> {
        // Requirement 7.1: Validate input size is exactly 4096 bytes (32x32 × 4 channels)
        const EXPECTED_SIZE: usize = 32 * 32 * 4;
        if rgba_bytes.len() != EXPECTED_SIZE {
            return Err(EasyHdrError::IconCache(IconCacheError::InvalidIconSize {
                actual: rgba_bytes.len(),
            }));
        }

        // Requirement 7.5: Pre-allocate PNG buffer with 8192 bytes capacity
        // Based on measured PNG sizes: typically 2-6KB for 32x32 RGBA
        let mut png_bytes = Vec::with_capacity(8192);

        // Encode RGBA data to PNG format
        // Use Cursor for in-memory encoding
        image::write_buffer_with_format(
            &mut Cursor::new(&mut png_bytes),
            rgba_bytes,
            32,
            32,
            image::ExtendedColorType::Rgba8,
            ImageFormat::Png,
        )
        .map_err(|source| {
            // Requirement 7.4: Return structured error with app UUID context
            EasyHdrError::IconCache(IconCacheError::PngEncodingError { app_id, source })
        })?;

        Ok(png_bytes)
    }

    /// Decode PNG data to RGBA format
    ///
    /// Decodes PNG image data and resizes to exactly 32x32 pixels using Lanczos3
    /// resampling for high-quality results (Requirement 7.2).
    ///
    /// # Arguments
    ///
    /// * `png_bytes` - PNG-encoded image data
    /// * `app_id` - Application UUID for error context
    ///
    /// # Returns
    ///
    /// Returns `Ok(Vec<u8>)` containing exactly 4096 bytes of RGBA data (32x32 pixels).
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError` if:
    /// - PNG decoding fails (`PngDecodingError`)
    /// - Image cannot be resized
    ///
    /// # Requirements
    ///
    /// - Requirement 7.2: Resizes images to exactly 32x32 pixels
    /// - Requirement 7.4: Returns structured error with app UUID context
    ///
    /// # Design
    ///
    /// Uses Lanczos3 resampling filter for high-quality downscaling:
    /// - Preserves sharp edges better than bilinear
    /// - Reduces aliasing artifacts
    /// - Industry standard for icon resampling
    fn decode_png_to_rgba(png_bytes: &[u8], app_id: Uuid) -> Result<Vec<u8>> {
        // Decode PNG from memory buffer
        let img = ImageReader::new(Cursor::new(png_bytes))
            .with_guessed_format()
            .map_err(|source| {
                // Requirement 7.4: Return structured error with app UUID context
                EasyHdrError::IconCache(IconCacheError::PngDecodingError {
                    app_id,
                    source: image::ImageError::IoError(source),
                })
            })?
            .decode()
            .map_err(|source| {
                // Requirement 7.4: Return structured error with app UUID context
                EasyHdrError::IconCache(IconCacheError::PngDecodingError { app_id, source })
            })?;

        // Requirement 7.2: Resize to exactly 32x32 pixels using Lanczos3 filter
        // Lanczos3 provides high-quality resampling with sharp edges
        let resized = img.resize_exact(32, 32, FilterType::Lanczos3);

        // Convert to RGBA8 format and extract raw bytes
        let rgba_img = resized.to_rgba8();
        let rgba_bytes = rgba_img.into_raw();

        // Verify output size (should always be 4096 bytes)
        debug_assert_eq!(
            rgba_bytes.len(),
            4096,
            "PNG decode produced unexpected size"
        );

        Ok(rgba_bytes)
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

    // PNG encoding/decoding tests

    #[test]
    fn encode_rgba_to_png_validates_input_size() {
        let app_id = Uuid::new_v4();

        // Test with invalid size (too small)
        let invalid_small = vec![0u8; 100];
        let result = IconCache::encode_rgba_to_png(&invalid_small, app_id);
        assert!(result.is_err());
        match result {
            Err(EasyHdrError::IconCache(IconCacheError::InvalidIconSize { actual })) => {
                assert_eq!(actual, 100);
            }
            _ => panic!("Expected InvalidIconSize error"),
        }

        // Test with invalid size (too large)
        let invalid_large = vec![0u8; 5000];
        let result = IconCache::encode_rgba_to_png(&invalid_large, app_id);
        assert!(result.is_err());
        match result {
            Err(EasyHdrError::IconCache(IconCacheError::InvalidIconSize { actual })) => {
                assert_eq!(actual, 5000);
            }
            _ => panic!("Expected InvalidIconSize error"),
        }
    }

    #[test]
    fn encode_rgba_to_png_accepts_valid_size() {
        let app_id = Uuid::new_v4();

        // Test with valid size (4096 bytes = 32x32 RGBA)
        let valid_rgba = vec![128u8; 4096];
        let result = IconCache::encode_rgba_to_png(&valid_rgba, app_id);
        assert!(result.is_ok(), "Valid size should succeed");

        let png_bytes = result.unwrap();
        assert!(!png_bytes.is_empty(), "PNG data should not be empty");
        assert!(
            png_bytes.len() < 8192,
            "PNG should be smaller than pre-allocated capacity"
        );
    }

    #[test]
    fn png_encoding_decoding_roundtrip() {
        let app_id = Uuid::new_v4();

        // Create test RGBA data with a pattern
        let mut rgba_data = vec![0u8; 4096];
        for i in 0..4096 {
            rgba_data[i] = (i % 256) as u8;
        }

        // Encode to PNG
        let png_bytes =
            IconCache::encode_rgba_to_png(&rgba_data, app_id).expect("Encoding should succeed");

        // Decode back to RGBA
        let decoded_rgba =
            IconCache::decode_png_to_rgba(&png_bytes, app_id).expect("Decoding should succeed");

        // Verify size
        assert_eq!(
            decoded_rgba.len(),
            4096,
            "Decoded data should be exactly 4096 bytes"
        );

        // Verify roundtrip preserves data
        assert_eq!(
            rgba_data, decoded_rgba,
            "Roundtrip should preserve RGBA data exactly"
        );
    }

    #[test]
    fn decode_png_to_rgba_produces_correct_size() {
        let app_id = Uuid::new_v4();

        // Create a simple RGBA image
        let rgba_data = vec![255u8; 4096]; // All white pixels

        // Encode to PNG
        let png_bytes =
            IconCache::encode_rgba_to_png(&rgba_data, app_id).expect("Encoding should succeed");

        // Decode
        let decoded =
            IconCache::decode_png_to_rgba(&png_bytes, app_id).expect("Decoding should succeed");

        // Verify exact size (32x32 RGBA = 4096 bytes)
        assert_eq!(
            decoded.len(),
            4096,
            "Decoded data must be exactly 4096 bytes"
        );
    }

    #[test]
    fn decode_png_handles_invalid_data() {
        let app_id = Uuid::new_v4();

        // Test with invalid PNG data
        let invalid_png = vec![0u8; 100];
        let result = IconCache::decode_png_to_rgba(&invalid_png, app_id);

        assert!(result.is_err(), "Invalid PNG data should produce error");
        match result {
            Err(EasyHdrError::IconCache(IconCacheError::PngDecodingError {
                app_id: error_id,
                ..
            })) => {
                assert_eq!(error_id, app_id, "Error should include correct app UUID");
            }
            _ => panic!("Expected PngDecodingError"),
        }
    }

    #[test]
    fn png_encoding_produces_valid_png() {
        let app_id = Uuid::new_v4();

        // Create test data
        let rgba_data = vec![200u8; 4096];

        // Encode
        let png_bytes =
            IconCache::encode_rgba_to_png(&rgba_data, app_id).expect("Encoding should succeed");

        // Verify PNG signature (first 8 bytes)
        // PNG files start with: 137 80 78 71 13 10 26 10
        assert!(
            png_bytes.len() >= 8,
            "PNG should have at least header bytes"
        );
        assert_eq!(png_bytes[0], 137, "PNG signature byte 0");
        assert_eq!(png_bytes[1], 80, "PNG signature byte 1 (P)");
        assert_eq!(png_bytes[2], 78, "PNG signature byte 2 (N)");
        assert_eq!(png_bytes[3], 71, "PNG signature byte 3 (G)");
    }

    // save_icon() tests

    #[test]
    fn save_icon_validates_input_size() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Test with invalid size
        let invalid_rgba = vec![0u8; 100];
        let result = cache.save_icon(app_id, &invalid_rgba);

        assert!(result.is_err(), "Invalid size should produce error");
        match result {
            Err(EasyHdrError::IconCache(IconCacheError::InvalidIconSize { actual })) => {
                assert_eq!(actual, 100);
            }
            _ => panic!("Expected InvalidIconSize error"),
        }
    }

    #[test]
    fn save_icon_creates_file_with_correct_name() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Create valid RGBA data
        let rgba_data = vec![128u8; 4096];

        // Save icon
        cache
            .save_icon(app_id, &rgba_data)
            .expect("save_icon should succeed");

        // Verify file exists with correct name
        let expected_path = temp_dir.path().join(format!("{app_id}.png"));
        assert!(
            expected_path.exists(),
            "Icon file should exist at expected path"
        );
    }

    #[test]
    fn save_icon_overwrites_existing_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Save first icon
        let rgba_data_1 = vec![100u8; 4096];
        cache
            .save_icon(app_id, &rgba_data_1)
            .expect("First save should succeed");

        // Save second icon with different data
        let rgba_data_2 = vec![200u8; 4096];
        cache
            .save_icon(app_id, &rgba_data_2)
            .expect("Second save should succeed");

        // Verify file exists (atomic persist should have overwritten)
        let expected_path = temp_dir.path().join(format!("{app_id}.png"));
        assert!(
            expected_path.exists(),
            "Icon file should exist after overwrite"
        );
    }

    #[test]
    fn save_icon_produces_valid_png_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Create RGBA data with a pattern
        let mut rgba_data = vec![0u8; 4096];
        for i in 0..4096 {
            rgba_data[i] = (i % 256) as u8;
        }

        // Save icon
        cache
            .save_icon(app_id, &rgba_data)
            .expect("save_icon should succeed");

        // Read the file directly and verify PNG signature
        let file_path = temp_dir.path().join(format!("{app_id}.png"));
        let file_data = std::fs::read(&file_path).expect("Failed to read icon file");

        // Verify PNG signature
        assert!(file_data.len() >= 8, "PNG should have at least header");
        assert_eq!(file_data[0], 137, "PNG signature byte 0");
        assert_eq!(file_data[1], 80, "PNG signature byte 1 (P)");
        assert_eq!(file_data[2], 78, "PNG signature byte 2 (N)");
        assert_eq!(file_data[3], 71, "PNG signature byte 3 (G)");
    }

    #[test]
    fn save_icon_atomic_write_no_partial_files() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Save icon
        let rgba_data = vec![255u8; 4096];
        cache
            .save_icon(app_id, &rgba_data)
            .expect("save_icon should succeed");

        // Verify no temporary files remain in cache directory
        let entries: Vec<_> = std::fs::read_dir(temp_dir.path())
            .expect("Failed to read cache dir")
            .collect();

        // Should only have one file (the .png file)
        assert_eq!(
            entries.len(),
            1,
            "Should only have the PNG file, no temp files"
        );

        // Verify it's the correct file
        let entry = entries[0].as_ref().expect("Failed to get entry");
        let file_name = entry.file_name();
        let expected_name = format!("{app_id}.png");
        assert_eq!(
            file_name.to_str(),
            Some(expected_name.as_str()),
            "Should be the PNG file"
        );
    }

    // load_icon() tests

    #[test]
    fn load_icon_cache_hit() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Create test RGBA data with a pattern
        let mut rgba_data = vec![0u8; 4096];
        for i in 0..4096 {
            rgba_data[i] = (i % 256) as u8;
        }

        // Save icon first
        cache
            .save_icon(app_id, &rgba_data)
            .expect("save_icon should succeed");

        // Load icon (no source path = UWP app, no validation)
        let loaded = cache
            .load_icon(app_id, None)
            .expect("load_icon should succeed")
            .expect("Should have loaded icon data");

        // Verify roundtrip preserves data
        assert_eq!(rgba_data, loaded, "Loaded icon should match original data");
    }

    #[test]
    fn load_icon_cache_miss_file_not_found() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Load icon that doesn't exist
        let result = cache
            .load_icon(app_id, None)
            .expect("load_icon should succeed (Ok(None))");

        // Should return None (cache miss)
        assert!(
            result.is_none(),
            "Should return None for non-existent cache file"
        );
    }

    #[test]
    fn load_icon_stale_cache_win32_app() {
        use std::fs::File;
        use std::io::Write;
        use std::thread;
        use std::time::Duration;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Create a fake source EXE file
        let source_path = temp_dir.path().join("test.exe");
        let mut source_file = File::create(&source_path).expect("Failed to create source file");
        source_file
            .write_all(b"fake exe")
            .expect("Failed to write source file");
        drop(source_file); // Close file

        // Wait a moment to ensure time difference
        thread::sleep(Duration::from_millis(10));

        // Save icon
        let rgba_data = vec![128u8; 4096];
        cache
            .save_icon(app_id, &rgba_data)
            .expect("save_icon should succeed");

        // Wait a moment to ensure time difference
        thread::sleep(Duration::from_millis(10));

        // Update source file modification time (simulate EXE update)
        let mut source_file = File::create(&source_path).expect("Failed to update source file");
        source_file
            .write_all(b"updated exe")
            .expect("Failed to write source file");
        drop(source_file); // Close file

        // Load icon with source path validation
        let result = cache
            .load_icon(app_id, Some(&source_path))
            .expect("load_icon should succeed (Ok(None))");

        // Should return None (stale cache)
        assert!(
            result.is_none(),
            "Should return None when source file is newer than cache"
        );
    }

    #[test]
    fn load_icon_fresh_cache_win32_app() {
        use std::fs::File;
        use std::io::Write;
        use std::thread;
        use std::time::Duration;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Create a fake source EXE file
        let source_path = temp_dir.path().join("test.exe");
        let mut source_file = File::create(&source_path).expect("Failed to create source file");
        source_file
            .write_all(b"fake exe")
            .expect("Failed to write source file");
        drop(source_file); // Close file

        // Wait a moment to ensure time difference
        thread::sleep(Duration::from_millis(10));

        // Save icon (cache will be newer than source)
        let rgba_data = vec![128u8; 4096];
        cache
            .save_icon(app_id, &rgba_data)
            .expect("save_icon should succeed");

        // Load icon with source path validation
        let loaded = cache
            .load_icon(app_id, Some(&source_path))
            .expect("load_icon should succeed")
            .expect("Should have loaded icon data (cache is fresh)");

        // Verify data matches
        assert_eq!(rgba_data, loaded, "Loaded icon should match original data");
    }

    #[test]
    fn load_icon_uwp_app_no_validation() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Create test RGBA data
        let rgba_data = vec![200u8; 4096];

        // Save icon
        cache
            .save_icon(app_id, &rgba_data)
            .expect("save_icon should succeed");

        // Load icon without source path (UWP app - no validation)
        let loaded = cache
            .load_icon(app_id, None)
            .expect("load_icon should succeed")
            .expect("Should have loaded icon data");

        // Verify data matches
        assert_eq!(rgba_data, loaded, "Loaded icon should match original data");
    }

    #[test]
    fn load_icon_corrupted_png() {
        use std::fs::File;
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Create a corrupted PNG file
        let cache_path = temp_dir.path().join(format!("{app_id}.png"));
        let mut file = File::create(&cache_path).expect("Failed to create cache file");
        file.write_all(b"not a valid PNG file")
            .expect("Failed to write corrupted data");
        drop(file);

        // Load icon - should return error for corrupted PNG
        let result = cache.load_icon(app_id, None);

        assert!(result.is_err(), "Should return error for corrupted PNG");
        match result {
            Err(EasyHdrError::IconCache(IconCacheError::PngDecodingError {
                app_id: error_id,
                ..
            })) => {
                assert_eq!(error_id, app_id, "Error should include correct app UUID");
            }
            _ => panic!("Expected PngDecodingError"),
        }
    }

    #[test]
    fn load_icon_save_load_roundtrip() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Create test data with varying pattern
        let mut rgba_data = vec![0u8; 4096];
        for i in 0..4096 {
            rgba_data[i] = ((i * 7 + 13) % 256) as u8; // More complex pattern
        }

        // Save icon
        cache
            .save_icon(app_id, &rgba_data)
            .expect("save_icon should succeed");

        // Load icon
        let loaded = cache
            .load_icon(app_id, None)
            .expect("load_icon should succeed")
            .expect("Should have loaded icon data");

        // Verify exact roundtrip
        assert_eq!(
            rgba_data.len(),
            loaded.len(),
            "Loaded data should have same length"
        );
        assert_eq!(rgba_data, loaded, "Roundtrip should preserve data exactly");
    }
}
