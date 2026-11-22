// SPDX-License-Identifier: MIT
//! Streamline 2.14.1 records for the Direct3D SR callers. The record layouts are checked against the
//! pinned SDK headers by `runtime/backends/dlss/src/dlss_streamline.cpp`; Rust builds them and calls
//! the SDK entry points through typed function pointers.
#[cfg(not(test))]
mod layout_fixture;

use super::records_abi::{
    rsf_sl_sdk_adapter as Adapter, rsf_sl_sdk_dlss_optimal as DlssOptimal,
    rsf_sl_sdk_dlss_options as DlssOptions, rsf_sl_sdk_requirements as Requirements,
    rsf_sl_sdk_viewport as Viewport,
};
use super::{Constants, Dimensions, MAX_TAGS, OptionPayload, TagBatch};
use crate::vulkan_streamline_camera::{
    rsf_slvk_sdk_constants as SdkConstants, rsf_slvk_sdk_header as Header,
    rsf_slvk_sdk_preferences as Preferences, rsf_slvk_sdk_resource as Resource,
    rsf_slvk_sdk_tag as Tag,
};
use std::ffi::c_void;
use std::ptr::null_mut;

type Handle = *mut c_void;

/// `sl::kSDKVersion` for the pinned 2.14.1 headers, passed to `slInit`.
pub(crate) const SDK_VERSION: u64 = 0x0002_000e_0001_fedc;
/// `sl::kFeatureDLSS` and `sl::kFeatureCommon`.
pub(crate) const FEATURE_DLSS: u32 = 0;
pub(crate) const FEATURE_COMMON: u32 = u32::MAX;
/// `sl::Result::eOk`.
pub(crate) const RESULT_OK: i32 = 0;
/// `PreferenceFlags::eUseManualHooking` and `eUseFrameBasedResourceTagging`.
pub(crate) const PREFERENCE_MANUAL_HOOKING: u64 = 1 << 2;
pub(crate) const PREFERENCE_FRAME_TAGGING: u64 = 1 << 7;
/// `RenderAPI::eD3D11` and `eD3D12`, and `LogLevel::eDefault`.
pub(crate) const RENDER_API_D3D11: u32 = 0;
pub(crate) const RENDER_API_D3D12: u32 = 1;
pub(crate) const LOG_LEVEL_DEFAULT: u32 = 1;
/// `Result::eErrorDriverOutOfDate`, `eErrorOSOutOfDate` and `eErrorNoSupportedAdapterFound`.
pub(crate) const RESULT_DRIVER_OUT_OF_DATE: u32 = 2;
pub(crate) const RESULT_OS_OUT_OF_DATE: u32 = 3;
pub(crate) const RESULT_NO_SUPPORTED_ADAPTER: u32 = 6;
/// `LogType::eWarn` and `LogType::eError`.
pub(crate) const LOG_WARN: u32 = 1;
pub(crate) const LOG_ERROR: u32 = 2;
/// `PreferenceFlags::eDisableCLStateTracking | eAllowOTA | eLoadDownloadedPlugins`, the defaults of
/// an `sl::Preferences` that the D3D11 startup extends with its own flags.
pub(crate) const DEFAULT_PREFERENCE_FLAGS: u64 = (1 << 0) | (1 << 3) | (1 << 6);

// Signatures of the SDK entry points, with each C++ reference passed as a pointer.
pub(crate) type InitFn = unsafe extern "C" fn(*const Preferences, u64) -> i32;
pub(crate) type ShutdownFn = unsafe extern "C" fn() -> i32;
pub(crate) type SupportedFn = unsafe extern "C" fn(u32, *const Adapter) -> i32;
pub(crate) type FeatureFn = unsafe extern "C" fn(u32, *const std::ffi::c_char, *mut Handle) -> i32;
pub(crate) type RequirementsFn = unsafe extern "C" fn(u32, *mut Requirements) -> i32;
pub(crate) type SetDeviceFn = unsafe extern "C" fn(Handle) -> i32;
pub(crate) type TokenFn = unsafe extern "C" fn(*mut Handle, *const u32) -> i32;
pub(crate) type ConstantsFn =
    unsafe extern "C" fn(*const SdkConstants, Handle, *const Viewport) -> i32;
pub(crate) type TagsFn =
    unsafe extern "C" fn(Handle, *const Viewport, *const Tag, u32, Handle) -> i32;
pub(crate) type EvaluateFn =
    unsafe extern "C" fn(u32, Handle, *const *const Header, u32, Handle) -> i32;
pub(crate) type FreeFn = unsafe extern "C" fn(u32, *const Viewport) -> i32;
pub(crate) type OptimalFn = unsafe extern "C" fn(*const DlssOptions, *mut DlssOptimal) -> i32;
pub(crate) type OptionsFn = unsafe extern "C" fn(*const Viewport, *const DlssOptions) -> i32;
pub(crate) type HookPresentFn = unsafe extern "C" fn(Handle, u32, u32, *mut bool) -> i32;
pub(crate) type HookAfterPresentFn = unsafe extern "C" fn(u32) -> i32;

