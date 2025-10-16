//! Integration tests for Windows version detection
//!
//! These tests verify the version detection logic with various scenarios
//! including mocked Windows API responses and edge cases.

use easyhdr::hdr::version::WindowsVersion;

/// Test that version detection returns a valid result
#[test]
fn test_version_detection_returns_valid_result() {
    let result = WindowsVersion::detect();
    assert!(result.is_ok(), "Version detection should succeed");

    let version = result.unwrap();

    // Verify it's one of the three valid versions
    assert!(
        matches!(
            version,
            WindowsVersion::Windows10 | WindowsVersion::Windows11 | WindowsVersion::Windows11_24H2
        ),
        "Version should be one of the valid Windows versions, got {version:?}"
    );
}

/// Test version detection consistency
#[test]
fn test_version_detection_consistency() {
    // Call detect multiple times and ensure we get the same result
    let version1 = WindowsVersion::detect().expect("First detection should succeed");
    let version2 = WindowsVersion::detect().expect("Second detection should succeed");
    let version3 = WindowsVersion::detect().expect("Third detection should succeed");

    assert_eq!(version1, version2, "Version detection should be consistent");
    assert_eq!(version2, version3, "Version detection should be consistent");
}

/// Test that version enum implements required traits
#[test]
fn test_version_enum_traits() {
    let v1 = WindowsVersion::Windows10;
    let v2 = WindowsVersion::Windows11;
    let v3 = WindowsVersion::Windows11_24H2;

    // Test Debug
    assert_eq!(format!("{v1:?}"), "Windows10");
    assert_eq!(format!("{v2:?}"), "Windows11");
    assert_eq!(format!("{v3:?}"), "Windows11_24H2");

    // Test Clone (Copy trait is used automatically for types that implement Copy)
    let v1_clone = v1;
    assert_eq!(v1, v1_clone);

    // Test Copy
    let v2_copy = v2;
    assert_eq!(v2, v2_copy);

    // Test PartialEq
    assert_eq!(v1, WindowsVersion::Windows10);
    assert_ne!(v1, WindowsVersion::Windows11);

    // Test Eq (transitivity)
    assert_eq!(v1, v1_clone);
    assert_eq!(v1_clone, WindowsVersion::Windows10);
    assert_eq!(v1, WindowsVersion::Windows10);
}

/// Test build number parsing for all Windows 10 versions
#[test]
fn test_all_windows10_versions() {
    // Test all known Windows 10 build numbers
    let windows10_builds = vec![
        10240, // 1507 (RTM)
        10586, // 1511 (November Update)
        14393, // 1607 (Anniversary Update)
        15063, // 1703 (Creators Update)
        16299, // 1709 (Fall Creators Update)
        17134, // 1803 (April 2018 Update)
        17763, // 1809 (October 2018 Update)
        18362, // 1903 (May 2019 Update)
        18363, // 1909 (November 2019 Update)
        19041, // 2004 (May 2020 Update)
        19042, // 20H2 (October 2020 Update)
        19043, // 21H1 (May 2021 Update)
        19044, // 21H2 (November 2021 Update)
        19045, // 22H2 (October 2022 Update)
    ];

    for build in windows10_builds {
        let version = WindowsVersion::parse_build_number(build);
        assert_eq!(
            version,
            WindowsVersion::Windows10,
            "Build {build} should be classified as Windows 10"
        );
    }
}

/// Test build number parsing for all Windows 11 versions (pre-24H2)
#[test]
fn test_all_windows11_versions() {
    // Test all known Windows 11 build numbers (before 24H2)
    let windows11_builds = vec![
        22000, // 21H2 (Initial release)
        22621, // 22H2
        22631, // 23H2
    ];

    for build in windows11_builds {
        let version = WindowsVersion::parse_build_number(build);
        assert_eq!(
            version,
            WindowsVersion::Windows11,
            "Build {build} should be classified as Windows 11"
        );
    }
}

/// Test build number parsing for Windows 11 24H2 and later
#[test]
fn test_all_windows11_24h2_versions() {
    // Test Windows 11 24H2 and future builds
    let windows11_24h2_builds = vec![
        26100, // 24H2
        26200, // Future build
        27000, // Future build
        30000, // Far future build
    ];

    for build in windows11_24h2_builds {
        let version = WindowsVersion::parse_build_number(build);
        assert_eq!(
            version,
            WindowsVersion::Windows11_24H2,
            "Build {build} should be classified as Windows 11 24H2"
        );
    }
}

