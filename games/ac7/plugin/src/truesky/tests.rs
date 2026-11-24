// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn module_and_effect_fingerprints() {
    assert_eq!(
        String::from_utf16(&MODULE).unwrap(),
        "TrueSkyPluginRender_MT.dll\0"
    );
    assert!(!effect_matches(&[]));
    assert!(!effect_matches(&[0; EFFECT_BYTES - 1]));
    assert!(EFFECT_PATH.ends_with(r"\Win64\mixed_resolution.fxo"));
}

#[test]
fn site_bytes_agree_with_their_guards() {
    assert_eq!(RESOLUTION_EXPECTED[..6], NATIVE_DIVISOR);
    assert_eq!(
        DEPTH_BRANCH_EXPECTED[DEPTH_BRANCH_LEAD..=DEPTH_BRANCH_LEAD],
        NATIVE_BRANCH
    );
    assert_eq!(
        DEPTH_FORMAT_EXPECTED[DEPTH_FORMAT_IMMEDIATE..DEPTH_FORMAT_IMMEDIATE + 4],
        NATIVE_DEPTH_FORMAT
    );
    assert_eq!(
        (RESOLUTION_RVA, DEPTH_CALL_RVA, DEPTH_BRANCH_RVA),
        (0x87f6d, 0xc003f, 0xbff0b)
    );
    assert_eq!(
        (
            DEPTH_FORMAT_RVA + DEPTH_FORMAT_IMMEDIATE as u32,
            MOTION_CALL_RVA
        ),
        (0xbfc09, 0xabc6b)
    );
    assert_eq!((DEPTH_CALL_BYTES, MOTION_CALL_BYTES), (7, 6));
}
