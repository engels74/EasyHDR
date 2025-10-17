#![no_main]

use libfuzzer_sys::fuzz_target;
use easyhdr::hdr::windows_api::{
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2,
    DISPLAYCONFIG_SET_HDR_STATE,
    DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE,
    DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_DEVICE_INFO_TYPE,
    LUID,
};

fuzz_target!(|data: &[u8]| {
    // Need at least 4 bytes for u32 value testing
    if data.len() < 4 {
        return;
    }

    // Parse arbitrary bytes as u32 for bit field testing
    let value = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);

    // Test DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO with arbitrary bit field value
    // This tests bit field extraction logic with all possible u32 values
    let info1 = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
            adapterId: LUID { LowPart: 0, HighPart: 0 },
            id: 0,
        },
        value,
        colorEncoding: 0,
        bitsPerColorChannel: 0,
    };

    // Exercise all bit field accessor methods
    let _ = info1.advancedColorSupported();
    let _ = info1.advancedColorEnabled();
    let _ = info1.wideColorEnforced();
    let _ = info1.advancedColorForceDisabled();

    // Test DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 with arbitrary bit field value
    // This is the critical structure with strict field ordering requirements
    let info2 = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            type_: DISPLAYCONFIG_DEVICE_INFO_TYPE::DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2,
            size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>() as u32,
            adapterId: LUID { LowPart: 0, HighPart: 0 },
            id: 0,
        },
        value,
        colorEncoding: 0,
        bitsPerColorChannel: 0,
        activeColorMode: 0,
    };

    // Exercise all bit field accessor methods for INFO_2
    let _ = info2.advancedColorSupported();
    let _ = info2.advancedColorActive();
    let _ = info2.advancedColorLimitedByPolicy();
    let _ = info2.highDynamicRangeSupported();
    let _ = info2.highDynamicRangeUserEnabled();
    let _ = info2.wideColorGamutSupported();
    let _ = info2.wideColorUserEnabled();

    // If we have enough data, test structure creation with arbitrary adapter/target IDs
    if data.len() >= 16 {
        let adapter_low = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let adapter_high = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let target_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

        let luid = LUID {
            LowPart: adapter_low,
            HighPart: adapter_high,
        };

        // Test SET_ADVANCED_COLOR_STATE construction (legacy)
        let _ = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE::new(luid, target_id, true);
        let _ = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE::new(luid, target_id, false);

        // Test SET_HDR_STATE construction (Windows 11 24H2+)
        let _ = DISPLAYCONFIG_SET_HDR_STATE::new(luid, target_id, true);
        let _ = DISPLAYCONFIG_SET_HDR_STATE::new(luid, target_id, false);
    }

    // Verify structure sizes remain consistent (critical for FFI)
    // These assertions ensure field ordering is correct
    assert_eq!(
        std::mem::size_of::<DISPLAYCONFIG_DEVICE_INFO_HEADER>(),
        20,
        "DISPLAYCONFIG_DEVICE_INFO_HEADER size must be 20 bytes"
    );
    assert_eq!(
        std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(),
        32,
        "DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO size must be 32 bytes"
    );
    assert_eq!(
        std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2>(),
        36,
        "DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 size must be 36 bytes"
    );
});
