//! Icon cache module for persistent icon storage
//!
//! This module provides disk-based icon caching with PNG encoding/decoding,
//! cache validation via file modification times, and parallel loading support.
//!
//! # Design Principles
//!
//! - **Thread Safety**: `IconCache` is `Send + Sync` for concurrent access
//! - **Immutable API**: All methods use `&self` to enable lock-free concurrent reads
//! - **Graceful Degradation**: Cache failures never prevent application operation
//! - **Structured Errors**: Uses `IconCacheError` with `thiserror` for matchable errors
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

#[derive(Debug)]
pub struct IconCache {
    /// Cache directory path (typically `%APPDATA%\EasyHDR\icon_cache`)
    cache_dir: PathBuf,
}

impl IconCache {
    /// Create a new icon cache manager
    ///
    /// Creates the cache directory if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError::CacheDirectoryCreationFailed` if the cache directory
    /// cannot be created (e.g., permission denied, disk full).
    pub fn new(cache_dir: impl Into<PathBuf>) -> Result<Self> {
        let cache_dir = cache_dir.into();

        // Create cache directory if it doesn't exist
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

    /// Get the default cache directory path (`%APPDATA%\EasyHDR\icon_cache` on Windows)
    ///
    /// # Panics
    ///
    /// Panics if `%APPDATA%` environment variable is not set.
    pub fn default_cache_dir() -> PathBuf {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        let mut path = PathBuf::from(appdata);
        path.push("EasyHDR");
        path.push("icon_cache");
        path
    }

    /// Load an icon from cache with validation
    ///
    /// For Win32 apps, validates cache freshness by comparing file modification times.
    /// Returns `Ok(None)` on cache miss or stale cache.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError` if cache file cannot be read, PNG decoding fails,
    /// or file metadata cannot be accessed.
    pub fn load_icon(&self, app_id: Uuid, source_path: Option<&Path>) -> Result<Option<Vec<u8>>> {
        let cache_path = self.cache_path(app_id);

        // Return Ok(None) if cache file does not exist
        if !cache_path.exists() {
            tracing::debug!("Cache miss for app {}: file does not exist", app_id);
            return Ok(None);
        }

        // Cache validation for Win32 apps
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

            // Compare modification times
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

            // Return Ok(None) if executable is newer than cache
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
            // Skip validation for UWP apps (no source path)
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
    /// Encodes RGBA data (must be exactly 4096 bytes for 32x32 pixels) to PNG format
    /// and saves atomically to cache.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError` if input size is not 4096 bytes, PNG encoding fails,
    /// temporary file creation fails, or atomic persist fails.
    pub fn save_icon(&self, app_id: Uuid, rgba_bytes: &[u8]) -> Result<()> {
        // Step 1 & 2: Validate size and encode RGBA to PNG
        // (validation happens inside encode_rgba_to_png)
        let png_bytes = Self::encode_rgba_to_png(rgba_bytes, app_id)?;

        // Get the final cache file path
        let cache_file_path = self.cache_path(app_id);

        // Step 3: Use tempfile::NamedTempFile::persist() for atomic write
        // (Atomic writes via tempfile)
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

    /// Remove a single icon from cache (idempotent)
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError::IconRemovalFailed` if the file cannot be deleted.
    pub fn remove_icon(&self, app_id: Uuid) -> Result<()> {
        let cache_path = self.cache_path(app_id);

        // Idempotent: OK if file doesn't exist
        if !cache_path.exists() {
            tracing::debug!(
                "Icon file for app {} does not exist, nothing to remove",
                app_id
            );
            return Ok(());
        }

        // Remove the cache file
        std::fs::remove_file(&cache_path).map_err(|source| {
            EasyHdrError::IconCache(IconCacheError::IconRemovalFailed {
                app_id,
                path: cache_path.clone(),
                source,
            })
        })?;

        tracing::debug!(
            "Removed cached icon for app {} from {}",
            app_id,
            cache_path.display()
        );

        Ok(())
    }

    /// Clear entire cache directory
    ///
    /// Removes all PNG files from the cache directory.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError::CacheClearFailed` if directory cannot be read
    /// or any file cannot be deleted.
    pub fn clear_cache(&self) -> Result<()> {
        // Remove all PNG files from cache directory

        // If directory doesn't exist, nothing to clear (idempotent)
        if !self.cache_dir.exists() {
            tracing::debug!("Cache directory does not exist, nothing to clear");
            return Ok(());
        }

        // Read directory entries
        let entries = std::fs::read_dir(&self.cache_dir).map_err(|source| {
            EasyHdrError::IconCache(IconCacheError::CacheClearFailed {
                path: self.cache_dir.clone(),
                source,
            })
        })?;

        let mut removed_count = 0;

        // Iterate through all entries and remove PNG files
        for entry in entries {
            let entry = entry.map_err(|source| {
                EasyHdrError::IconCache(IconCacheError::CacheClearFailed {
                    path: self.cache_dir.clone(),
                    source,
                })
            })?;

            let path = entry.path();

            // Only remove .png files
            if path.extension().and_then(|s| s.to_str()) == Some("png") {
                std::fs::remove_file(&path).map_err(|source| {
                    EasyHdrError::IconCache(IconCacheError::CacheClearFailed {
                        path: self.cache_dir.clone(),
                        source,
                    })
                })?;
                removed_count += 1;
            }
        }

        tracing::info!(
            "Cleared icon cache: removed {} PNG files from {}",
            removed_count,
            self.cache_dir.display()
        );

        Ok(())
    }

    /// Get cache statistics (icon count and total size in bytes)
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError::CacheStatsFailed` if the cache directory cannot be read.
    pub fn get_cache_stats(&self) -> Result<CacheStats> {
        // Calculate icon count and total size in bytes

        // If directory doesn't exist, return zero stats
        if !self.cache_dir.exists() {
            tracing::debug!("Cache directory does not exist, returning zero stats");
            return Ok(CacheStats {
                count: 0,
                size_bytes: 0,
            });
        }

        // Read directory entries
        let entries = std::fs::read_dir(&self.cache_dir).map_err(|source| {
            EasyHdrError::IconCache(IconCacheError::CacheStatsFailed {
                path: self.cache_dir.clone(),
                source,
            })
        })?;

        let mut count = 0;
        let mut size_bytes = 0u64;

        // Iterate through all entries and sum PNG file sizes
        for entry in entries {
            let entry = entry.map_err(|source| {
                EasyHdrError::IconCache(IconCacheError::CacheStatsFailed {
                    path: self.cache_dir.clone(),
                    source,
                })
            })?;

            let path = entry.path();

            // Only count .png files
            if path.extension().and_then(|s| s.to_str()) == Some("png") {
                // Get file metadata for size
                let metadata = std::fs::metadata(&path).map_err(|source| {
                    EasyHdrError::IconCache(IconCacheError::CacheStatsFailed {
                        path: self.cache_dir.clone(),
                        source,
                    })
                })?;

                count += 1;
                size_bytes += metadata.len();
            }
        }

        tracing::debug!("Cache statistics: {} icons, {} bytes", count, size_bytes);

        Ok(CacheStats { count, size_bytes })
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

    /// Cache icon data to disk with graceful error handling
    ///
    /// Failures are logged but do not propagate errors (graceful degradation).
    pub fn cache_icon_gracefully(id: Uuid, data: &[u8], display_name: &str) {
        if let Ok(cache) = Self::new(Self::default_cache_dir()) {
            if let Err(e) = cache.save_icon(id, data) {
                tracing::warn!(
                    "Failed to cache icon for '{}' ({}): {}. Icon will remain in memory only.",
                    display_name,
                    id,
                    e
                );
            } else {
                tracing::debug!(
                    "Successfully cached icon for '{}' ({}) to disk",
                    display_name,
                    id
                );
            }
        }
    }

    /// Encode RGBA data to PNG format
    ///
    /// Pre-allocates 8KB buffer (typical PNG size: 2-6KB for 32x32 RGBA).
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError` if input size is not exactly 4096 bytes or PNG encoding fails.
    fn encode_rgba_to_png(rgba_bytes: &[u8], app_id: Uuid) -> Result<Vec<u8>> {
        // Validate input size is exactly 4096 bytes (32x32 × 4 channels)
        const EXPECTED_SIZE: usize = 32 * 32 * 4;
        if rgba_bytes.len() != EXPECTED_SIZE {
            return Err(EasyHdrError::IconCache(IconCacheError::InvalidIconSize {
                actual: rgba_bytes.len(),
            }));
        }

        // Pre-allocate PNG buffer with 8192 bytes capacity
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
            // Return structured error with app UUID context
            EasyHdrError::IconCache(IconCacheError::PngEncodingError { app_id, source })
        })?;

        Ok(png_bytes)
    }

    /// Decode PNG data to RGBA format
    ///
    /// Resizes to exactly 32x32 pixels using Lanczos3 resampling.
    ///
    /// # Errors
    ///
    /// Returns `IconCacheError` if PNG decoding or resizing fails.
    fn decode_png_to_rgba(png_bytes: &[u8], app_id: Uuid) -> Result<Vec<u8>> {
        // Decode PNG from memory buffer
        let img = ImageReader::new(Cursor::new(png_bytes))
            .with_guessed_format()
            .map_err(|source| {
                // Return structured error with app UUID context
                EasyHdrError::IconCache(IconCacheError::PngDecodingError {
                    app_id,
                    source: image::ImageError::IoError(source),
                })
            })?
            .decode()
            .map_err(|source| {
                // Return structured error with app UUID context
                EasyHdrError::IconCache(IconCacheError::PngDecodingError { app_id, source })
            })?;

        // Resize to exactly 32x32 pixels using Lanczos3 filter
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

#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// Number of cached icons
    pub count: usize,
    /// Total size of all cached icons in bytes
    pub size_bytes: u64,
}

impl CacheStats {
    /// Format size as human-readable string (e.g., "42 KB" or "1.5 MB")
    #[expect(
        clippy::cast_precision_loss,
        reason = "Precision loss is acceptable for human-readable display formatting; exact byte values aren't needed for UI presentation"
    )]
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Compile-time assertion that `IconCache` is `Send + Sync`
    ///
    /// This test ensures thread safety requirements are met at compile time.
    /// If `IconCache` ever becomes non-Send or non-Sync, this test will fail
    /// to compile, preventing regressions.
    ///

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

