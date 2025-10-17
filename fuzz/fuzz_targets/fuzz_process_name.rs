#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;

fuzz_target!(|data: &[u8]| {
    // Try to create a PathBuf from arbitrary bytes and extract process name
    // This tests the process name extraction logic for edge cases
    if let Ok(s) = std::str::from_utf8(data) {
        let path = PathBuf::from(s);

        // This is the same logic used in MonitoredApp::from_exe_path
        let _process_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase());

        // We don't care about the result, just that it doesn't crash
    }
});