const fn header(lo: u32, mid: u16, hi: u16, tail: [u8; 8], version: usize) -> Header {
    Header {
        next: null_mut(),
        type_lo: lo,
        type_mid: mid,
        type_hi: hi,
        type_tail: tail,
        version,
    }
}

pub(crate) const PREFERENCES: Header = header(
    0x1ca1_0965,
    0xbf8e,
    0x432b,
    [0x8d, 0xa1, 0x67, 0x16, 0xd8, 0x79, 0xfb, 0x14],
    1,
);
const CONSTANTS: Header = header(
    0xdcd3_5ad7,
    0x4e4a,
    0x4bad,
    [0xa9, 0x0c, 0xe0, 0xc4, 0x9e, 0xb2, 0x3a, 0xfe],
    2,
);
const DLSS_OPTIONS: Header = header(
    0x6ac8_26e4,
    0x4c61,
    0x4101,
    [0xa9, 0x2d, 0x63, 0x8d, 0x42, 0x10, 0x57, 0xb8],
    3,
);
const DLSS_OPTIMAL: Header = header(
    0xef1d_0957,
    0xfd58,
    0x4df7,
    [0xb5, 0x04, 0x8b, 0x69, 0xd8, 0xaa, 0x6b, 0x76],
    1,
);
const RESOURCE: Header = header(
    0x3a9d_70cf,
    0x2418,
    0x4b72,
    [0x83, 0x91, 0x13, 0xf8, 0x72, 0x1c, 0x72, 0x61],
    1,
);
const TAG: Header = header(
    0x4c6a_5aad,
    0xb445,
    0x496c,
    [0x87, 0xff, 0x1a, 0xf3, 0x84, 0x5b, 0xe6, 0x53],
    1,
);
const VIEWPORT: Header = header(
    0x171b_6435,
    0x9b3c,
    0x4fc8,
    [0x99, 0x94, 0xfb, 0xe5, 0x25, 0x69, 0xaa, 0xa4],
    1,
);
const REQUIREMENTS: Header = header(
    0x6671_4097,
    0xac6d,
    0x4bc6,
    [0x89, 0x15, 0x1e, 0x0f, 0x55, 0xa6, 0xb6, 0x1f],
    2,
);
const ADAPTER: Header = header(
    0x0677_315f,
    0xa746,
    0x4492,
    [0x9f, 0x42, 0xcb, 0x61, 0x42, 0xc9, 0xc3, 0xd4],
    1,
);

pub(crate) fn viewport(value: u32) -> Viewport {
    Viewport {
        base: VIEWPORT,
        value,
    }
}

/// An adapter naming its LUID, which must stay alive for the call.
pub(crate) fn adapter(luid: &mut u64) -> Adapter {
    Adapter {
        base: ADAPTER,
        luid: std::ptr::from_mut(luid).cast(),
        luid_size: size_of::<u64>() as u32,
        vk_physical_device: null_mut(),
    }
}

pub(crate) fn requirements() -> Requirements {
    Requirements {
        base: REQUIREMENTS,
        ..Default::default()
    }
}

pub(crate) fn dlss_optimal() -> DlssOptimal {
    DlssOptimal {
        base: DLSS_OPTIMAL,
        ..Default::default()
    }
}

/// The constructor defaults of `sl::DLSSOptions` with the requested mode and output size.
pub(crate) fn plan_options(dimensions: &Dimensions) -> DlssOptions {
    DlssOptions {
        base: DLSS_OPTIONS,
        mode: dimensions.mode,
        output_width: dimensions.width,
        output_height: dimensions.height,
        sharpness: 0.0,
        pre_exposure: 1.0,
        exposure_scale: 1.0,
        // colorBuffersHDR defaults to eTrue.
        hdr: 1,
        ..Default::default()
    }
}

pub(crate) fn options(input: &OptionPayload) -> DlssOptions {
    DlssOptions {
        presets: input.presets,
        hdr: u8::from(input.hdr != 0),
        auto_exposure: u8::from(input.auto_exposure != 0),
        alpha: u8::from(input.alpha != 0),
        ..plan_options(&input.dimensions)
    }
}

/// The constructor defaults of `sl::Constants` overlaid with the camera and motion values.
pub(crate) fn constants(input: &Constants) -> SdkConstants {
    SdkConstants {
        base: CONSTANTS,
        view_to_clip: input.view_to_clip,
        clip_to_view: input.clip_to_view,
        // clipToLensClip keeps its INVALID_FLOAT rows.
        clip_to_lens: [f32::MAX; 16],
        clip_to_previous: input.clip_to_previous,
        previous_to_clip: input.previous_to_clip,
        jitter: input.jitter,
        motion_scale: input.motion_scale,
        pinhole: input.pinhole,
        position: input.position,
        up: input.up,
        right: input.right,
        forward: input.forward,
        near_plane: input.near_plane,
        far_plane: input.far_plane,
        fov: input.fov,
        aspect: input.aspect,
        invalid_motion: input.invalid_motion,
        depth_inverted: input.depth_inverted,
        motion_includes_camera: input.camera_motion,
        motion_3d: input.motion_3d,
        reset: input.reset,
        orthographic: 0,
        motion_dilated: 0,
        motion_jittered: 0,
        min_relative_depth_separation: 40.0,
    }
}

