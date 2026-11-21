// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn report_matches_former_msvc_receipt() {
    assert_eq!(
        report(0, 0, 0x0000_01f2_a3b4_c5d6),
        "Skyrim native creation scoped-end native_HRESULT=0x00000000 admission=0 outer=000001F2A3B4C5D6; outer outputs and wrapper Present preserved"
    );
    assert_eq!(
        report(0x8000_4005_u32 as i32, -3, 0),
        "Skyrim native creation scoped-end native_HRESULT=0x80004005 admission=-3 outer=0000000000000000; outer outputs and wrapper Present preserved"
    );
}

#[test]
fn lifecycle_before_prepare_refuses_start_and_stops_cleanly() {
    // No trampoline or table exists before prepare; stop has nothing to disable.
    assert!(!start());
    assert!(stop());
    assert!(!busy());
}

#[test]
fn description_window_is_within_description() {
    assert!(DESCRIPTION_WINDOW + size_of::<usize>() <= DESCRIPTION_SIZE);
}
