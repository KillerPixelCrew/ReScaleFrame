// SPDX-License-Identifier: MIT
//! Rust mirrors of the pinned Streamline 2.14.1 records the D3D12 hosts pass to the SDK.
//!
//! `sl.h` is a C++ header, but the interposer and its feature plugins export every entry point
//! with C linkage, and the references in their signatures are pointers on Windows x64. Each
//! record repeats the SDK's versioned header (`next`, GUID, `size_t` version) and member order.
//! The layout assertions repeat the sizes and offsets MSVC reports for the 2.14.1 declarations;
//! `sl::Boolean` is one byte and `sl::Result` an `int`.
#![allow(dead_code)]

use std::ffi::{c_char, c_void};
use std::mem::{offset_of, size_of};
use std::ptr::null_mut;

use rsf_streamline_vk::sdk_camera_defaults;

pub(crate) use crate::sys::rsf_slvk_sdk_fg_options as FgOptions;
pub(crate) use crate::sys::rsf_slvk_sdk_header as FgHeader;
pub(crate) use crate::sys::{
    rsf_slvk_sdk_constants as Constants, rsf_slvk_sdk_header as Header,
    rsf_slvk_sdk_pcl_options as PclOptions, rsf_slvk_sdk_preferences as Preferences,
    rsf_slvk_sdk_reflex_options as ReflexOptions, rsf_slvk_sdk_resource as Resource,
    rsf_slvk_sdk_tag as ResourceTag,
};
pub(crate) use sdk_camera_defaults::constants as default_constants;

/// `sl::kSDKVersion` of the pinned headers.
pub(crate) const SDK_VERSION: u64 = 0x0002_000e_0001_fedc;
pub(crate) const OK: i32 = 0;
pub(crate) const TRUE: u8 = 1;

// Feature IDs.
pub(crate) const FEATURE_DLSS_G: u32 = 1000;
pub(crate) const FEATURE_REFLEX: u32 = 3;
pub(crate) const FEATURE_COMMON: u32 = u32::MAX;

// sl::LogType::eWarn and eError.
pub(crate) const LOG_WARN: u32 = 1;
pub(crate) const LOG_ERROR: u32 = 2;

/// `sl::EngineType::eCount`.
pub(crate) const ENGINE_COUNT: u32 = 3;
/// `sl::ResourceLifecycle::eValidUntilPresent`.
pub(crate) const VALID_UNTIL_PRESENT: u32 = 1;
/// `sl::kReflexFrameReportCount`.
pub(crate) const REFLEX_REPORTS: usize = 64;