/// Resource and tag records for one slSetTagForFrame call. `fill` points each tag at its resource,
/// so build the value where it will stay for the call.
pub(crate) struct TagRecords {
    resources: [Resource; MAX_TAGS],
    tags: [Tag; MAX_TAGS],
    count: u32,
}

impl TagRecords {
    pub(crate) fn new() -> Self {
        Self {
            resources: Default::default(),
            tags: Default::default(),
            count: 0,
        }
    }

    pub(crate) fn fill(&mut self, batch: &TagBatch) {
        let count = (batch.count as usize).min(MAX_TAGS);
        self.count = count as u32;
        for (index, entry) in batch.entries[..count].iter().enumerate() {
            self.resources[index] = Resource {
                base: RESOURCE,
                type_: 0, // ResourceType::eTex2d
                image: entry.texture,
                layout: entry.state,
                ..Default::default()
            };
            let (width, height) = if entry.has_extent != 0 {
                (entry.size[0], entry.size[1])
            } else {
                (0, 0)
            };
            self.tags[index] = Tag {
                base: TAG,
                resource: std::ptr::from_mut(&mut self.resources[index]).cast(),
                type_: entry.kind,
                lifecycle: batch.lifecycle,
                top: 0,
                left: 0,
                width,
                height,
            };
        }
    }

    pub(crate) fn tags(&self) -> (*const Tag, u32) {
        (self.tags.as_ptr(), self.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streamline_sr::Options;

    #[test]
    fn constants_keep_unspecified_sdk_defaults() {
        let mut input = Constants::default();
        input.view_to_clip[3] = 2.0;
        input.reset = 1;
        input.camera_motion = 1;
        let record = constants(&input);
        assert_eq!(record.base.version, 2);
        assert_eq!(record.view_to_clip[3], 2.0);
        assert_eq!((record.reset, record.motion_includes_camera), (1, 1));
        assert_eq!(record.clip_to_lens, [f32::MAX; 16]);
        assert_eq!(
            (
                record.orthographic,
                record.motion_dilated,
                record.motion_jittered
            ),
            (0, 0, 0)
        );
        assert_eq!(record.min_relative_depth_separation, 40.0);
    }

    #[test]
    fn option_records_copy_modes_presets_and_flags() {
        let options = Options {
            mode: 3,
            width: 1920,
            height: 1080,
            hdr: 0,
            auto_exposure: 1,
            alpha: 1,
            preset: 11,
        };
        let record = super::options(&options.payload());
        assert_eq!(record.base.version, 3);
        assert_eq!(
            (record.mode, record.output_width, record.output_height),
            (3, 1920, 1080)
        );
        assert_eq!(record.presets, [11; 6]);
        assert_eq!((record.hdr, record.auto_exposure, record.alpha), (0, 1, 1));
        assert_eq!((record.pre_exposure, record.exposure_scale), (1.0, 1.0));
        let plan = plan_options(&Dimensions {
            mode: 6,
            width: 8,
            height: 9,
        });
        assert_eq!((plan.hdr, plan.presets, plan.auto_exposure), (1, [0; 6], 0));
    }

    #[test]
    fn tag_records_point_at_their_resources_and_keep_extents() {
        let mut batch = TagBatch::d3d11(&unsafe { std::mem::zeroed() }, false);
        batch.count = 2;
        batch.entries[0].texture = std::ptr::without_provenance_mut(8);
        batch.entries[0].kind = 3;
        batch.entries[0].state = 0x40;
        batch.entries[0].size = [640, 360];
        batch.entries[1].texture = std::ptr::without_provenance_mut(16);
        batch.entries[1].kind = 13;
        batch.entries[1].has_extent = 0;
        let mut records = TagRecords::new();
        records.fill(&batch);
        let (tags, count) = records.tags();
        assert_eq!(count, 2);
        let tags = unsafe { std::slice::from_raw_parts(tags, 2) };
        let resource = unsafe { &*tags[0].resource.cast::<Resource>() };
        assert_eq!(
            (resource.image.addr(), resource.layout, resource.type_),
            (8, 0x40, 0)
        );
        assert_eq!(
            (tags[0].type_, tags[0].width, tags[0].height),
            (3, 640, 360)
        );
        assert_eq!((tags[1].width, tags[1].height), (0, 0));
        assert_eq!(tags[1].lifecycle, 0);
    }

    #[test]
    fn viewport_and_adapter_records_carry_their_payloads() {
        assert_eq!(viewport(7).value, 7);
        let mut luid = 0x1234_u64;
        let adapter = adapter(&mut luid);
        assert_eq!(adapter.luid_size, 8);
        assert_eq!(unsafe { adapter.luid.cast::<u64>().read() }, 0x1234);
    }
}
