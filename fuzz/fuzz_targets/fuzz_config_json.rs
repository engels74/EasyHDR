#![no_main]

use libfuzzer_sys::fuzz_target;
use easyhdr::config::AppConfig;

fuzz_target!(|data: &[u8]| {
    // Try to parse arbitrary bytes as JSON into AppConfig
    // This tests for crashes, panics, and undefined behavior
    if let Ok(s) = std::str::from_utf8(data) {
        let _result: Result<AppConfig, _> = serde_json::from_str(s);
        // We don't care if parsing fails, we just want to ensure it doesn't crash
    }
});