const fn header(
    type_lo: u32,
    type_mid: u16,
    type_hi: u16,
    type_tail: [u8; 8],
    version: usize,
) -> Header {
    Header {
        next: null_mut(),
        type_lo,
        type_mid,
        type_hi,
        type_tail,
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
pub(crate) const RESOURCE: Header = header(
    0x3a9d_70cf,
    0x2418,
    0x4b72,
    [0x83, 0x91, 0x13, 0xf8, 0x72, 0x1c, 0x72, 0x61],
    1,
);
pub(crate) const TAG: Header = header(
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
const DLSSG_STATE: Header = header(
    0xcc8a_c8e1,
    0xa179,
    0x44f5,
    [0x97, 0xfa, 0xe7, 0x41, 0x12, 0xf9, 0xbc, 0x61],
    4,
);
const REFLEX_REPORT: Header = header(
    0x0d56_9b37,
    0xa1c8,
    0x4453,
    [0xbe, 0x4d, 0x40, 0xf4, 0xde, 0x57, 0x95, 0x2b],
    1,
);
const REFLEX_REPORT2: Header = header(
    0x68bb_0632,
    0x5e1c,
    0x402b,
    [0x89, 0x9d, 0xb4, 0x9f, 0x63, 0x3c, 0x56, 0xc2],
    1,
);
const REFLEX_STATE: Header = header(
    0xf0bb_5985,
    0xdaf9,
    0x4728,
    [0xb2, 0xfd, 0xae, 0x80, 0xa2, 0xbd, 0x79, 0x89],
    2,
);
const PCL_STATE: Header = header(
    0xcfa3_2f9b,
    0x023c,
    0x420e,
    [0x90, 0x56, 0x68, 0x32, 0xb7, 0x4f, 0x89, 0xb5],
    1,
);
const FEATURE_VERSION: Header = header(
    0x6d5b_51f0,
    0x076b,
    0x486d,
    [0x99, 0x95, 0x5a, 0x56, 0x10, 0x43, 0xf5, 0xc1],
    1,
);
const ADAPTER_INFO: Header = header(
    0x0677_315f,
    0xa746,
    0x4492,
    [0x9f, 0x42, 0xcb, 0x61, 0x42, 0xc9, 0xc3, 0xd4],
    1,
);

/// `sl::ViewportHandle`.
#[repr(C)]
pub(crate) struct ViewportHandle {
    base: Header,
    value: u32,
}

impl ViewportHandle {
    pub(crate) const fn new(value: u32) -> Self {
        Self {
            base: VIEWPORT,
            value,
        }
    }
}

/// `sl::Version`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Version {
    pub(crate) major: u32,
    pub(crate) minor: u32,
    pub(crate) build: u32,
}

impl Version {
    /// `sl::Version::operator bool`.
    pub(crate) fn present(&self) -> bool {
        self.major != 0 || self.minor != 0 || self.build != 0
    }
}

/// `sl::FeatureVersion`.
#[repr(C)]
pub(crate) struct FeatureVersion {
    base: Header,
    pub(crate) streamline: Version,
    pub(crate) ngx: Version,
}

impl FeatureVersion {
    pub(crate) fn new() -> Self {
        Self {
            base: FEATURE_VERSION,
            streamline: Version::default(),
            ngx: Version::default(),
        }
    }
}

/// `sl::AdapterInfo`.
#[repr(C)]
pub(crate) struct AdapterInfo {
    base: Header,
    pub(crate) luid: *mut u8,
    pub(crate) luid_size: u32,
    pub(crate) vk_physical_device: *mut c_void,
}

impl AdapterInfo {
    pub(crate) fn new(luid: *mut u8, luid_size: u32) -> Self {
        Self {
            base: ADAPTER_INFO,
            luid,
            luid_size,
            vk_physical_device: null_mut(),
        }
    }
}

/// `sl::DLSSGState`, structure version 4.
#[repr(C)]
pub(crate) struct DlssgState {
    base: Header,
    pub(crate) estimated_vram: u64,
    pub(crate) status: u32,
    pub(crate) min_width_or_height: u32,
    pub(crate) presented: u32,
    pub(crate) generated_max: u32,
    reserved4: u8,
    pub(crate) vsync_available: u8,
    pub(crate) fence: *mut c_void,
    pub(crate) fence_value: u64,
    pub(crate) dynamic_supported: u8,
}

impl DlssgState {
    pub(crate) fn new() -> Self {
        Self {
            base: DLSSG_STATE,
            estimated_vram: 0,
            status: 0,
            min_width_or_height: 0,
            presented: 0,
            generated_max: 0,
            reserved4: 0,
            vsync_available: 0,
            fence: null_mut(),
            fence_value: 0,
            dynamic_supported: 0,
        }
    }
}

/// `sl::ReflexReport`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ReflexReport {
    base: Header,
    pub(crate) frame_id: u64,
    pub(crate) input_sample: u64,
    pub(crate) simulation_start: u64,
    pub(crate) simulation_end: u64,
    pub(crate) submit_start: u64,
    pub(crate) submit_end: u64,
    pub(crate) present_start: u64,
    pub(crate) present_end: u64,
    pub(crate) driver_start: u64,
    pub(crate) driver_end: u64,
    pub(crate) queue_start: u64,
    pub(crate) queue_end: u64,
    pub(crate) gpu_start: u64,
    pub(crate) gpu_end: u64,
    pub(crate) gpu_active_us: u32,
    pub(crate) gpu_frame_us: u32,
}

/// `sl::ReflexReport2`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ReflexReport2 {
    base: Header,
    pub(crate) camera_constructed: u64,
    pub(crate) cross_adapter_copy_us: u32,
}

/// `sl::ReflexState`, structure version 2.
#[repr(C)]
pub(crate) struct ReflexState {
    base: Header,
    pub(crate) low_latency_available: bool,
    pub(crate) latency_report_available: bool,
    pub(crate) stats_window_message: u32,
    pub(crate) reports: [ReflexReport; REFLEX_REPORTS],
    pub(crate) flash_indicator_driver_controlled: bool,
    pub(crate) reports2: [ReflexReport2; REFLEX_REPORTS],
}

// The SDK constructors fill every nested report header. The state answer leaves them intact.
impl ReflexState {
    pub(crate) fn new() -> Box<Self> {
        let report = ReflexReport {
            base: REFLEX_REPORT,
            frame_id: 0,
            input_sample: 0,
            simulation_start: 0,
            simulation_end: 0,
            submit_start: 0,
            submit_end: 0,
            present_start: 0,
            present_end: 0,
            driver_start: 0,
            driver_end: 0,
            queue_start: 0,
            queue_end: 0,
            gpu_start: 0,
            gpu_end: 0,
            gpu_active_us: 0,
            gpu_frame_us: 0,
        };
        let report2 = ReflexReport2 {
            base: REFLEX_REPORT2,
            camera_constructed: 0,
            cross_adapter_copy_us: 0,
        };
        Box::new(Self {
            base: REFLEX_STATE,
            low_latency_available: false,
            latency_report_available: false,
            stats_window_message: 0,
            reports: [report; REFLEX_REPORTS],
            flash_indicator_driver_controlled: false,
            reports2: [report2; REFLEX_REPORTS],
        })
    }
}