    // PNG encoding/decoding tests

    #[test]
    fn encode_rgba_to_png_validates_input_size() {
        let app_id = Uuid::new_v4();

        // Test with invalid size (too small)
        let invalid_small = vec![0u8; 100];
        let result = IconCache::encode_rgba_to_png(&invalid_small, app_id);
        let err = result.expect_err("Should fail with InvalidIconSize");
        match err {
            EasyHdrError::IconCache(IconCacheError::InvalidIconSize { actual }) => {
                assert_eq!(actual, 100);
            }
            other => panic!("Expected InvalidIconSize, got: {other:?}"),
        }

        // Test with invalid size (too large)
        let invalid_large = vec![0u8; 5000];
        let result = IconCache::encode_rgba_to_png(&invalid_large, app_id);
        let err = result.expect_err("Should fail with InvalidIconSize");
        match err {
            EasyHdrError::IconCache(IconCacheError::InvalidIconSize { actual }) => {
                assert_eq!(actual, 5000);
            }
            other => panic!("Expected InvalidIconSize, got: {other:?}"),
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
    fn decode_png_handles_invalid_data() {
        let app_id = Uuid::new_v4();

        // Test with invalid PNG data
        let invalid_png = vec![0u8; 100];
        let result = IconCache::decode_png_to_rgba(&invalid_png, app_id);

        let err = result.expect_err("Invalid PNG data should produce error");
        match err {
            EasyHdrError::IconCache(IconCacheError::PngDecodingError {
                app_id: error_id, ..
            }) => {
                assert_eq!(error_id, app_id, "Error should include correct app UUID");
            }
            other => panic!("Expected PngDecodingError, got: {other:?}"),
        }
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

        let err = result.expect_err("Invalid size should produce error");
        match err {
            EasyHdrError::IconCache(IconCacheError::InvalidIconSize { actual }) => {
                assert_eq!(actual, 100);
            }
            other => panic!("Expected InvalidIconSize, got: {other:?}"),
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
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Test utility: modulo 256 ensures value fits in u8 range (0-255)"
    )]
    fn load_icon_cache_hit() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Create test RGBA data with a pattern
        let mut rgba_data = vec![0u8; 4096];
        for (i, item) in rgba_data.iter_mut().enumerate().take(4096) {
            *item = (i % 256) as u8;
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

        let err = result.expect_err("Should return error for corrupted PNG");
        match err {
            EasyHdrError::IconCache(IconCacheError::PngDecodingError {
                app_id: error_id, ..
            }) => {
                assert_eq!(error_id, app_id, "Error should include correct app UUID");
            }
            other => panic!("Expected PngDecodingError, got: {other:?}"),
        }
    }

    // Cache management operation tests

    #[test]
    fn remove_icon_deletes_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Save icon first
        let rgba_data = vec![128u8; 4096];
        cache
            .save_icon(app_id, &rgba_data)
            .expect("save_icon should succeed");

        // Verify file exists
        let cache_path = temp_dir.path().join(format!("{app_id}.png"));
        assert!(cache_path.exists(), "Icon file should exist");

        // Remove icon
        cache
            .remove_icon(app_id)
            .expect("remove_icon should succeed");

        // Verify file is deleted
        assert!(!cache_path.exists(), "Icon file should be deleted");
    }

    #[test]
    fn remove_icon_is_idempotent() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");
        let app_id = Uuid::new_v4();

        // Remove non-existent icon (should not error)
        cache
            .remove_icon(app_id)
            .expect("remove_icon should succeed for non-existent file");

        // Remove again (should still not error)
        cache
            .remove_icon(app_id)
            .expect("remove_icon should be idempotent");
    }

