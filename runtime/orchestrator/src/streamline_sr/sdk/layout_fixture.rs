// SPDX-License-Identifier: MIT
//! Compares the Streamline records and constants Rust writes with the vendor C++ declarations.
//! `sl.h` cannot be read from Rust, so tests/dlss_sr_layout_facts.cpp constructs the default SDK
//! objects and reports the SDK's own numbers; this fixture holds the expectations. It covers the
//! struct types and versions in every record header, the defaults Rust leaves in place, and each
//! enum and buffer-type value the SR code carries as a literal.
use super::super::{
    COLOR_BEFORE_TRANSPARENCY, COLOR_IN, COLOR_OUT, DEPTH, EXPOSURE, MOTION,
    NON_PIXEL_SHADER_RESOURCE, ONLY_VALID_NOW, TRANSPARENCY_HINT, TRANSPARENCY_LAYER,
    UNORDERED_ACCESS, dlss_mode,
};
use super::*;
use crate::vulkan_streamline_camera::rsf_slvk_sdk_preferences;
use std::mem::offset_of;

const PREFERENCES_KIND: u32 = 0;
const CONSTANTS_KIND: u32 = 1;
const OPTIONS_KIND: u32 = 2;
const OPTIMAL_KIND: u32 = 3;
const REQUIREMENTS_KIND: u32 = 4;
const ADAPTER_KIND: u32 = 5;
const VIEWPORT_KIND: u32 = 6;
const RESOURCE_KIND: u32 = 7;
const TAG_KIND: u32 = 8;

/// Native entry points of the C++ facts translation unit.
#[repr(C)]
pub struct Facts {
    /// Copies the bytes of a default-constructed SDK object, returns its size.
    default_object: unsafe extern "C" fn(u32, *mut u8, usize) -> usize,
    /// Fills the SDK's numeric values in the order of `expected_values`, returns the count.
    values: unsafe extern "C" fn(*mut u64, usize) -> usize,
}

fn bytes_of<T>(value: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(std::ptr::from_ref(value).cast(), size_of::<T>()) }
}

fn read<T: Copy>(bytes: &[u8], offset: usize) -> T {
    assert!(offset + size_of::<T>() <= bytes.len());
    unsafe { bytes.as_ptr().add(offset).cast::<T>().read_unaligned() }
}

/// The SDK's numbers, in the order tests/dlss_sr_layout_facts.cpp reports them.
fn expected_values() -> Vec<(&'static str, u64)> {
    let mode = |quality| u64::from(dlss_mode(quality));
    vec![
        ("kSDKVersion", SDK_VERSION),
        ("kFeatureDLSS", u64::from(FEATURE_DLSS)),
        ("kFeatureCommon", u64::from(FEATURE_COMMON)),
        (
            "PreferenceFlags::eUseManualHooking",
            PREFERENCE_MANUAL_HOOKING,
        ),
        (
            "PreferenceFlags::eUseFrameBasedResourceTagging",
            PREFERENCE_FRAME_TAGGING,
        ),
        ("default preference flags", DEFAULT_PREFERENCE_FLAGS),
        (
            "EngineType::eCustom",
            u64::from(crate::streamline_policy::engine_type(0)),
        ),
        (
            "EngineType::eUnreal",
            u64::from(crate::streamline_policy::engine_type(1)),
        ),
        (
            "EngineType::eUnity",
            u64::from(crate::streamline_policy::engine_type(2)),
        ),
        ("RenderAPI::eD3D11", u64::from(RENDER_API_D3D11)),
        ("RenderAPI::eD3D12", u64::from(RENDER_API_D3D12)),
        ("LogLevel::eDefault", u64::from(LOG_LEVEL_DEFAULT)),
        ("LogType::eWarn", u64::from(LOG_WARN)),
        ("LogType::eError", u64::from(LOG_ERROR)),
        ("Result::eOk", RESULT_OK as u64),
        (
            "Result::eErrorDriverOutOfDate",
            u64::from(RESULT_DRIVER_OUT_OF_DATE),
        ),
        (
            "Result::eErrorOSOutOfDate",
            u64::from(RESULT_OS_OUT_OF_DATE),
        ),
        (
            "Result::eErrorNoSupportedAdapterFound",
            u64::from(RESULT_NO_SUPPORTED_ADAPTER),
        ),
        ("DLSSMode::eDLAA", mode(0)),
        ("DLSSMode::eMaxQuality", mode(1)),
        ("DLSSMode::eBalanced", mode(2)),
        ("DLSSMode::eMaxPerformance", mode(3)),
        ("DLSSMode::eUltraPerformance", mode(4)),
        ("DLSSMode::eOff", mode(5)),
        ("kBufferTypeScalingInputColor", u64::from(COLOR_IN)),
        ("kBufferTypeScalingOutputColor", u64::from(COLOR_OUT)),
        ("kBufferTypeDepth", u64::from(DEPTH)),
        ("kBufferTypeMotionVectors", u64::from(MOTION)),
        ("kBufferTypeExposure", u64::from(EXPOSURE)),
        ("kBufferTypeTransparencyHint", u64::from(TRANSPARENCY_HINT)),
        (
            "kBufferTypeBiasCurrentColorHint",
            u64::from(super::super::BIAS_CURRENT_COLOR),
        ),
        (
            "kBufferTypeColorBeforeTransparency",
            u64::from(COLOR_BEFORE_TRANSPARENCY),
        ),
        (
            "kBufferTypeTransparencyLayer",
            u64::from(TRANSPARENCY_LAYER),
        ),
        (
            "ResourceLifecycle::eOnlyValidNow",
            u64::from(ONLY_VALID_NOW),
        ),
        ("ResourceType::eTex2d", 0),
        (
            "D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE",
            u64::from(NON_PIXEL_SHADER_RESOURCE),
        ),
        (
            "D3D12_RESOURCE_STATE_UNORDERED_ACCESS",
            u64::from(UNORDERED_ACCESS),
        ),
        ("Boolean::eFalse", 0),
        ("Boolean::eTrue", 1),
    ]
}

