// SPDX-License-Identifier: MIT
use super::*;
use crate::platform::fake;
use std::ffi::CStr;

fn values() -> [OptionValue; FIELD_COUNT] {
    [OptionValue::default(); FIELD_COUNT]
}

fn int(value: i64) -> OptionValue {
    OptionValue {
        present: 1,
        variant: VARIANT_INT,
        integer: value,
        real: 0.0,
    }
}

fn index(name: &CStr) -> usize {
    FIELDS
        .0
        .iter()
        // SAFETY: the table holds static C strings.
        .position(|field| unsafe { CStr::from_ptr(field.name) } == name)
        .expect("schema field")
}

#[test]
fn absent_options_keep_the_defaults() {
    let settings = compose(&values()).expect("defaults");
    let mut expected = default_settings();
    expected.sharpening = fake::sharpening_defaults();
    assert_eq!(settings, expected);
}

#[test]
fn checked_fields_accept_int_and_bool_in_range() {
    let mut input = values();
    input[index(c"sr_backend")] = int(3);
    input[index(c"frame_limit_us")] = int(i64::from(u32::MAX));
    input[index(c"gameplay")] = OptionValue {
        present: 1,
        variant: VARIANT_BOOL,
        integer: 0,
        real: 0.0,
    };
    let settings = compose(&input).expect("accepted");
    assert_eq!(settings.sr_backend, 3);
    assert_eq!(settings.frame_limit_us, u32::MAX);
    assert_eq!(settings.gameplay, 0);
}

#[test]
fn checked_fields_refuse_other_types_and_ranges() {
    for (value, name) in [
        (int(-1), c"quality"),
        (int(i64::from(u32::MAX) + 1), c"generated_frames"),
        (
            OptionValue {
                present: 1,
                variant: 0,
                integer: 1,
                real: 1.0,
            },
            c"fg_mode",
        ),
    ] {
        let mut input = values();
        input[index(name)] = value;
        assert!(compose(&input).is_none());
    }
}

#[test]
fn sharpening_fields_convert_without_checks() {
    let mut input = values();
    input[index(c"sharpening_mode")] = OptionValue {
        present: 1,
        variant: 0,
        integer: (1 << 32) + 3,
        real: 0.0,
    };
    input[index(c"sharpening_sharpness")] = OptionValue {
        present: 1,
        variant: 0,
        integer: 0,
        real: 0.75,
    };
    let settings = compose(&input).expect("accepted");
    assert_eq!(settings.sharpening.mode, 3);
    assert_eq!(settings.sharpening.sharpness, 0.75);
    assert_eq!(settings.sharpening.depth_scale, 35.0);
}

#[test]
fn schema_names_are_unique_and_typed() {
    for (position, field) in FIELDS.0.iter().enumerate() {
        // SAFETY: the table holds static C strings.
        let name = unsafe { CStr::from_ptr(field.name) };
        assert_eq!(index(name), position);
        let real = matches!(
            name.to_bytes(),
            b"sharpening_sharpness"
                | b"sharpening_motion_sharpness"
                | b"sharpening_motion_threshold"
                | b"sharpening_motion_scale_limit"
                | b"sharpening_contrast"
                | b"sharpening_depth_scale"
                | b"sharpening_depth_bias"
        );
        assert_eq!(field.conversion, if real { REAL } else { INTEGER });
    }
}

fn name(value: &[u8]) -> Option<&[u8]> {
    Some(value)
}

#[test]
fn renderer_admission() {
    let both = ADMIT_DRIVER | ADMIT_RENDERER;
    assert_eq!(
        admission(name(b"d3d12"), name(b"forward_plus"), false),
        both
    );
    assert_eq!(
        admission(name(b"vulkan"), name(b"forward_plus"), true),
        both
    );
    assert_eq!(
        admission(name(b"vulkan"), name(b"forward_plus"), false),
        ADMIT_RENDERER
    );
    assert_eq!(
        admission(name(b"opengl3"), name(b"gl_compatibility"), true),
        0
    );
    assert_eq!(
        admission(name(b"d3d12"), name(b"mobile"), true),
        ADMIT_DRIVER
    );
    assert_eq!(admission(None, None, true), 0);
}