    #[test]
    fn clear_cache_removes_all_png_files() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");

        // Create multiple icon files
        let app_id_1 = Uuid::new_v4();
        let app_id_2 = Uuid::new_v4();
        let app_id_3 = Uuid::new_v4();

        let rgba_data = vec![128u8; 4096];
        cache.save_icon(app_id_1, &rgba_data).expect("save 1");
        cache.save_icon(app_id_2, &rgba_data).expect("save 2");
        cache.save_icon(app_id_3, &rgba_data).expect("save 3");

        // Verify files exist
        assert!(temp_dir.path().join(format!("{app_id_1}.png")).exists());
        assert!(temp_dir.path().join(format!("{app_id_2}.png")).exists());
        assert!(temp_dir.path().join(format!("{app_id_3}.png")).exists());

        // Clear cache
        cache.clear_cache().expect("clear_cache should succeed");

        // Verify all files are deleted
        assert!(!temp_dir.path().join(format!("{app_id_1}.png")).exists());
        assert!(!temp_dir.path().join(format!("{app_id_2}.png")).exists());
        assert!(!temp_dir.path().join(format!("{app_id_3}.png")).exists());
    }

    #[test]
    fn clear_cache_only_removes_png_files() {
        use std::fs::File;
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");

        // Create PNG file
        let app_id = Uuid::new_v4();
        let rgba_data = vec![128u8; 4096];
        cache.save_icon(app_id, &rgba_data).expect("save icon");

        // Create a non-PNG file in the cache directory
        let txt_path = temp_dir.path().join("readme.txt");
        let mut txt_file = File::create(&txt_path).expect("create txt file");
        txt_file.write_all(b"test file").expect("write txt");
        drop(txt_file);

        // Clear cache
        cache.clear_cache().expect("clear_cache should succeed");

        // PNG should be deleted
        assert!(!temp_dir.path().join(format!("{app_id}.png")).exists());

        // Non-PNG file should remain
        assert!(txt_path.exists(), "Non-PNG files should not be deleted");
    }

    #[test]
    fn clear_cache_is_idempotent() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");

        // Clear empty cache (should not error)
        cache
            .clear_cache()
            .expect("clear_cache should succeed on empty cache");

        // Clear again (should still not error)
        cache
            .clear_cache()
            .expect("clear_cache should be idempotent");
    }

    #[test]
    fn get_cache_stats_returns_correct_count_and_size() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");

        // Initially empty
        let stats = cache
            .get_cache_stats()
            .expect("get_cache_stats should succeed");
        assert_eq!(stats.count, 0, "Initial count should be 0");
        assert_eq!(stats.size_bytes, 0, "Initial size should be 0");

        // Add first icon
        let app_id_1 = Uuid::new_v4();
        let rgba_data = vec![128u8; 4096];
        cache.save_icon(app_id_1, &rgba_data).expect("save icon 1");

        let stats = cache
            .get_cache_stats()
            .expect("get_cache_stats should succeed");
        assert_eq!(stats.count, 1, "Count should be 1 after adding one icon");
        assert!(stats.size_bytes > 0, "Size should be greater than 0");

        let size_after_one = stats.size_bytes;

        // Add second icon
        let app_id_2 = Uuid::new_v4();
        cache.save_icon(app_id_2, &rgba_data).expect("save icon 2");

        let stats = cache
            .get_cache_stats()
            .expect("get_cache_stats should succeed");
        assert_eq!(stats.count, 2, "Count should be 2 after adding two icons");
        assert!(
            stats.size_bytes > size_after_one,
            "Size should increase after adding second icon"
        );

        // Add third icon
        let app_id_3 = Uuid::new_v4();
        cache.save_icon(app_id_3, &rgba_data).expect("save icon 3");

        let stats = cache
            .get_cache_stats()
            .expect("get_cache_stats should succeed");
        assert_eq!(stats.count, 3, "Count should be 3 after adding three icons");
    }

    #[test]
    fn get_cache_stats_only_counts_png_files() {
        use std::fs::File;
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");

        // Add PNG icon
        let app_id = Uuid::new_v4();
        let rgba_data = vec![128u8; 4096];
        cache.save_icon(app_id, &rgba_data).expect("save icon");

        // Create non-PNG file
        let txt_path = temp_dir.path().join("readme.txt");
        let mut txt_file = File::create(&txt_path).expect("create txt file");
        txt_file.write_all(b"test file content").expect("write txt");
        drop(txt_file);

        // Get stats
        let stats = cache
            .get_cache_stats()
            .expect("get_cache_stats should succeed");

        // Should only count the PNG file
        assert_eq!(stats.count, 1, "Should only count PNG files");
    }

    #[test]
    fn get_cache_stats_empty_directory() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache = IconCache::new(temp_dir.path()).expect("Failed to create cache");

        let stats = cache
            .get_cache_stats()
            .expect("get_cache_stats should succeed");

        assert_eq!(stats.count, 0, "Empty cache should have count 0");
        assert_eq!(stats.size_bytes, 0, "Empty cache should have size 0");
    }
}

