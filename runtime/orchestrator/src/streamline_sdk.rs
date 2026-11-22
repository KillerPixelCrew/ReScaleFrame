// SPDX-License-Identifier: MIT
//! The pinned Streamline 2.14.1 SDK as the D3D owners call it.
//!
//! `sl.h` is a C++ header, but the interposer and its feature plugins export every entry point
//! with C linkage, and the references in their signatures are pointers on Windows x64. The records
//! are the ones rsf-streamline-vk generates from `sdk_records.h`, `sdk_sr_options.h` and
//! `sdk_fg_options.h`, which `sdk_records_checks.h` and `sdk_fg_options_checks.h` check against
//! the SDK declarations (compiled by `tests/dlss_sr_layout_facts.cpp`); the entry points are its
//! one table, [`Entries`]. Every D3D owner loads the interposer through [`load_interposer`].
#[cfg(all(feature = "test-fixtures", not(test)))]
mod layout_fixture;

use crate::streamline_sr::{Constants as Camera, Dimensions, MAX_TAGS, OptionPayload, TagBatch};
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::core::PCWSTR;

pub(crate) use crate::sys::{
    rsf_slvk_sdk_adapter as AdapterInfo, rsf_slvk_sdk_constants as Constants,
    rsf_slvk_sdk_feature_version as FeatureVersion, rsf_slvk_sdk_fg_options as FgOptions,
    rsf_slvk_sdk_fg_state as DlssgState, rsf_slvk_sdk_header as Header,
    rsf_slvk_sdk_pcl_options as PclOptions, rsf_slvk_sdk_pcl_state as PclState,
    rsf_slvk_sdk_preferences as Preferences, rsf_slvk_sdk_reflex_options as ReflexOptions,
    rsf_slvk_sdk_reflex_state as ReflexState, rsf_slvk_sdk_requirements as Requirements,
    rsf_slvk_sdk_resource as Resource, rsf_slvk_sdk_tag as ResourceTag,
    rsf_slvk_sdk_version as Version, rsf_vkdlss_optimal as DlssOptimal,
    rsf_vkdlss_options as DlssOptions,
};
pub(crate) use rsf_streamline_vk::sdk_api::{
    DLSS_OPTIMAL, DLSS_OPTIONS, Entries, FEATURE_COMMON, FEATURE_DLSS, FEATURE_DLSS_G, FEATURE_PCL,
    FEATURE_REFLEX, HookAfterPresent, HookPresent, OK, PCL_STATE, PREFERENCES, REFLEX_STATE,
    RESOURCE, SDK_VERSION, TAG, header, viewport,
};
pub(crate) use rsf_streamline_vk::sdk_camera_defaults::constants as default_constants;
pub(crate) use rsf_streamline_vk::sdk_fg_options::{pack as fg_options, state as dlssg_state};
pub(crate) use rsf_streamline_vk::sdk_records::{
    reflex_options, rsf_slvk_pack_pcl_options as pcl_options,
};

type Handle = *mut c_void;

/// `sl::Result::eOk`, as the SR callers compare it.
pub(crate) const RESULT_OK: i32 = OK;
pub(crate) const TRUE: u8 = 1;
/// `PreferenceFlags::eUseManualHooking` and `eUseFrameBasedResourceTagging`.
pub(crate) const PREFERENCE_MANUAL_HOOKING: u64 = 1 << 2;
pub(crate) const PREFERENCE_FRAME_TAGGING: u64 = 1 << 7;
/// `PreferenceFlags::eDisableCLStateTracking | eAllowOTA | eLoadDownloadedPlugins`, the defaults of
/// an `sl::Preferences` that the D3D11 startup extends with its own flags.
pub(crate) const DEFAULT_PREFERENCE_FLAGS: u64 = (1 << 0) | (1 << 3) | (1 << 6);
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
/// `sl::EngineType::eCount`.
pub(crate) const ENGINE_COUNT: u32 = 3;
/// `sl::ResourceLifecycle::eValidUntilPresent`.
pub(crate) const VALID_UNTIL_PRESENT: u32 = 1;