struct Report(bool);

impl Report {
    fn check(&mut self, passed: bool, what: impl FnOnce() -> String) {
        if !passed {
            eprintln!("dlss_sr_layout: {}", what());
            self.0 = false;
        }
    }
}

fn native_default(facts: &Facts, kind: u32) -> Vec<u8> {
    let size = unsafe { (facts.default_object)(kind, null_mut(), 0) };
    let mut bytes = vec![0u8; size];
    let written = unsafe { (facts.default_object)(kind, bytes.as_mut_ptr(), bytes.len()) };
    assert_eq!(written, size, "native default object {kind} changed size");
    bytes
}

/// The record header (next pointer, struct type GUID, struct version) must match the SDK's.
fn check_record<T>(report: &mut Report, name: &str, native: &[u8], record: &T) {
    report.check(native.len() == size_of::<T>(), || {
        format!(
            "{name}: SDK object is {} bytes, Rust record {}",
            native.len(),
            size_of::<T>()
        )
    });
    let header = size_of::<Header>();
    report.check(
        native.len() >= header && native[..header] == bytes_of(record)[..header],
        || format!("{name}: header (struct type or version) differs from the SDK constructor"),
    );
}

/// Runs the checks. Returns 0 when Rust and the vendor declarations agree.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_dlss_sr_layout(facts: &Facts) -> i32 {
    let mut report = Report(true);

    let expected = expected_values();
    let mut actual = vec![0u64; expected.len() + 8];
    let count = unsafe { (facts.values)(actual.as_mut_ptr(), actual.len()) };
    report.check(count == expected.len(), || {
        format!(
            "SDK reported {count} values, Rust expects {}",
            expected.len()
        )
    });
    for ((name, want), got) in expected.iter().zip(&actual[..count.min(expected.len())]) {
        report.check(want == got, || format!("{name}: SDK {got}, Rust {want}"));
    }

    let preferences = native_default(facts, PREFERENCES_KIND);
    check_record(
        &mut report,
        "Preferences",
        &preferences,
        &rsf_slvk_sdk_preferences {
            base: PREFERENCES,
            ..Default::default()
        },
    );
    report.check(
        read::<u64>(&preferences, offset_of!(rsf_slvk_sdk_preferences, flags))
            == DEFAULT_PREFERENCE_FLAGS
            && read::<u32>(
                &preferences,
                offset_of!(rsf_slvk_sdk_preferences, log_level),
            ) == LOG_LEVEL_DEFAULT
            && read::<u8>(
                &preferences,
                offset_of!(rsf_slvk_sdk_preferences, show_console),
            ) == 0,
        || "Preferences: default flags, log level or console flag differ".to_owned(),
    );

    let constants_native = native_default(facts, CONSTANTS_KIND);
    let constants = constants(&Constants::default());
    check_record(&mut report, "Constants", &constants_native, &constants);
    let lens = offset_of!(SdkConstants, clip_to_lens);
    report.check(
        constants_native[lens..lens + size_of::<[f32; 16]>()]
            == bytes_of(&constants.clip_to_lens)[..],
        || "Constants: clipToLensClip default differs".to_owned(),
    );
    for (name, offset, rust) in [
        (
            "orthographicProjection",
            offset_of!(SdkConstants, orthographic),
            constants.orthographic,
        ),
        (
            "motionVectorsDilated",
            offset_of!(SdkConstants, motion_dilated),
            constants.motion_dilated,
        ),
        (
            "motionVectorsJittered",
            offset_of!(SdkConstants, motion_jittered),
            constants.motion_jittered,
        ),
    ] {
        report.check(read::<u8>(&constants_native, offset) == rust, || {
            format!("Constants::{name} default differs")
        });
    }
    report.check(
        read::<f32>(
            &constants_native,
            offset_of!(SdkConstants, min_relative_depth_separation),
        ) == constants.min_relative_depth_separation,
        || "Constants: minRelativeLinearDepthObjectSeparation default differs".to_owned(),
    );

    let options_native = native_default(facts, OPTIONS_KIND);
    let options = plan_options(&Dimensions::default());
    check_record(&mut report, "DLSSOptions", &options_native, &options);
    report.check(
        read::<f32>(&options_native, offset_of!(DlssOptions, sharpness)) == options.sharpness
            && read::<f32>(&options_native, offset_of!(DlssOptions, pre_exposure))
                == options.pre_exposure
            && read::<f32>(&options_native, offset_of!(DlssOptions, exposure_scale))
                == options.exposure_scale
            && read::<u8>(&options_native, offset_of!(DlssOptions, hdr)) == options.hdr
            && read::<u8>(&options_native, offset_of!(DlssOptions, invert_axis_x))
                == options.invert_axis_x
            && read::<u8>(&options_native, offset_of!(DlssOptions, invert_axis_y))
                == options.invert_axis_y
            && read::<[u32; 6]>(&options_native, offset_of!(DlssOptions, presets))
                == options.presets
            && read::<u8>(&options_native, offset_of!(DlssOptions, auto_exposure))
                == options.auto_exposure
            && read::<u8>(&options_native, offset_of!(DlssOptions, alpha)) == options.alpha,
        || "DLSSOptions: a default Rust leaves in place differs".to_owned(),
    );

    check_record(
        &mut report,
        "DLSSOptimalSettings",
        &native_default(facts, OPTIMAL_KIND),
        &dlss_optimal(),
    );
    check_record(
        &mut report,
        "FeatureRequirements",
        &native_default(facts, REQUIREMENTS_KIND),
        &requirements(),
    );
    let mut luid = 0u64;
    check_record(
        &mut report,
        "AdapterInfo",
        &native_default(facts, ADAPTER_KIND),
        &adapter(&mut luid),
    );
    let viewport_native = native_default(facts, VIEWPORT_KIND);
    let viewport = viewport(0);
    check_record(&mut report, "ViewportHandle", &viewport_native, &viewport);
    report.check(
        read::<u32>(&viewport_native, offset_of!(Viewport, value)) == viewport.value,
        || "ViewportHandle: value offset differs".to_owned(),
    );

    let mut tags = TagRecords::new();
    let mut batch = TagBatch::d3d12(&unsafe { std::mem::zeroed() }, &unsafe {
        std::mem::zeroed()
    });
    batch.count = 1;
    tags.fill(&batch);
    let resource_native = native_default(facts, RESOURCE_KIND);
    check_record(
        &mut report,
        "Resource",
        &resource_native,
        &tags.resources[0],
    );
    report.check(
        read::<u8>(&resource_native, offset_of!(Resource, type_)) == tags.resources[0].type_,
        || "Resource: type default differs".to_owned(),
    );
    check_record(
        &mut report,
        "ResourceTag",
        &native_default(facts, TAG_KIND),
        &tags.tags[0],
    );

    i32::from(!report.0)
}