/// Property-based tests for PNG encoding/decoding
///
/// Verifies that PNG encoding/decoding roundtrip preserves data for arbitrary RGBA inputs.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property test: PNG encoding/decoding roundtrip preserves data
        ///
        /// Verifies that encode → decode roundtrip preserves RGBA data byte-for-byte.
        #[test]
        fn png_encoding_roundtrip_preserves_data(
            rgba_bytes in prop::collection::vec(any::<u8>(), 4096..=4096)
        ) {
            // Generate a fresh UUID for each test case
            let app_id = Uuid::new_v4();

            // Encode RGBA to PNG
            let encoded = IconCache::encode_rgba_to_png(&rgba_bytes, app_id)
                .expect("Encoding should succeed for valid 4096-byte input");

            // Decode PNG back to RGBA
            let decoded = IconCache::decode_png_to_rgba(&encoded, app_id)
                .expect("Decoding should succeed for valid PNG data");

            // Verify roundtrip preserves data exactly
            prop_assert_eq!(rgba_bytes.len(), decoded.len(),
                "Decoded data should have same length as original");
            prop_assert_eq!(rgba_bytes, decoded,
                "Roundtrip should preserve RGBA data byte-for-byte");
        }

        /// Property test: PNG decoding produces consistent output size (always 4096 bytes)
        #[test]
        fn png_decoding_always_produces_correct_size(
            rgba_bytes in prop::collection::vec(any::<u8>(), 4096..=4096)
        ) {
            let app_id = Uuid::new_v4();

            // Encode to PNG
            let encoded = IconCache::encode_rgba_to_png(&rgba_bytes, app_id)
                .expect("Encoding should succeed");

            // Decode back
            let decoded = IconCache::decode_png_to_rgba(&encoded, app_id)
                .expect("Decoding should succeed");

            // Verify output size is always exactly 4096 bytes (32x32 RGBA)
            prop_assert_eq!(decoded.len(), 4096,
                "Decoded data must always be exactly 4096 bytes");
        }

        /// Property test: PNG encoding produces valid PNG signature
        #[test]
        fn png_encoding_produces_valid_png_signature(
            rgba_bytes in prop::collection::vec(any::<u8>(), 4096..=4096)
        ) {
            let app_id = Uuid::new_v4();

            // Encode to PNG
            let encoded = IconCache::encode_rgba_to_png(&rgba_bytes, app_id)
                .expect("Encoding should succeed");

            // Verify PNG signature (first 8 bytes)
            // PNG files start with: 137 80 78 71 13 10 26 10
            prop_assert!(encoded.len() >= 8,
                "PNG data should have at least 8 bytes for signature");
            prop_assert_eq!(encoded[0], 137, "PNG signature byte 0 (0x89)");
            prop_assert_eq!(encoded[1], 80,  "PNG signature byte 1 ('P')");
            prop_assert_eq!(encoded[2], 78,  "PNG signature byte 2 ('N')");
            prop_assert_eq!(encoded[3], 71,  "PNG signature byte 3 ('G')");
            prop_assert_eq!(encoded[4], 13,  "PNG signature byte 4 (\\r)");
            prop_assert_eq!(encoded[5], 10,  "PNG signature byte 5 (\\n)");
            prop_assert_eq!(encoded[6], 26,  "PNG signature byte 6 (0x1a)");
            prop_assert_eq!(encoded[7], 10,  "PNG signature byte 7 (\\n)");
        }
    }
}
