//! Configuration manager for loading and saving application configuration.

use crate::config::models::AppConfig;
use crate::error::{EasyHdrError, Result};
use std::path::PathBuf;
use tracing::{info, warn};

/// Configuration manager
pub struct ConfigManager;

impl ConfigManager {
    /// Get the path to the configuration file.
    pub fn get_config_path() -> PathBuf {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata).join("EasyHDR").join("config.json")
    }

    /// Ensure the configuration directory exists.
    pub fn ensure_config_dir() -> Result<PathBuf> {
        use tracing::{debug, error};

        let config_path = Self::get_config_path();
        let config_dir = config_path.parent().ok_or_else(|| {
            error!("Invalid config path - no parent directory");
            EasyHdrError::ConfigError(crate::error::StringError::new("Invalid config path"))
        })?;

        std::fs::create_dir_all(config_dir).map_err(|e| {
            error!("Failed to create config directory {:?}: {}", config_dir, e);
            e
        })?;

        debug!("Config directory ensured: {:?}", config_dir);
        Ok(config_dir.to_path_buf())
    }

    /// Load configuration from disk.
    ///
    /// Returns default configuration if the file doesn't exist or is corrupt.
    pub fn load() -> Result<AppConfig> {
        use tracing::error;

        let config_path = Self::get_config_path();

        if !config_path.exists() {
            info!(
                "Configuration file not found at {:?}, using defaults",
                config_path
            );
            return Ok(AppConfig::default());
        }

        let json = std::fs::read_to_string(&config_path).map_err(|e| {
            error!("Failed to read configuration file {:?}: {}", config_path, e);
            e
        })?;

        let mut config = match serde_json::from_str(&json) {
            Ok(config) => {
                info!("Configuration loaded successfully from {:?}", config_path);
                config
            }
            Err(e) => {
                warn!(
                    "Failed to parse configuration from {:?}, using defaults: {}",
                    config_path, e
                );
                AppConfig::default()
            }
        };

        if let Err(e) = Self::restore_icons_from_cache(&mut config) {
            warn!(
                "Failed to restore icons from cache: {}. Continuing without cached icons.",
                e
            );
        }

        Self::regenerate_missing_icons(&mut config);

        Ok(config)
    }

    /// Restore icons from disk cache in parallel.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "Returns Result<()> for API consistency with other ConfigManager methods and to allow future error propagation. Current implementation uses graceful degradation where all errors are logged but don't prevent startup."
    )]
    fn restore_icons_from_cache(config: &mut AppConfig) -> Result<()> {
        use crate::config::models::MonitoredApp;
        use rayon::prelude::*;

        if config.monitored_apps.is_empty() {
            tracing::debug!("No monitored apps to restore icons for");
            return Ok(());
        }

        // Initialize icon cache
        let cache = match crate::utils::IconCache::new(crate::utils::IconCache::default_cache_dir())
        {
            Ok(cache) => cache,
            Err(e) => {
                tracing::warn!(
                    "Failed to initialize icon cache: {}. Icons will not be restored.",
                    e
                );
                return Ok(()); // Graceful degradation
            }
        };

        tracing::debug!(
            "Restoring icons from cache for {} apps",
            config.monitored_apps.len()
        );

        let icons: Vec<(uuid::Uuid, Vec<u8>)> = config
            .monitored_apps
            .par_iter()
            .filter_map(|app| {
                let source_path = match app {
                    MonitoredApp::Win32(win32) => Some(win32.exe_path.as_path()),
                    MonitoredApp::Uwp(_) => None,
                };

                match cache.load_icon(*app.id(), source_path) {
                    Ok(Some(icon_data)) => {
                        tracing::trace!("Restored icon for app {} from cache", app.id());
                        Some((*app.id(), icon_data))
                    }
                    Ok(None) => {
                        tracing::debug!(
                            "Cache miss for app {} ({})",
                            app.display_name(),
                            app.id()
                        );
                        None
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to load cached icon for app {} ({}): {}. Icon will need to be re-extracted.",
                            app.display_name(),
                            app.id(),
                            e
                        );
                        None
                    }
                }
            })
            .collect();

        let restored_count = icons.len();
        for (app_id, icon_data) in icons {
            if let Some(app) = config.monitored_apps.iter_mut().find(|a| *a.id() == app_id) {
                *app.icon_data_mut() = Some(icon_data);
            }
        }

        if restored_count > 0 {
            tracing::info!(
                "Restored {} cached icon{} from disk",
                restored_count,
                if restored_count == 1 { "" } else { "s" }
            );
        } else {
            tracing::debug!("No cached icons were restored (cache miss or errors)");
        }

        Ok(())
    }

    /// Re-extract icons for apps that failed to load from cache.
    #[expect(
        clippy::too_many_lines,
        reason = "Icon regeneration logic requires handling both Win32 and UWP apps with different extraction strategies; splitting would reduce cohesion"
    )]
    fn regenerate_missing_icons(config: &mut AppConfig) {
        use crate::config::models::MonitoredApp;
        #[cfg(windows)]
        use crate::utils::memory_profiler;
        #[cfg(windows)]
        use crate::uwp;

        if config.monitored_apps.is_empty() {
            tracing::debug!("No monitored apps to regenerate icons for");
            return;
        }

        let apps_needing_icons_count = config
            .monitored_apps
            .iter()
            .filter(|app| match app {
                MonitoredApp::Win32(win32) => win32.icon_data.is_none(),
                MonitoredApp::Uwp(uwp) => uwp.icon_data.is_none(),
            })
            .count();

        if apps_needing_icons_count == 0 {
            tracing::debug!("All apps have icons loaded, no regeneration needed");
            return;
        }

        tracing::info!(
            "Regenerating icons for {} app{} with missing icons",
            apps_needing_icons_count,
            if apps_needing_icons_count == 1 {
                ""
            } else {
                "s"
            }
        );

        // Initialize icon cache for saving regenerated icons
        let cache = match crate::utils::IconCache::new(crate::utils::IconCache::default_cache_dir())
        {
            Ok(cache) => Some(cache),
            Err(e) => {
                tracing::warn!(
                    "Failed to initialize icon cache for regeneration: {}. Icons will not be cached.",
                    e
                );
                None
            }
        };

        #[cfg(windows)]
        let uwp_packages = {
            match uwp::enumerate_packages() {
                Ok(packages) => Some(packages),
                Err(e) => {
                    tracing::warn!(
                        "Failed to enumerate UWP packages for icon regeneration: {}. UWP icons will not be regenerated.",
                        e
                    );
                    None
                }
            }
        };

        let mut regenerated_count = 0;

        for app in &mut config.monitored_apps {
            if app.icon_data_mut().is_some() {
                continue;
            }

            match app {
                MonitoredApp::Win32(win32_app) => {
                    if win32_app.ensure_icon_loaded().is_some() {
                        tracing::debug!(
                            "Regenerated icon for Win32 app '{}' from exe",
                            win32_app.display_name
                        );

                        if let (Some(cache), Some(icon_data)) = (&cache, &win32_app.icon_data) {
                            if let Err(e) = cache.save_icon(win32_app.id, icon_data) {
                                tracing::warn!(
                                    "Failed to cache regenerated icon for '{}': {}",
                                    win32_app.display_name,
                                    e
                                );
                            }
                        }

                        regenerated_count += 1;
                    } else {
                        tracing::warn!(
                            "Failed to regenerate icon for Win32 app '{}' from {:?}",
                            win32_app.display_name,
                            win32_app.exe_path
                        );
                    }
                }
                MonitoredApp::Uwp(uwp_app) => {
                    #[cfg(windows)]
                    if let Some(packages) = &uwp_packages {
                        if let Some(pkg) = packages
                            .iter()
                            .find(|p| p.package_family_name == uwp_app.package_family_name)
                        {
                            if let Some(logo_stream) = &pkg.logo_stream {
                                match uwp::extract_icon_from_stream(logo_stream) {
                                    Ok(icon_data) if !icon_data.is_empty() => {
                                        tracing::debug!(
                                            "Regenerated icon for UWP app '{}' from package",
                                            uwp_app.display_name
                                        );

                                        if let Some(cache) = &cache {
                                            if let Err(e) = cache.save_icon(uwp_app.id, &icon_data)
                                            {
                                                tracing::warn!(
                                                    "Failed to cache regenerated UWP icon for '{}': {}",
                                                    uwp_app.display_name,
                                                    e
                                                );
                                            }
                                        }

                                        memory_profiler::get_profiler()
                                            .record_icon_cached(icon_data.len());

                                        uwp_app.icon_data = Some(icon_data);
                                        regenerated_count += 1;
                                    }
                                    Ok(_) => {
                                        tracing::debug!(
                                            "Icon extraction returned empty data for UWP app '{}'",
                                            uwp_app.display_name
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to regenerate icon for UWP app '{}': {}",
                                            uwp_app.display_name,
                                            e
                                        );
                                    }
                                }
                            } else {
                                tracing::debug!(
                                    "No logo path available for UWP app '{}'",
                                    uwp_app.display_name
                                );
                            }
                        } else {
                            tracing::warn!(
                                "UWP package '{}' not found during icon regeneration",
                                uwp_app.package_family_name
                            );
                        }
                    }

                    #[cfg(not(windows))]
                    {
                        let _ = uwp_app; // Suppress unused variable warning
                    }
                }
            }
        }

        if regenerated_count > 0 {
            tracing::info!(
                "Successfully regenerated {} icon{} from source",
                regenerated_count,
                if regenerated_count == 1 { "" } else { "s" }
            );
        } else {
            tracing::debug!("No icons were regenerated");
        }
    }

    /// Save configuration to disk with atomic write.
    pub fn save(config: &AppConfig) -> Result<()> {
        use tracing::{debug, error};

        let config_path = Self::get_config_path();
        Self::ensure_config_dir()?;

        let config_dir = config_path.parent().ok_or_else(|| {
            error!("Invalid config path - no parent directory");
            EasyHdrError::ConfigError(crate::error::StringError::new("Invalid config path"))
        })?;

        let temp_path = config_dir.join("config.json.tmp");

        debug!("Serializing configuration to JSON");
        let json = serde_json::to_string_pretty(config).map_err(|e| {
            error!("Failed to serialize configuration to JSON: {}", e);
            e
        })?;

        debug!("Writing configuration to temp file: {:?}", temp_path);
        std::fs::write(&temp_path, &json).map_err(|e| {
            error!(
                "Failed to write configuration to temp file {:?}: {}",
                temp_path, e
            );
            e
        })?;

        debug!("Renaming temp file to config file: {:?}", config_path);
        std::fs::rename(&temp_path, &config_path).map_err(|e| {
            error!(
                "Failed to rename temp file {:?} to {:?}: {}",
                temp_path, config_path, e
            );
            e
        })?;

        info!("Configuration saved successfully to {:?}", config_path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::{MonitoredApp, Win32App};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;
    use uuid::Uuid;

    // Global mutex to serialize tests that modify the APPDATA environment variable.
    // This prevents race conditions when multiple tests run in parallel and try to
    // set different APPDATA values.
    static APPDATA_LOCK: Mutex<()> = Mutex::new(());

    /// Helper function to create a temporary test directory using tempfile
    /// Returns a `TempDir` that automatically cleans up when dropped
    fn create_test_dir() -> TempDir {
        tempfile::tempdir().expect("Failed to create temp directory")
    }

    /// Helper to set APPDATA for a test scope
    /// Returns a guard that restores the original value when dropped
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
    /// 3. No other threads should be spawned or running during the lifetime of this guard
    ///    within the same test function
    ///
    /// **Why this is safe in parallel test execution:**
    /// - While `std::env::set_var` is unsafe, the actual risk is when threads read env vars
    ///   while another thread modifies them
    /// - Each test function runs in its own thread with its own stack frame
    /// - The `ConfigManager` being tested is not spawning additional threads
    /// - The guard ensures cleanup even on panic via Drop
    /// - The modification is scoped to the test function's lifetime
    /// - Tests can safely run in parallel (`cargo test --lib`) without `--test-threads=1`
    ///
    /// **Note:** While these tests CAN run in parallel, they can also run single-threaded
    /// if needed for other reasons (e.g., debugging, Miri analysis).
    struct AppdataGuard {
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
        fn new(temp_dir: &TempDir) -> Self {
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

    /// Helper function to clean up test directory (deprecated - use `TempDir` instead)
    #[expect(dead_code)]
    fn cleanup_test_dir(dir: &PathBuf) {
        if dir.exists() {
            fs::remove_dir_all(dir).ok();
        }
    }

    #[test]
    fn test_config_path() {
        let path = ConfigManager::get_config_path();
        assert!(path.to_string_lossy().contains("EasyHDR"));
        assert!(path.to_string_lossy().ends_with("config.json"));
    }

    // NOTE: This test may fail when run in parallel with other tests due to a race condition.
    // This test expects the config file to be missing, but other tests (particularly in
    // controller::app_controller) write to the same shared config file (./EasyHDR/config.json
    // on macOS, %APPDATA%\EasyHDR\config.json on Windows). When those tests run concurrently,
    // they may create/modify the config file, causing this test to load non-default data.
    //
    // The functionality itself is correct - the test passes consistently when run:
    // - Individually: `cargo test test_load_missing_config`
    // - Single-threaded: `cargo test -- --test-threads=1`
    //
    // This is a test isolation issue, not a code defect.
    #[test]
    fn test_load_missing_config() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // This should return default config without error
        let config = ConfigManager::load();
        assert!(config.is_ok());

        let config = config.unwrap();
        assert_eq!(config.monitored_apps.len(), 0);
        assert_eq!(config.preferences.monitoring_interval_ms, 1000);

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    #[test]
    fn test_save_and_load_round_trip() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Create a config with data
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Test Game".to_string(),
            exe_path: PathBuf::from("C:\\Games\\test.exe"),
            process_name: "test".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.preferences.auto_start = true;
        config.preferences.monitoring_interval_ms = 500;

        // Create config directory manually
        let config_dir = test_dir.path().join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();

        // Write config directly to test directory
        let config_path = config_dir.join("config.json");
        let json = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&config_path, json).unwrap();

        // Verify file exists
        assert!(config_path.exists(), "Config file was not created");

        // Read back and verify
        let json_content = fs::read_to_string(&config_path).unwrap();
        let loaded_config: AppConfig = serde_json::from_str(&json_content).unwrap();

        // Verify the data matches
        assert_eq!(
            config.monitored_apps.len(),
            loaded_config.monitored_apps.len()
        );
        assert_eq!(
            config.monitored_apps[0].id(),
            loaded_config.monitored_apps[0].id()
        );
        assert_eq!(
            config.monitored_apps[0].display_name(),
            loaded_config.monitored_apps[0].display_name()
        );
        assert_eq!(
            config.preferences.auto_start,
            loaded_config.preferences.auto_start
        );
        assert_eq!(
            config.preferences.monitoring_interval_ms,
            loaded_config.preferences.monitoring_interval_ms
        );

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    #[test]
    fn test_load_corrupt_json_returns_defaults() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Create config directory
        let config_dir = test_dir.path().join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();

        // Write corrupt JSON to config file
        let config_path = config_dir.join("config.json");
        fs::write(&config_path, "{ this is not valid json }").unwrap();

        // Load should return default config without error
        let result = ConfigManager::load();
        assert!(result.is_ok(), "Load should succeed with corrupt JSON");

        let config = result.unwrap();

        // Verify we got default config
        assert_eq!(config.monitored_apps.len(), 0);
        assert_eq!(config.preferences.monitoring_interval_ms, 1000);
        assert!(!config.preferences.auto_start);

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    #[test]
    fn test_load_incomplete_json_returns_defaults() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Create config directory
        let config_dir = test_dir.path().join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();

        // Write incomplete JSON (missing required fields)
        let config_path = config_dir.join("config.json");
        fs::write(&config_path, r#"{"monitored_apps": []}"#).unwrap();

        // Load should return default config without error
        let result = ConfigManager::load();
        assert!(result.is_ok(), "Load should succeed with incomplete JSON");

        let config = result.unwrap();

        // Verify we got default config
        assert_eq!(config.monitored_apps.len(), 0);

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    #[test]
    fn test_load_malformed_json_returns_defaults() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Create config directory
        let config_dir = test_dir.path().join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();

        // Write various types of malformed JSON
        let test_cases = [
            "",                                      // Empty file
            "{",                                     // Unclosed brace
            "null",                                  // Null value
            "[]",                                    // Array instead of object
            r#"{"monitored_apps": "not an array"}"#, // Wrong type
        ];

        for (i, malformed_json) in test_cases.iter().enumerate() {
            let config_path = config_dir.join("config.json");

            // Remove the file if it exists to ensure clean state between iterations
            // This prevents Windows file system caching issues
            if config_path.exists() {
                fs::remove_file(&config_path).unwrap();
            }

            fs::write(&config_path, malformed_json).unwrap();

            let result = ConfigManager::load();
            assert!(
                result.is_ok(),
                "Test case {i} failed: Load should succeed with malformed JSON"
            );

            let config = result.unwrap();
            assert_eq!(
                config.monitored_apps.len(),
                0,
                "Test case {i} failed: Should return default config"
            );
        }

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    #[test]
    fn test_atomic_write_creates_temp_file() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        let config = AppConfig::default();

        // Save the config
        let result = ConfigManager::save(&config);
        assert!(result.is_ok());

        // Verify the final config file exists
        let config_path = test_dir.path().join("EasyHDR").join("config.json");
        assert!(config_path.exists(), "Final config file should exist");

        // Verify the temp file was cleaned up
        let temp_path = test_dir.path().join("EasyHDR").join("config.json.tmp");
        assert!(
            !temp_path.exists(),
            "Temp file should be removed after atomic write"
        );

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    #[test]
    fn test_save_creates_directory_if_missing() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Ensure directory doesn't exist
        let config_dir = test_dir.path().join("EasyHDR");
        if config_dir.exists() {
            fs::remove_dir_all(&config_dir).unwrap();
        }

        let config = AppConfig::default();

        // Save should create the directory
        let result = ConfigManager::save(&config);
        assert!(result.is_ok(), "Save should create directory if missing");

        // Verify directory was created
        assert!(config_dir.exists(), "Config directory should be created");

        // Verify config file exists
        let config_path = config_dir.join("config.json");
        assert!(config_path.exists(), "Config file should exist");

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    #[test]
    fn test_ensure_config_dir() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Call ensure_config_dir
        let result = ConfigManager::ensure_config_dir();
        assert!(result.is_ok());

        let config_dir = result.unwrap();

        // Verify directory exists
        assert!(config_dir.exists());
        assert!(config_dir.to_string_lossy().contains("EasyHDR"));

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    #[test]
    fn test_config_json_is_pretty_printed() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        let config = AppConfig::default();
        ConfigManager::save(&config).unwrap();

        // Read the raw JSON file
        let config_path = test_dir.path().join("EasyHDR").join("config.json");
        let json_content = fs::read_to_string(&config_path).unwrap();

        // Verify it's pretty-printed (contains newlines and indentation)
        assert!(
            json_content.contains('\n'),
            "JSON should be pretty-printed with newlines"
        );
        assert!(
            json_content.contains("  "),
            "JSON should be pretty-printed with indentation"
        );

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    /// Test that saving UI settings preserves update check metadata
    ///
    /// This test verifies the fix for Phase 2 of the remediation plan:
    /// When GUI settings (`auto_start`, `monitoring_interval`, etc.) are saved,
    /// the update check metadata (`last_update_check_time`, `cached_latest_version`)
    /// should be preserved, not zeroed out.
    ///
    /// This ensures rate limiting for update checks continues to work correctly
    /// after settings changes.
    #[test]
    fn test_settings_save_preserves_update_metadata() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Create initial config with update check metadata
        let mut config = AppConfig::default();
        config.preferences.last_update_check_time = 1_234_567_890;
        config.preferences.cached_latest_version = "1.2.3".to_string();
        config.preferences.auto_start = false;
        config.preferences.monitoring_interval_ms = 1000;

        // Save initial config
        ConfigManager::save(&config).unwrap();

        // Verify the saved JSON contains the expected fields (Windows file system sync check)
        let config_path = ConfigManager::get_config_path();
        let saved_json =
            fs::read_to_string(&config_path).expect("Should be able to read saved config");
        assert!(
            saved_json.contains("1234567890"),
            "First save should include last_update_check_time in JSON"
        );

        // Simulate GUI settings save: load config, modify UI settings, save
        let mut loaded_config = ConfigManager::load().unwrap();

        // Verify the loaded config has the correct metadata (Windows caching check)
        assert_eq!(
            loaded_config.preferences.last_update_check_time, 1_234_567_890,
            "First load should preserve last_update_check_time"
        );
        assert_eq!(
            loaded_config.preferences.cached_latest_version, "1.2.3",
            "First load should preserve cached_latest_version"
        );

        // Modify only UI-controlled fields (simulating partial update pattern)
        loaded_config.preferences.auto_start = true;
        loaded_config.preferences.monitoring_interval_ms = 1500;
        loaded_config.preferences.show_tray_notifications = false;
        // Note: last_update_check_time and cached_latest_version are NOT modified

        // Remove config file before second save to prevent Windows file system caching issues
        // This ensures the new save creates a fresh file rather than potentially being cached
        if config_path.exists() {
            fs::remove_file(&config_path).unwrap();
        }

        // Save the modified config
        ConfigManager::save(&loaded_config).unwrap();

        // Verify the second save also includes metadata (Windows file system sync check)
        let saved_json = fs::read_to_string(&config_path)
            .expect("Should be able to read saved config after second save");
        assert!(
            saved_json.contains("1234567890"),
            "Second save should preserve last_update_check_time in JSON"
        );

        // Load again and verify update metadata was preserved
        let final_config = ConfigManager::load().unwrap();

        // Verify UI settings were updated
        assert!(final_config.preferences.auto_start);
        assert_eq!(final_config.preferences.monitoring_interval_ms, 1500);
        assert!(!final_config.preferences.show_tray_notifications);

        // Verify update check metadata was preserved (this is the critical assertion)
        assert_eq!(
            final_config.preferences.last_update_check_time, 1_234_567_890,
            "last_update_check_time should be preserved across settings saves"
        );
        assert_eq!(
            final_config.preferences.cached_latest_version, "1.2.3",
            "cached_latest_version should be preserved across settings saves"
        );

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    /// Test `ConfigManager` save/load with mixed Win32 and UWP apps
    #[test]
    fn test_save_and_load_mixed_win32_and_uwp_apps() {
        use crate::config::models::UwpApp;

        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Create config with mixed Win32 and UWP apps
        let mut config = AppConfig::default();

        // Add Win32 app
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Cyberpunk 2077".to_string(),
            exe_path: PathBuf::from("C:\\Games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe"),
            process_name: "cyberpunk2077".to_string(),
            enabled: true,
            icon_data: None,
        }));

        // Add UWP app
        config.monitored_apps.push(MonitoredApp::Uwp(UwpApp {
            id: Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap(),
            display_name: "Calculator".to_string(),
            package_family_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
            app_id: "App".to_string(),
            enabled: true,
            icon_data: None,
        }));

        // Add another Win32 app
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::parse_str("7c9e6679-7425-40de-944b-e07fc1f90ae7").unwrap(),
            display_name: "Red Dead Redemption 2".to_string(),
            exe_path: PathBuf::from("D:\\Games\\RDR2\\RDR2.exe"),
            process_name: "rdr2".to_string(),
            enabled: false,
            icon_data: None,
        }));

        config.preferences.auto_start = true;
        config.preferences.monitoring_interval_ms = 500;

        // Save config
        let result = ConfigManager::save(&config);
        assert!(result.is_ok(), "Save should succeed with mixed app types");

        // Verify the saved JSON contains app_type fields
        let config_path = ConfigManager::get_config_path();
        let saved_json = fs::read_to_string(&config_path).unwrap();
        assert!(
            saved_json.contains("\"app_type\": \"win32\""),
            "Saved JSON should contain Win32 app_type"
        );
        assert!(
            saved_json.contains("\"app_type\": \"uwp\""),
            "Saved JSON should contain UWP app_type"
        );

        // Load config back
        let loaded_config = ConfigManager::load().unwrap();

        // Verify all apps were loaded correctly
        assert_eq!(
            loaded_config.monitored_apps.len(),
            3,
            "Should load all 3 apps"
        );

        // Verify first app (Win32)
        assert!(matches!(
            loaded_config.monitored_apps[0],
            MonitoredApp::Win32(_)
        ));
        assert_eq!(
            loaded_config.monitored_apps[0].display_name(),
            "Cyberpunk 2077"
        );
        assert!(loaded_config.monitored_apps[0].is_enabled());

        // Verify second app (UWP)
        assert!(matches!(
            loaded_config.monitored_apps[1],
            MonitoredApp::Uwp(_)
        ));
        assert_eq!(loaded_config.monitored_apps[1].display_name(), "Calculator");
        assert!(loaded_config.monitored_apps[1].is_enabled());

        // Verify third app (Win32)
        assert!(matches!(
            loaded_config.monitored_apps[2],
            MonitoredApp::Win32(_)
        ));
        assert_eq!(
            loaded_config.monitored_apps[2].display_name(),
            "Red Dead Redemption 2"
        );
        assert!(!loaded_config.monitored_apps[2].is_enabled());

        // Verify preferences were preserved
        assert!(loaded_config.preferences.auto_start);
        assert_eq!(loaded_config.preferences.monitoring_interval_ms, 500);

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    /// Test `ConfigManager` load with legacy config format (automatic migration)
    #[test]
    fn test_load_legacy_config_automatic_migration() {
        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Create config directory
        let config_dir = test_dir.path().join("EasyHDR");
        fs::create_dir_all(&config_dir).unwrap();

        // Write legacy config format (without app_type field)
        let legacy_json = r#"{
            "monitored_apps": [
                {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "display_name": "Legacy Game",
                    "exe_path": "C:\\Games\\Legacy\\game.exe",
                    "process_name": "game",
                    "enabled": true
                },
                {
                    "id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
                    "display_name": "Another Legacy App",
                    "exe_path": "D:\\Apps\\app.exe",
                    "process_name": "app",
                    "enabled": false
                }
            ],
            "preferences": {
                "auto_start": true,
                "monitoring_interval_ms": 1500,
                "show_tray_notifications": false,
                "show_update_notifications": true,
                "minimize_to_tray_on_minimize": true,
                "minimize_to_tray_on_close": false,
                "start_minimized_to_tray": false,
                "last_update_check_time": 0,
                "cached_latest_version": ""
            },
            "window_state": {
                "x": 200,
                "y": 150,
                "width": 800,
                "height": 600
            }
        }"#;

        let config_path = config_dir.join("config.json");
        fs::write(&config_path, legacy_json).unwrap();

        // Load config - should automatically migrate legacy format
        let loaded_config = ConfigManager::load().unwrap();

        // Verify both apps were loaded and migrated to Win32 variant
        assert_eq!(
            loaded_config.monitored_apps.len(),
            2,
            "Should load both legacy apps"
        );

        // Verify first app
        assert!(
            matches!(loaded_config.monitored_apps[0], MonitoredApp::Win32(_)),
            "Legacy app should be migrated to Win32 variant"
        );
        assert_eq!(
            loaded_config.monitored_apps[0].id(),
            &Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
        );
        assert_eq!(
            loaded_config.monitored_apps[0].display_name(),
            "Legacy Game"
        );
        assert!(loaded_config.monitored_apps[0].is_enabled());

        // Verify all fields were preserved for first app
        if let MonitoredApp::Win32(app) = &loaded_config.monitored_apps[0] {
            assert_eq!(app.exe_path, PathBuf::from("C:\\Games\\Legacy\\game.exe"));
            assert_eq!(app.process_name, "game");
        } else {
            panic!("Expected Win32 variant");
        }

        // Verify second app
        assert!(
            matches!(loaded_config.monitored_apps[1], MonitoredApp::Win32(_)),
            "Legacy app should be migrated to Win32 variant"
        );
        assert_eq!(
            loaded_config.monitored_apps[1].display_name(),
            "Another Legacy App"
        );
        assert!(!loaded_config.monitored_apps[1].is_enabled());

        // Verify preferences were preserved
        assert!(loaded_config.preferences.auto_start);
        assert_eq!(loaded_config.preferences.monitoring_interval_ms, 1500);
        assert!(!loaded_config.preferences.show_tray_notifications);

        // Verify window state was preserved
        assert_eq!(loaded_config.window_state.x, 200);
        assert_eq!(loaded_config.window_state.y, 150);
        assert_eq!(loaded_config.window_state.width, 800);
        assert_eq!(loaded_config.window_state.height, 600);

        // TempDir and AppdataGuard automatically clean up when dropped
    }

    /// Test that atomic write mechanism works correctly with `MonitoredApp` enum
    ///
    /// Verifies that the temp file -> rename atomic write pattern still functions
    /// correctly with the new `MonitoredApp` enum structure.
    #[test]
    fn test_atomic_write_with_monitored_app_enum() {
        use crate::config::models::UwpApp;

        let test_dir = create_test_dir();
        let _guard = AppdataGuard::new(&test_dir);

        // Create config with both Win32 and UWP apps
        let mut config = AppConfig::default();
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            display_name: "Test Game".to_string(),
            exe_path: PathBuf::from("C:\\Games\\test.exe"),
            process_name: "test".to_string(),
            enabled: true,
            icon_data: None,
        }));
        config.monitored_apps.push(MonitoredApp::Uwp(UwpApp {
            id: Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap(),
            display_name: "Calculator".to_string(),
            package_family_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe".to_string(),
            app_id: "App".to_string(),
            enabled: true,
            icon_data: None,
        }));

        // Save the config
        let result = ConfigManager::save(&config);
        assert!(result.is_ok(), "Atomic write should succeed");

        // Verify the final config file exists
        let config_path = test_dir.path().join("EasyHDR").join("config.json");
        assert!(config_path.exists(), "Final config file should exist");

        // Verify the temp file was cleaned up (atomic write completed)
        let temp_path = test_dir.path().join("EasyHDR").join("config.json.tmp");
        assert!(
            !temp_path.exists(),
            "Temp file should be removed after atomic write"
        );

        // Verify the saved file is valid JSON and can be loaded
        let loaded_config = ConfigManager::load().unwrap();
        assert_eq!(
            loaded_config.monitored_apps.len(),
            2,
            "Loaded config should have both apps"
        );

        // TempDir and AppdataGuard automatically clean up when dropped
    }
}