const REQUIREMENTS: Header = header(
    0x6671_4097,
    0xac6d,
    0x4bc6,
    [0x89, 0x15, 0x1e, 0x0f, 0x55, 0xa6, 0xb6, 0x1f],
    2,
);
const FEATURE_VERSION: Header = header(
    0x6d5b_51f0,
    0x076b,
    0x486d,
    [0x99, 0x95, 0x5a, 0x56, 0x10, 0x43, 0xf5, 0xc1],
    1,
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

/// Whether a Streamline interposer is already loaded in this process. Its owner holds the SDK and
/// its device registration.
pub(crate) fn interposer_loaded() -> bool {
    !unsafe {
        windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(windows_sys::core::w!(
            "sl.interposer.dll"
        ))
    }
    .is_null()
}

/// Loads `sl.interposer.dll` from an absolute path, after the NVIDIA signature check when one is
/// required, and returns the module or null. This puts a signed NVIDIA module into a game
/// process, so the path alone never decides which module answers: every D3D owner uses the same
/// fixed search. The caller adopts the module with `SdkModule::adopt_system`.
pub(crate) unsafe fn load_interposer(
    path: *const u16,
    require_signature: bool,
    log: &crate::input_dump::Log,
) -> Handle {
    let path_text = || {
        if path.is_null() {
            "(null)".to_owned()
        } else {
            unsafe { PCWSTR(path).to_string() }.unwrap_or_else(|_| "(invalid UTF-16)".to_owned())
        }
    };
    if unsafe { crate::streamline_policy::rsf_streamline_absolute_path(path) } == 0 {
        log.say(&format!("interposer path is not absolute: {}", path_text()));
        return null_mut();
    }
    if require_signature && !unsafe { rsf_streamline_vk::admission::nvidia_signed(path) } {
        log.say(&format!(
            "interposer at {} could not be verified as signed",
            path_text()
        ));
        return null_mut();
    }
    let (module, error) = unsafe { crate::sdk_module::load_absolute_library(path) };
    if module.is_null() {
        log.say(&format!(
            "could not load {} (Windows error {error})",
            path_text()
        ));
    }
    module
}

/// Calls `slHookPresent` (begin) or `slHookAfterPresent`. Succeeds only when the hook succeeded
/// and did not ask to skip the Present.
pub(crate) unsafe fn hook_present(
    [before, after]: [Handle; 2],
    swapchain: Handle,
    interval: u32,
    flags: u32,
    begin: bool,
) -> bool {
    let mut skip = false;
    let result = if begin {
        let before = unsafe { std::mem::transmute::<Handle, HookPresent>(before) };
        unsafe { before(swapchain, interval, flags, &mut skip) }
    } else {
        let after = unsafe { std::mem::transmute::<Handle, HookAfterPresent>(after) };
        unsafe { after(flags) }
    };
    result >= 0 && !skip
}

/// An adapter naming its LUID, which must stay alive for the call.
pub(crate) fn adapter(luid: &mut u64) -> AdapterInfo {
    AdapterInfo {
        luid: std::ptr::from_mut(luid).cast(),
        luid_size: size_of::<u64>() as u32,
        ..rsf_streamline_vk::sdk_api::adapter(null_mut())
    }
}

pub(crate) fn requirements() -> Requirements {
    Requirements {
        base: REQUIREMENTS,
        ..Default::default()
    }
}

pub(crate) fn feature_version() -> FeatureVersion {
    FeatureVersion {
        base: FEATURE_VERSION,
        ..Default::default()
    }
}

/// `sl::Version::operator bool`.
pub(crate) fn version_present(version: &Version) -> bool {
    version.major != 0 || version.minor != 0 || version.build != 0
}

/// An `sl::ReflexState` as its constructor leaves it: every nested report carries its header.
/// The state answer leaves them intact.
pub(crate) fn reflex_state() -> Box<ReflexState> {
    let mut state: Box<ReflexState> = Box::default();
    state.base = REFLEX_STATE;
    for report in &mut state.reports {
        report.base = REFLEX_REPORT;
    }
    for report in &mut state.reports2 {
        report.base = REFLEX_REPORT2;
    }
    state
}

pub(crate) fn pcl_state() -> PclState {
    PclState {
        base: PCL_STATE,
        window_message: 0,
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
        width: dimensions.width,
        height: dimensions.height,
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
        alpha_upscaling: u8::from(input.alpha != 0),
        ..plan_options(&input.dimensions)
    }
}

/// The constructor defaults of `sl::Constants` overlaid with the camera and motion values.
pub(crate) fn constants(input: &Camera) -> Constants {
    Constants {
        view_to_clip: input.view_to_clip,
        clip_to_view: input.clip_to_view,
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
        ..default_constants()
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

/// `sl::ResourceTag(resource, type, eValidUntilPresent, &extent)`.
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

/// Resource and tag records for one slSetTagForFrame call. `fill` points each tag at its resource,
/// so build the value where it will stay for the call.
pub(crate) struct TagRecords {
    resources: [Resource; MAX_TAGS],
    tags: [ResourceTag; MAX_TAGS],
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
            self.resources[index] = texture(entry.texture, entry.state);
            let (width, height) = if entry.has_extent {
                (entry.size[0], entry.size[1])
            } else {
                (0, 0)
            };
            self.tags[index] = ResourceTag {
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

    pub(crate) fn tags(&self) -> (*const ResourceTag, u32) {
        (self.tags.as_ptr(), self.count)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streamline_sr::Options;

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
        let state = reflex_state();
        assert_eq!(state.base.version, 2);
        assert_eq!(state.reports[63].base.type_lo, 0x0d56_9b37);
        assert_eq!(state.reports2[0].base.type_lo, 0x68bb_0632);
        assert_eq!(viewport(0).base.type_lo, 0x171b_6435);
        assert_eq!(dlssg_state().base.version, 4);
    }

    #[test]
    fn constants_keep_unspecified_sdk_defaults() {
        let mut input = Camera::default();
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
        assert_eq!((record.mode, record.width, record.height), (3, 1920, 1080));
        assert_eq!(record.presets, [11; 6]);
        assert_eq!(
            (record.hdr, record.auto_exposure, record.alpha_upscaling),
            (0, 1, 1)
        );
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
        batch.entries[1].has_extent = false;
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