/// `sl::PCLState`.
#[repr(C)]
pub(crate) struct PclState {
    base: Header,
    pub(crate) stats_window_message: u32,
}

impl PclState {
    pub(crate) fn new() -> Self {
        Self {
            base: PCL_STATE,
            stats_window_message: 0,
        }
    }
}

/// `sl::Resource(sl::ResourceType::eTex2d, native, state)`.
pub(crate) fn texture(native: *mut c_void, state: u32) -> Resource {
    Resource {
        base: RESOURCE,
        type_: 0,
        image: native,
        layout: state,
        ..Default::default()
    }
}

/// `sl::ResourceTag(resource, type, lifecycle, &extent)`.
pub(crate) fn tag(resource: *mut Resource, kind: u32, extent: [u32; 4]) -> ResourceTag {
    let [top, left, width, height] = extent;
    ResourceTag {
        base: TAG,
        resource: resource.cast(),
        type_: kind,
        lifecycle: VALID_UNTIL_PRESENT,
        top,
        left,
        width,
        height,
    }
}

/// `sl::matrixFullInvert` from sl_matrix_helpers.h, with its row-major element order and its
/// rule for a singular input: the cofactors are returned unscaled.
pub(crate) fn full_invert(m: &[f32; 16]) -> [f32; 16] {
    let mut r = [0.0f32; 16];
    r[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];
    r[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];
    r[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];
    r[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];
    r[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];
    r[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];
    r[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];
    r[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];
    r[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];
    r[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];
    r[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];
    r[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];
    r[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];
    r[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    r[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    r[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];
    let det = m[0] * r[0] + m[1] * r[4] + m[2] * r[8] + m[3] * r[12];
    if det != 0.0 {
        let det = 1.0 / det;
        for value in &mut r {
            *value *= det;
        }
    }
    r
}

/// The C-linkage Streamline entry points the D3D12 host binds, with `sl::` references as pointers.
pub(crate) mod api {
    use super::*;
    type Handle = *mut c_void;

    pub(crate) type Init = unsafe extern "C" fn(&Preferences, u64) -> i32;
    pub(crate) type Shutdown = unsafe extern "C" fn() -> i32;
    pub(crate) type Upgrade = unsafe extern "C" fn(*mut Handle) -> i32;
    pub(crate) type Native = unsafe extern "C" fn(Handle, *mut Handle) -> i32;
    pub(crate) type SetDevice = unsafe extern "C" fn(Handle) -> i32;
    pub(crate) type Supported = unsafe extern "C" fn(u32, &AdapterInfo) -> i32;
    pub(crate) type Function = unsafe extern "C" fn(u32, *const c_char, *mut Handle) -> i32;
    pub(crate) type FeatureVersionFn = unsafe extern "C" fn(u32, &mut FeatureVersion) -> i32;
    pub(crate) type Loaded = unsafe extern "C" fn(u32, bool) -> i32;
    pub(crate) type NewToken = unsafe extern "C" fn(*mut Handle, *const u32) -> i32;
    pub(crate) type SetConstants =
        unsafe extern "C" fn(*const Constants, Handle, &ViewportHandle) -> i32;
    pub(crate) type SetTags =
        unsafe extern "C" fn(Handle, &ViewportHandle, *const ResourceTag, u32, Handle) -> i32;
    pub(crate) type GetState =
        unsafe extern "C" fn(&ViewportHandle, &mut DlssgState, *const FgOptions) -> i32;
    pub(crate) type SetOptions = unsafe extern "C" fn(&ViewportHandle, &FgOptions) -> i32;
    pub(crate) type ReflexSetOptions = unsafe extern "C" fn(&ReflexOptions) -> i32;
    pub(crate) type ReflexGetState = unsafe extern "C" fn(&mut ReflexState) -> i32;
    pub(crate) type Sleep = unsafe extern "C" fn(Handle) -> i32;
    pub(crate) type PclMarker = unsafe extern "C" fn(u32, Handle) -> i32;
    pub(crate) type PclSetOptions = unsafe extern "C" fn(&PclOptions) -> i32;
    pub(crate) type PclGetState = unsafe extern "C" fn(&mut PclState) -> i32;
    pub(crate) type Log = unsafe extern "C" fn(u32, *const c_char);
    /// `slHookPresent`: `HRESULT(IDXGISwapChain*, UINT, UINT, bool& skip)`.
    pub(crate) type HookPresent = unsafe extern "C" fn(Handle, u32, u32, &mut bool) -> i32;
    /// `slHookAfterPresent`: `HRESULT(UINT)`.
    pub(crate) type HookAfterPresent = unsafe extern "C" fn(u32) -> i32;
}

const _: () = {
    assert!(size_of::<Header>() == 32);
    assert!(size_of::<FgHeader>() == 32);
    assert!(size_of::<ViewportHandle>() == 40);
    assert!(offset_of!(ViewportHandle, value) == 32);
    assert!(size_of::<Version>() == 12);
    assert!(size_of::<FeatureVersion>() == 56);
    assert!(offset_of!(FeatureVersion, streamline) == 32);
    assert!(offset_of!(FeatureVersion, ngx) == 44);
    assert!(size_of::<AdapterInfo>() == 56);
    assert!(offset_of!(AdapterInfo, luid) == 32);
    assert!(offset_of!(AdapterInfo, luid_size) == 40);
    assert!(offset_of!(AdapterInfo, vk_physical_device) == 48);
    assert!(size_of::<DlssgState>() == 88);
    assert!(offset_of!(DlssgState, estimated_vram) == 32);
    assert!(offset_of!(DlssgState, status) == 40);
    assert!(offset_of!(DlssgState, min_width_or_height) == 44);
    assert!(offset_of!(DlssgState, presented) == 48);
    assert!(offset_of!(DlssgState, generated_max) == 52);
    assert!(offset_of!(DlssgState, reserved4) == 56);
    assert!(offset_of!(DlssgState, vsync_available) == 57);
    assert!(offset_of!(DlssgState, fence) == 64);
    assert!(offset_of!(DlssgState, fence_value) == 72);
    assert!(offset_of!(DlssgState, dynamic_supported) == 80);
    assert!(size_of::<ReflexReport>() == 152);
    assert!(offset_of!(ReflexReport, frame_id) == 32);
    assert!(offset_of!(ReflexReport, gpu_end) == 136);
    assert!(offset_of!(ReflexReport, gpu_active_us) == 144);
    assert!(size_of::<ReflexReport2>() == 48);
    assert!(offset_of!(ReflexReport2, cross_adapter_copy_us) == 40);
    assert!(offset_of!(ReflexState, low_latency_available) == 32);
    assert!(offset_of!(ReflexState, latency_report_available) == 33);
    assert!(offset_of!(ReflexState, stats_window_message) == 36);
    assert!(offset_of!(ReflexState, reports) == 40);
    assert!(offset_of!(ReflexState, flash_indicator_driver_controlled) == 9768);
    assert!(offset_of!(ReflexState, reports2) == 9776);
    assert!(size_of::<ReflexState>() == 12848);
    assert!(size_of::<PclState>() == 40);
    assert!(offset_of!(PclState, stats_window_message) == 32);
    // Preferences, Constants, Resource, ResourceTag, the FG, Reflex and PCL options carry their
    // own generated assertions, which runtime/vulkan/streamline/sdk_records_checks.h and
    // sdk_fg_options_checks.h compare against the SDK declarations.
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_invert_matches_the_sdk_helper() {
        let identity: [f32; 16] = std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 });
        assert_eq!(full_invert(&identity), identity);
        // Row-major translation by (2, 3, 4) in the fourth row.
        let mut translate = identity;
        translate[12..15].copy_from_slice(&[2.0, 3.0, 4.0]);
        let mut expected = identity;
        expected[12..15].copy_from_slice(&[-2.0, -3.0, -4.0]);
        assert_eq!(full_invert(&translate), expected);
        let scale: [f32; 16] = std::array::from_fn(|i| if i % 5 == 0 { 2.0 } else { 0.0 });
        assert_eq!(full_invert(&scale)[0], 0.5);
        // Singular input keeps the unscaled cofactors, as the SDK helper does.
        let mut singular = identity;
        singular[0] = 0.0;
        let cofactors = full_invert(&singular);
        assert_eq!(cofactors[0], 1.0);
        assert_eq!(cofactors[5], 0.0);
    }

    #[test]
    fn report_headers_match_the_sdk_constructors() {
        let state = ReflexState::new();
        assert_eq!(state.base.version, 2);
        assert_eq!(state.reports[63].base.type_lo, 0x0d56_9b37);
        assert_eq!(state.reports2[0].base.type_lo, 0x68bb_0632);
        assert_eq!(ViewportHandle::new(0).base.type_lo, 0x171b_6435);
        assert_eq!(DlssgState::new().base.version, 4);
    }
}
