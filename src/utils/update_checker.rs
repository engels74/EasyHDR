//! Update checker for `EasyHDR`
//!
//! This module provides functionality to check for application updates from GitHub releases.
//! It implements rate limiting, caching, and graceful error handling.

use crate::error::{EasyHdrError, Result};
use semver::Version;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// GitHub API response for a release
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    /// Release tag name (e.g., "v1.2.3" or "1.2.3")
    tag_name: String,
    /// Release name (not currently used, but part of API response)
    #[expect(dead_code)]
    name: String,
    /// Whether this is a prerelease
    prerelease: bool,
}

/// Result of an update check
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckResult {
    /// Current version of the application
    pub current_version: Version,
    /// Latest version available on GitHub
    pub latest_version: Version,
    /// Whether an update is available
    pub update_available: bool,
    /// URL to the releases page
    pub releases_url: String,
}

/// Update checker for `EasyHDR`
pub struct UpdateChecker {
    /// GitHub repository owner
    repo_owner: String,
    /// GitHub repository name
    repo_name: String,
    /// Current application version
    current_version: Version,
    /// Minimum time between checks in seconds (rate limiting)
    min_check_interval_secs: u64,
}

impl UpdateChecker {
    /// Create a new update checker
    ///
    /// # Arguments
    ///
    /// * `repo_owner` - GitHub repository owner (e.g., "engels74")
    /// * `repo_name` - GitHub repository name (e.g., "`EasyHDR`")
    /// * `current_version` - Current application version
    /// * `min_check_interval_secs` - Minimum time between checks in seconds (default: 60)
    pub fn new(
        repo_owner: impl Into<String>,
        repo_name: impl Into<String>,
        current_version: Version,
        min_check_interval_secs: u64,
    ) -> Self {
        Self {
            repo_owner: repo_owner.into(),
            repo_name: repo_name.into(),
            current_version,
            min_check_interval_secs,
        }
    }

    /// Check if enough time has passed since the last check (rate limiting)
    ///
    /// # Arguments
    ///
    /// * `last_check_time` - Unix timestamp of the last check (0 if never checked)
    ///
    /// # Returns
    ///
    /// `true` if enough time has passed, `false` otherwise
    pub fn should_check(&self, last_check_time: u64) -> bool {
        if last_check_time == 0 {
            return true; // Never checked before
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let elapsed = now.saturating_sub(last_check_time);
        elapsed >= self.min_check_interval_secs
    }

    /// Get the current Unix timestamp in seconds
    pub fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Check for updates from GitHub releases
    ///
    /// This method:
    /// - Fetches the latest release from GitHub API
    /// - Compares versions using semantic versioning
    /// - Returns an `UpdateCheckResult` if successful
    /// - Fails silently on network errors (returns `Err`)
    ///
    /// # Returns
    ///
    /// - `Ok(UpdateCheckResult)` if the check succeeded
    /// - `Err(EasyHdrError)` if the check failed (network error, parse error, etc.)
    pub fn check_for_updates(&self) -> Result<UpdateCheckResult> {
        info!("Checking for updates from GitHub");

        // Build GitHub API URL
        let api_url = format!(
            "https://api.github.com/repos/{}/{}/releases/latest",
            self.repo_owner, self.repo_name
        );

        debug!("Fetching latest release from: {}", api_url);

        // Create HTTP client with timeout
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent(format!("EasyHDR/{}", self.current_version))
            .build()
            .map_err(|e| {
                warn!("Failed to create HTTP client: {}", e);
                // Preserve error chain by wrapping the source error
                EasyHdrError::ConfigError(Box::new(e))
            })?;

        // Fetch the latest release
        let response = client.get(&api_url).send().map_err(|e| {
            warn!("Failed to fetch latest release: {}", e);
            // Preserve error chain by wrapping the source error
            EasyHdrError::ConfigError(Box::new(e))
        })?;

        // Check HTTP status
        if !response.status().is_success() {
            warn!("GitHub API returned error status: {}", response.status());
            return Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                format!("GitHub API returned error status: {}", response.status()),
            )));
        }

        // Parse JSON response
        let release: GitHubRelease = response.json().map_err(|e| {
            warn!("Failed to parse GitHub API response: {}", e);
            // Preserve error chain by wrapping the source error
            EasyHdrError::ConfigError(Box::new(e))
        })?;

        debug!("Fetched release: {:?}", release);

        // Skip prereleases
        if release.prerelease {
            info!("Latest release is a prerelease, skipping");
            return Err(EasyHdrError::ConfigError(crate::error::StringError::new(
                "Latest release is a prerelease",
            )));
        }

        // Parse version from tag name (strip leading 'v' if present)
        let tag_name = release.tag_name.trim_start_matches('v');
        let latest_version = Version::parse(tag_name).map_err(|e| {
            warn!("Failed to parse version from tag '{}': {}", tag_name, e);
            // Preserve error chain by wrapping the source error
            EasyHdrError::ConfigError(Box::new(e))
        })?;

        info!(
            "Current version: {}, Latest version: {}",
            self.current_version, latest_version
        );

        // Compare versions
        let update_available = latest_version > self.current_version;

        if update_available {
            info!(
                "Update available: {} -> {}",
                self.current_version, latest_version
            );
        } else {
            info!("Application is up to date");
        }

        Ok(UpdateCheckResult {
            current_version: self.current_version.clone(),
            latest_version,
            update_available,
            releases_url: format!(
                "https://github.com/{}/{}/releases",
                self.repo_owner, self.repo_name
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_check_never_checked() {
        let checker =
            UpdateChecker::new("engels74", "EasyHDR", Version::parse("0.1.0").unwrap(), 60);

        // Should check if never checked before (last_check_time = 0)
        assert!(checker.should_check(0));
    }

    #[test]
    fn test_should_check_rate_limiting() {
        let checker =
            UpdateChecker::new("engels74", "EasyHDR", Version::parse("0.1.0").unwrap(), 60);

        let now = UpdateChecker::current_timestamp();

        // Should not check if last check was recent
        assert!(!checker.should_check(now));

        // Should check if last check was more than min_check_interval_secs ago
        let old_timestamp = now.saturating_sub(61);
        assert!(checker.should_check(old_timestamp));
    }

    #[test]
    fn test_version_comparison() {
        let current = Version::parse("0.1.0").unwrap();
        let latest = Version::parse("0.2.0").unwrap();

        assert!(latest > current);
    }

    #[test]
    fn test_version_parsing_with_v_prefix() {
        let tag_with_v = "v1.2.3";
        let tag_without_v = "1.2.3";

        let version_with_v = Version::parse(tag_with_v.trim_start_matches('v')).unwrap();
        let version_without_v = Version::parse(tag_without_v).unwrap();

        assert_eq!(version_with_v, version_without_v);
    }
}