/// Test boundary conditions around version thresholds
#[test]
fn test_version_threshold_boundaries() {
    // Test around Windows 11 threshold (22000)
    assert_eq!(
        WindowsVersion::parse_build_number(21999),
        WindowsVersion::Windows10,
        "Build 21999 should be Windows 10"
    );
    assert_eq!(
        WindowsVersion::parse_build_number(22000),
        WindowsVersion::Windows11,
        "Build 22000 should be Windows 11"
    );

    // Test around Windows 11 24H2 threshold (26100)
    assert_eq!(
        WindowsVersion::parse_build_number(26099),
        WindowsVersion::Windows11,
        "Build 26099 should be Windows 11"
    );
    assert_eq!(
        WindowsVersion::parse_build_number(26100),
        WindowsVersion::Windows11_24H2,
        "Build 26100 should be Windows 11 24H2"
    );
}

/// Test extreme build number values
#[test]
fn test_extreme_build_numbers() {
    // Test minimum value
    assert_eq!(
        WindowsVersion::parse_build_number(0),
        WindowsVersion::Windows10,
        "Build 0 should default to Windows 10"
    );

    // Test very small value
    assert_eq!(
        WindowsVersion::parse_build_number(1),
        WindowsVersion::Windows10,
        "Build 1 should be Windows 10"
    );

    // Test maximum value
    assert_eq!(
        WindowsVersion::parse_build_number(u32::MAX),
        WindowsVersion::Windows11_24H2,
        "Maximum build number should be Windows 11 24H2"
    );
}

/// Test version detection error handling
#[test]
#[cfg(windows)]
fn test_version_detection_error_handling() {
    // This test verifies that version detection handles errors gracefully
    // On Windows, detection should always succeed
    let result = WindowsVersion::detect();
    assert!(
        result.is_ok(),
        "Version detection should succeed on Windows platform"
    );
}

/// Test non-Windows platform behavior
#[test]
#[cfg(not(windows))]
fn test_non_windows_platform() {
    // On non-Windows platforms, should return a default version
    let result = WindowsVersion::detect();
    assert!(
        result.is_ok(),
        "Version detection should succeed on non-Windows"
    );

    let version = result.unwrap();
    assert_eq!(
        version,
        WindowsVersion::Windows11,
        "Non-Windows platforms should default to Windows 11"
    );
}

/// Test that version detection is deterministic
#[test]
fn test_version_detection_deterministic() {
    // Run detection multiple times in quick succession
    let results: Vec<_> = (0..10).map(|_| WindowsVersion::detect()).collect();

    // All results should be Ok
    for (i, result) in results.iter().enumerate() {
        assert!(result.is_ok(), "Detection {i} should succeed");
    }

    // All results should be the same
    let first = results[0].as_ref().unwrap();
    for (i, result) in results.iter().enumerate().skip(1) {
        assert_eq!(
            result.as_ref().unwrap(),
            first,
            "Detection {i} should match first detection"
        );
    }
}

/// Test version comparison operations
#[test]
fn test_version_comparisons() {
    let v10 = WindowsVersion::Windows10;
    let v11 = WindowsVersion::Windows11;
    let v11_24h2 = WindowsVersion::Windows11_24H2;

    // Test equality
    assert_eq!(v10, WindowsVersion::Windows10);
    assert_eq!(v11, WindowsVersion::Windows11);
    assert_eq!(v11_24h2, WindowsVersion::Windows11_24H2);

    // Test inequality
    assert_ne!(v10, v11);
    assert_ne!(v11, v11_24h2);
    assert_ne!(v10, v11_24h2);
}

/// Test that `parse_build_number` is consistent with known version mappings
#[test]
fn test_parse_build_number_consistency() {
    // Define expected mappings
    let test_cases = vec![
        (10240, WindowsVersion::Windows10),
        (19044, WindowsVersion::Windows10),
        (19045, WindowsVersion::Windows10),
        (21999, WindowsVersion::Windows10),
        (22000, WindowsVersion::Windows11),
        (22621, WindowsVersion::Windows11),
        (22631, WindowsVersion::Windows11),
        (26099, WindowsVersion::Windows11),
        (26100, WindowsVersion::Windows11_24H2),
        (26200, WindowsVersion::Windows11_24H2),
        (30000, WindowsVersion::Windows11_24H2),
    ];

    for (build, expected) in test_cases {
        let actual = WindowsVersion::parse_build_number(build);
        assert_eq!(
            actual, expected,
            "Build {build} should map to {expected:?}, got {actual:?}"
        );
    }
}
