// SPDX-License-Identifier: MIT
#![allow(non_upper_case_globals)]
use super::hooks::{bind_named, rsf_census_hook_bind_thunk, rsf_census_hook_original};
use super::json::{Json, emit_decimal, rsf_census_json_bytes, rsf_census_json_wide};
use super::memory::{plausible, read, u16_at, u32_at, u64_at};
use super::platform;
use super::rsf_census_call_state;
use super::session::{rsf_census_call_begin, rsf_census_call_finish};
use crate::sys::*;
use std::ffi::{CStr, c_char, c_void};
use std::io::Write;
use std::mem::MaybeUninit;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

type Ptr = *mut c_void;
type Args = [u64; 8];
type Entry = rsf_census_misc_entry;
const COUNT: usize = RSF_MISC_COUNT as usize;
const SL_HEADER: usize = 32;
struct Services {
    entries: [u16; COUNT],
}
static SERVICES: OnceLock<Services> = OnceLock::new();

fn id(entry: Entry) -> u16 {
    SERVICES.get().unwrap().entries[entry as usize]
}

fn start(
    slot: &mut MaybeUninit<rsf_census_call_state>,
    entry: Entry,
    caller: Ptr,
) -> *mut rsf_census_call_state {
    let state = slot.as_mut_ptr();
    unsafe { rsf_census_call_begin(state, id(entry), caller) };
    state
}

fn read_name(address: u64, text: &mut [u8]) -> bool {
    unsafe { platform().read_string.unwrap()(address as _, text.as_mut_ptr().cast(), text.len()) }
}
fn label(names: &[&'static CStr], index: u32) -> &'static CStr {
    names
        .get(index as usize)
        .copied()
        .filter(|value| !value.is_empty())
        .unwrap_or(c"other")
}

trait MiscJson {
    fn flag(&mut self, key: &CStr, value: bool);
    fn float(&mut self, key: Option<&CStr>, bytes: &[u8], offset: usize);
    fn output(&mut self, key: &CStr, out: u64);
    fn fields(&mut self, data: &[u8], fields: &[(&CStr, usize)]);
}

impl MiscJson for Json<'_> {
    fn flag(&mut self, key: &CStr, value: bool) {
        self.raw(key, if value { c"true" } else { c"false" });
    }
    fn float(&mut self, key: Option<&CStr>, bytes: &[u8], offset: usize) {
        let value = f64::from(f32::from_bits(u32_at(bytes, offset)));
        unsafe {
            emit_decimal(
                self.0,
                key.map_or(null(), CStr::as_ptr),
                value,
                platform().decimal.unwrap(),
            )
        };
    }
    fn output(&mut self, key: &CStr, out: u64) {
        if let Some(result) = read::<8>(out, 0) {
            self.hex(Some(key), u64::from_le_bytes(result));
        } else {
            self.raw(key, c"null");
        }
    }
    fn fields(&mut self, data: &[u8], fields: &[(&CStr, usize)]) {
        for &(key, offset) in fields {
            self.number(key, u32_at(data, offset));
        }
    }
}

const NV_MARKER_NAMES: &[&CStr] = &[
    c"SIMULATION_START",
    c"SIMULATION_END",
    c"RENDERSUBMIT_START",
    c"RENDERSUBMIT_END",
    c"PRESENT_START",
    c"PRESENT_END",
    c"INPUT_SAMPLE",
    c"TRIGGER_FLASH",
    c"PC_LATENCY_PING",
    c"OUT_OF_BAND_RENDERSUBMIT_START",
    c"OUT_OF_BAND_RENDERSUBMIT_END",
    c"OUT_OF_BAND_PRESENT_START",
    c"OUT_OF_BAND_PRESENT_END",
];

#[inline(never)]
extern "system" fn nv_query(interface: u32) -> Ptr {
    let pc = caller!();
    let original = original!(
        RSF_MISC_NvapiQueryInterface,
        unsafe extern "system" fn(u32) -> Ptr
    );
    let mut slot = MaybeUninit::uninit();
    let state = start(&mut slot, RSF_MISC_NvapiQueryInterface, pc);
    let result = original.map_or(null_mut(), |resolve| unsafe { resolve(interface) });
    let reflex = match interface {
        0xac1c_a9e0 => Some((RSF_MISC_NvapiSetSleepMode, c"NvapiSetSleepMode")),
        0x852c_d1d2 => Some((RSF_MISC_NvapiSleep, c"NvapiSleep")),
        0xd998_4c05 => Some((RSF_MISC_NvapiSetLatencyMarker, c"NvapiSetLatencyMarker")),
        0x1a58_7f9c => Some((RSF_MISC_NvapiGetLatency, c"NvapiGetLatency")),
        0x13c9_8f73 => Some((
            RSF_MISC_NvapiD3D12SetAsyncFrameMarker,
            c"NvapiD3D12SetAsyncFrameMarker",
        )),
        _ => None,
    };
    if unsafe { (*state).full } {
        let mut json = Json(unsafe { &mut (*state).json });
        json.hex(Some(c"id"), interface.into());
        json.hex(Some(c"result"), result as u64);
        if let Some((_, name)) = reflex {
            json.text(c"reflex_entry", name);
        }
    }
    unsafe { rsf_census_call_finish(state, result as u64) };
    if let Some((entry, _)) = reflex
        && !result.is_null()
    {
        rsf_census_hook_bind_thunk(id(entry), result)
    } else {
        result
    }
}

fn nv_mode(json: &mut Json<'_>, args: &Args) {
    json.hex(Some(c"device"), args[0]);
    if let Some(params) = read::<40>(args[1], 0) {
        json.hex(Some(c"version"), u32_at(&params, 0).into());
        json.flag(c"low_latency_mode", params[4] != 0);
        json.flag(c"low_latency_boost", params[5] != 0);
        json.number(c"minimum_interval_us", u32_at(&params, 8));
        json.flag(c"use_markers_to_optimize", params[12] != 0);
        json.text(c"layout", c"assumed: mode@4 boost@5 interval@8 markers@12");
    } else {
        json.flag(c"params_unreadable", true);
    }
}
fn nv_marker(json: &mut Json<'_>, args: &Args, vulkan: bool) {
    if vulkan {
        json.hex(Some(c"device"), args[0]);
        if let Some(params) = read::<16>(args[1], 0) {
            marker_fields(
                json,
                u64_at(&params, 0),
                u32_at(&params, 8),
                c"assumed: frameID@0 markerType@8",
            );
        }
    } else {
        json.hex(Some(c"device_or_queue"), args[0]);
        if let Some(params) = read::<24>(args[1], 0) {
            json.hex(Some(c"version"), u32_at(&params, 0).into());
            marker_fields(
                json,
                u64_at(&params, 8),
                u32_at(&params, 16),
                c"assumed: frameID@8 markerType@16",
            );
        } else {
            json.flag(c"params_unreadable", true);
        }
    }
}
fn marker_fields(json: &mut Json<'_>, frame: u64, marker: u32, layout: &CStr) {
    json.number(Some(c"frame_id"), frame);
    json.number(c"marker_type", marker);
    json.text(c"marker", label(NV_MARKER_NAMES, marker));
    json.text(c"layout", layout);
}

fn feature_name(value: u32) -> &'static CStr {
    match value {
        0 => c"DLSS",
        2 => c"NIS",
        3 => c"Reflex",
        4 => c"PCL",
        5 => c"DeepDVC",
        6 => c"Latewarp",
        1000 => c"DLSS_G",
        1001 => c"DLSS_RR",
        1002 => c"NvPerf",
        1003 => c"DirectSR",
        1004 => c"DLSS_NR",
        9999 => c"ImGUI",
        _ => c"other",
    }
}
fn feature(json: &mut Json<'_>, feature: u32) {
    json.number(c"feature", feature);
    json.text(c"feature_name", feature_name(feature));
}
fn viewport(json: &mut Json<'_>, address: u64) {
    json.number(
        c"viewport",
        read::<4>(address, SL_HEADER).map_or(u32::MAX, u32::from_le_bytes),
    );
}
fn frame(json: &mut Json<'_>, token: u64) {
    let mut value = 0;
    if unsafe { platform().frame_index.unwrap()(token as _, &mut value) } {
        json.number(c"frame", value);
    } else {
        json.hex(Some(c"frame_token"), token);
    }
}

const BUFFERS: &[&CStr] = &[
    c"Depth",
    c"MotionVectors",
    c"HUDLessColor",
    c"ScalingInputColor",
    c"ScalingOutputColor",
    c"Normals",
    c"Roughness",
    c"Albedo",
    c"SpecularAlbedo",
    c"IndirectAlbedo",
    c"SpecularMotionVectors",
    c"DisocclusionMask",
    c"Emissive",
    c"Exposure",
    c"NormalRoughness",
    c"DiffuseHitNoisy",
    c"DiffuseHitDenoised",
    c"SpecularHitNoisy",
    c"SpecularHitDenoised",
    c"ShadowNoisy",
    c"ShadowDenoised",
    c"AmbientOcclusionNoisy",
    c"AmbientOcclusionDenoised",
    c"UIColorAndAlpha",
    c"ShadowHint",
    c"ReflectionHint",
    c"ParticleHint",
    c"TransparencyHint",
    c"AnimatedTextureHint",
    c"BiasCurrentColorHint",
    c"RaytracingDistance",
    c"ReflectionMotionVectors",
    c"Position",
    c"InvalidDepthMotionHint",
    c"Alpha",
    c"OpaqueColor",
    c"ReactiveMaskHint",
    c"TransparencyAndCompositionMaskHint",
    c"ReflectedAlbedo",
    c"ColorBeforeParticles",
];

fn tags(json: &mut Json<'_>, address: u64, count: u32) {
    json.open(Some(c"tags"), b'[');
    for index in 0..count.min(16) as usize {
        let Some(tag) = read::<64>(address, index * 64) else {
            break;
        };
        json.open(None, b'{');
        let kind = u32_at(&tag, 40);
        json.number(c"buffer_type", kind);
        json.text(c"buffer", label(BUFFERS, kind));
        json.fields(
            &tag,
            &[
                (c"lifecycle", 44),
                (c"extent_top", 48),
                (c"extent_left", 52),
                (c"extent_width", 56),
                (c"extent_height", 60),
            ],
        );
        if let Some(resource) = read::<96>(u64_at(&tag, 32), 0) {
            let native = u64_at(&resource, 40);
            json.number(c"resource_type", resource[32]);
            json.hex(Some(c"state"), u32_at(&resource, 64).into());
            json.fields(
                &resource,
                &[(c"width", 68), (c"height", 72), (c"native_format", 76)],
            );
            if resource[32] == 0 && native != 0 {
                unsafe {
                    super::decode::rsf_census_json_resource(
                        json.0,
                        c"native".as_ptr(),
                        native as _,
                        true,
                        platform(),
                    )
                };
            } else if native == 0 {
                json.raw(c"native_ptr", c"null");
            } else {
                json.hex(Some(c"native_ptr"), native);
            }
        }
        json.close(b'}');
    }
    json.close(b']');
}

fn sl_init(json: &mut Json<'_>, args: &Args) {
    if let Some(preferences) = read::<144>(args[0], 0) {
        json.flag(c"show_console", preferences[32] != 0);
        json.fields(
            &preferences,
            &[(c"log_level", 36), (c"num_plugin_paths", 48)],
        );
        let mut path = [0; 260];
        if unsafe {
            platform().read_wstring.unwrap()(
                u64_at(&preferences, 56) as _,
                path.as_mut_ptr(),
                path.len(),
            )
        } && path[0] != 0
        {
            unsafe {
                rsf_census_json_wide(json.0, c"path_to_logs_and_data".as_ptr(), path.as_ptr())
            };
        }
        json.hex(Some(c"flags"), u64_at(&preferences, 88));
        json.fields(&preferences, &[(c"application_id", 108), (c"engine", 112)]);
        for (key, offset) in [(c"engine_version", 120), (c"project_id", 128)] {
            let mut value = [0; 96];
            if read_name(u64_at(&preferences, offset), &mut value)
                && value[0] != 0
                && let Ok(text) = CStr::from_bytes_until_nul(&value)
            {
                json.text(key, text);
            }
        }
        json.number(c"render_api", u32_at(&preferences, 136));
        json.open(Some(c"features_to_load"), b'[');
        for index in 0..u32_at(&preferences, 104).min(24) as usize {
            let Some(feature) = read::<4>(u64_at(&preferences, 96), index * 4) else {
                break;
            };
            json.number(None, u64::from(u32::from_le_bytes(feature)));
        }
        json.close(b']');
    } else {
        json.flag(c"preferences_unreadable", true);
    }
    json.hex(Some(c"sdk_version"), args[1]);
}

fn constants(json: &mut Json<'_>, args: &Args) {
    if let Some(values) = read::<456>(args[0], 0) {
        json.open(Some(c"constants"), b'{');
        json.number(c"struct_version", u32_at(&values, 24));
        for (key, offset, count) in [
            (c"camera_view_to_clip", 32, 16),
            (c"clip_to_camera_view", 96, 16),
            (c"clip_to_lens_clip", 160, 16),
            (c"clip_to_prev_clip", 224, 16),
            (c"prev_clip_to_clip", 288, 16),
            (c"camera_pos", 376, 3),
            (c"camera_up", 388, 3),
            (c"camera_right", 400, 3),
            (c"camera_fwd", 412, 3),
        ] {
            json.open(Some(key), b'[');
            for index in 0..count {
                json.float(None, &values, offset + index * 4);
            }
            json.close(b']');
        }
        for (key, offset) in [
            (c"jitter_x", 352),
            (c"jitter_y", 356),
            (c"mvec_scale_x", 360),
            (c"mvec_scale_y", 364),
            (c"camera_near", 424),
            (c"camera_far", 428),
            (c"camera_fov", 432),
            (c"camera_aspect", 436),
        ] {
            json.float(Some(key), &values, offset);
        }
        for (index, key) in [
            c"depth_inverted",
            c"camera_motion_included",
            c"motion_vectors_3d",
            c"reset",
            c"orthographic",
            c"motion_vectors_dilated",
            c"motion_vectors_jittered",
        ]
        .into_iter()
        .enumerate()
        {
            json.flag(key, values[444 + index] == 1);
        }
        json.close(b'}');
    }
    frame(json, args[1]);
    viewport(json, args[2]);
}

fn dlssg_options(json: &mut Json<'_>, args: &Args) {
    viewport(json, args[0]);
    if let Some(options) = read::<68>(args[1], SL_HEADER) {
        let mode = u32_at(&options, 0);
        json.number(c"mode", mode);
        json.text(
            c"mode_name",
            label(&[c"Off", c"On", c"Auto", c"Dynamic"], mode),
        );
        json.number(c"num_frames_to_generate", u32_at(&options, 4));
        json.hex(Some(c"flags"), u32_at(&options, 8).into());
        json.fields(
            &options,
            &[
                (c"dynamic_res_width", 12),
                (c"dynamic_res_height", 16),
                (c"num_back_buffers", 20),
                (c"mvec_depth_width", 24),
                (c"mvec_depth_height", 28),
                (c"color_width", 32),
                (c"color_height", 36),
                (c"color_buffer_format", 40),
                (c"mvec_buffer_format", 44),
                (c"depth_buffer_format", 48),
                (c"hudless_buffer_format", 52),
                (c"ui_buffer_format", 56),
            ],
        );
    }
}

fn dlss_options(json: &mut Json<'_>, args: &Args) {
    viewport(json, args[0]);
    if let Some(options) = read::<24>(args[1], SL_HEADER) {
        json.fields(
            &options,
            &[(c"mode", 0), (c"output_width", 4), (c"output_height", 8)],
        );
        json.float(Some(c"sharpness"), &options, 12);
        json.float(Some(c"pre_exposure"), &options, 16);
        json.float(Some(c"exposure_scale"), &options, 20);
    }
}

fn reflex_options(json: &mut Json<'_>, args: &Args) {
    if let Some(options) = read::<16>(args[0], SL_HEADER) {
        let mode = u32_at(&options, 0);
        json.number(c"mode", mode);
        json.text(
            c"mode_name",
            label(&[c"Off", c"LowLatency", c"LowLatencyWithBoost"], mode),
        );
        json.number(c"frame_limit_us", u32_at(&options, 4));
        json.flag(c"use_markers_to_optimize", options[8] != 0);
        json.number(c"virtual_key", u16_at(&options, 10));
        json.number(c"id_thread", u32_at(&options, 12));
    }
}

fn put_guid(json: &mut Json<'_>, address: u64) {
    let Some(data) = read::<16>(address, 0) else {
        return;
    };
    let mut text = [0u8; 48];
    let mut line = &mut text[..47];
    let _ = write!(
        line,
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-",
        u32_at(&data, 0),
        u16_at(&data, 4),
        u16_at(&data, 6),
        data[8],
        data[9]
    );
    for byte in &data[10..] {
        let _ = write!(line, "{byte:02X}");
    }
    if let Ok(text) = CStr::from_bytes_until_nul(&text) {
        json.text(c"riid", text);
    }
}

fn pre(entry: Entry, json: &mut Json<'_>, args: &Args) {
    match entry {
        RSF_MISC_NvapiSetSleepMode => nv_mode(json, args),
        RSF_MISC_NvapiSetLatencyMarker | RSF_MISC_NvapiD3D12SetAsyncFrameMarker => {
            nv_marker(json, args, false)
        }
        RSF_MISC_NvllSetLatencyMarker => nv_marker(json, args, true),
        RSF_MISC_NvapiSleep
        | RSF_MISC_NvllDestroyLowLatencyDevice
        | RSF_MISC_NvllGetLatency
        | RSF_MISC_SlSetD3DDevice
        | RSF_MISC_SlUpgradeInterface => json.hex(Some(c"device"), args[0]),
        RSF_MISC_NvllInitLowLatencyDevice | RSF_MISC_NvllSleep => {
            json.hex(Some(c"device"), args[0]);
            json.number(Some(c"signal_value"), args[1]);
        }
        RSF_MISC_NvllSetSleepMode => {
            json.hex(Some(c"device"), args[0]);
            if let Some(params) = read::<16>(args[1], 0) {
                unsafe {
                    rsf_census_json_bytes(
                        json.0,
                        c"params_first_16_bytes".as_ptr(),
                        params.as_ptr().cast(),
                        params.len(),
                    )
                };
            }
        }
        RSF_MISC_SlInit => sl_init(json, args),
        RSF_MISC_SlSetTag => {
            viewport(json, args[0]);
            json.hex(Some(c"command_buffer"), args[3]);
            tags(json, args[1], args[2] as u32);
        }
        RSF_MISC_SlSetTagForFrame => {
            frame(json, args[0]);
            viewport(json, args[1]);
            json.hex(Some(c"command_buffer"), args[4]);
            tags(json, args[2], args[3] as u32);
        }
        RSF_MISC_SlSetConstants => constants(json, args),
        RSF_MISC_SlEvaluateFeature => {
            feature(json, args[0] as u32);
            frame(json, args[1]);
            json.number(c"num_inputs", args[3] as u32);
            json.hex(Some(c"command_buffer"), args[4]);
        }
        RSF_MISC_SlIsFeatureSupported | RSF_MISC_SlAllocateResources | RSF_MISC_SlFreeResources => {
            feature(json, args[0] as u32)
        }
        RSF_MISC_SlDLSSGGetState => viewport(json, args[0]),
        RSF_MISC_SlDLSSGSetOptions => dlssg_options(json, args),
        RSF_MISC_SlDLSSSetOptions => dlss_options(json, args),
        RSF_MISC_SlReflexSetOptions => reflex_options(json, args),
        RSF_MISC_SlReflexSleep => frame(json, args[0]),
        RSF_MISC_SlPclSetMarker => {
            let marker = args[0] as u32;
            const MARKERS: &[&CStr] = &[
                c"SimulationStart",
                c"SimulationEnd",
                c"RenderSubmitStart",
                c"RenderSubmitEnd",
                c"PresentStart",
                c"PresentEnd",
                c"",
                c"TriggerFlash",
                c"PCLatencyPing",
            ];
            json.number(c"marker", marker);
            json.text(c"marker_name", label(MARKERS, marker));
            frame(json, args[1]);
        }
        RSF_MISC_AmdExtD3DCreateInterface => {
            json.hex(Some(c"outer"), args[0]);
            put_guid(json, args[1]);
        }
        RSF_MISC_AgsInitialize | RSF_MISC_AgsDriverExtensionsDX12CreateDevice => {
            json.open(Some(c"raw_args"), b'[');
            for &value in &args[..5] {
                json.hex(None, value);
            }
            json.close(b']');
        }
        _ => {}
    }
}

fn post(entry: Entry, json: &mut Json<'_>, args: &Args) {
    match entry {
        RSF_MISC_NvapiGetLatency => {
            json.hex(Some(c"device"), args[0]);
            if let Some(latency) = read::<16>(args[1], 0) {
                let report = u64_at(&latency, 8);
                json.flag(c"latency_report_present", report != 0);
                json.number(Some(c"first_report_frame_id"), report);
            }
        }
        RSF_MISC_SlGetNewFrameToken => {
            if let Some(token) = read::<8>(args[0], 0) {
                frame(json, u64::from_le_bytes(token));
            }
            if plausible(args[1]) {
                json.number(
                    c"requested_frame_index",
                    read::<4>(args[1], 0).map_or(0, u32::from_le_bytes),
                );
            }
        }
        RSF_MISC_AmdExtD3DCreateInterface => json.output(c"interface", args[2]),
        _ => {}
    }
}

#[inline(never)]
extern "system" fn sl_get_feature(feature_id: u32, name: *const c_char, output: *mut Ptr) -> i32 {
    let pc = caller!();
    let entry = RSF_MISC_SlGetFeatureFunction;
    let original = original!(
        entry,
        unsafe extern "system" fn(u32, *const c_char, *mut Ptr) -> i32
    );
    let mut slot = MaybeUninit::uninit();
    let call = start(&mut slot, entry, pc);
    let mut text = [0u8; 64];
    let have_name = read_name(name as u64, &mut text);
    if unsafe { (*call).full } {
        let mut json = Json(unsafe { &mut (*call).json });
        feature(&mut json, feature_id);
        json.text(
            c"function_name",
            CStr::from_bytes_until_nul(&text).unwrap_or(c""),
        );
    }
    let result = original.map_or(0, |function| unsafe { function(feature_id, name, output) });
    if result == 0
        && have_name
        && let Some(real) = read::<8>(output as u64, 0)
            .map(u64::from_le_bytes)
            .filter(|&real| real != 0)
        && let Ok(name) = CStr::from_bytes_until_nul(&text)
        && let Some(wrapper) = bind_named(c"streamline", name, real as Ptr)
    {
        unsafe { platform().write_ptr.unwrap()(output, wrapper) };
    }
    unsafe { rsf_census_call_finish(call, u64::from(result as u32)) };
    result
}

#[inline(never)]
extern "system" fn wrapper<const ENTRY: Entry>(
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
    a7: u64,
) -> u64 {
    let pc = caller!();
    type Original = unsafe extern "system" fn(u64, u64, u64, u64, u64, u64, u64, u64) -> u64;
    let original = original!(ENTRY, Original);
    let mut slot = MaybeUninit::uninit();
    let call = start(&mut slot, ENTRY, pc);
    let args = [a0, a1, a2, a3, a4, a5, a6, a7];
    if unsafe { (*call).full } {
        pre(ENTRY, &mut Json(unsafe { &mut (*call).json }), &args);
    }
    let result = original.map_or(0, |function| unsafe {
        function(a0, a1, a2, a3, a4, a5, a6, a7)
    });
    if unsafe { (*call).full } {
        post(ENTRY, &mut Json(unsafe { &mut (*call).json }), &args);
    }
    unsafe { rsf_census_call_finish(call, result) };
    result
}

pub(super) unsafe fn rsf_census_misc_bind(entries: &[u16; COUNT], output: &mut [Ptr; COUNT]) {
    let _ = SERVICES.set(Services { entries: *entries });
    *output = [
        nv_query as Ptr,
        wrapper::<RSF_MISC_NvapiSetSleepMode> as Ptr,
        wrapper::<RSF_MISC_NvapiSleep> as Ptr,
        wrapper::<RSF_MISC_NvapiSetLatencyMarker> as Ptr,
        wrapper::<RSF_MISC_NvapiGetLatency> as Ptr,
        wrapper::<RSF_MISC_NvapiD3D12SetAsyncFrameMarker> as Ptr,
        wrapper::<RSF_MISC_NvllInitialize> as Ptr,
        wrapper::<RSF_MISC_NvllInitLowLatencyDevice> as Ptr,
        wrapper::<RSF_MISC_NvllDestroyLowLatencyDevice> as Ptr,
        wrapper::<RSF_MISC_NvllSetSleepMode> as Ptr,
        wrapper::<RSF_MISC_NvllSleep> as Ptr,
        wrapper::<RSF_MISC_NvllSetLatencyMarker> as Ptr,
        wrapper::<RSF_MISC_NvllGetLatency> as Ptr,
        wrapper::<RSF_MISC_SlInit> as Ptr,
        wrapper::<RSF_MISC_SlShutdown> as Ptr,
        wrapper::<RSF_MISC_SlSetTag> as Ptr,
        wrapper::<RSF_MISC_SlSetTagForFrame> as Ptr,
        wrapper::<RSF_MISC_SlSetConstants> as Ptr,
        wrapper::<RSF_MISC_SlEvaluateFeature> as Ptr,
        wrapper::<RSF_MISC_SlGetNewFrameToken> as Ptr,
        sl_get_feature as Ptr,
        wrapper::<RSF_MISC_SlIsFeatureSupported> as Ptr,
        wrapper::<RSF_MISC_SlSetD3DDevice> as Ptr,
        wrapper::<RSF_MISC_SlUpgradeInterface> as Ptr,
        wrapper::<RSF_MISC_SlAllocateResources> as Ptr,
        wrapper::<RSF_MISC_SlFreeResources> as Ptr,
        wrapper::<RSF_MISC_SlDLSSGSetOptions> as Ptr,
        wrapper::<RSF_MISC_SlDLSSGGetState> as Ptr,
        wrapper::<RSF_MISC_SlDLSSSetOptions> as Ptr,
        wrapper::<RSF_MISC_SlReflexSleep> as Ptr,
        wrapper::<RSF_MISC_SlReflexSetOptions> as Ptr,
        wrapper::<RSF_MISC_SlPclSetMarker> as Ptr,
        wrapper::<RSF_MISC_SlPclSetOptions> as Ptr,
        wrapper::<RSF_MISC_AmdExtD3DCreateInterface> as Ptr,
        wrapper::<RSF_MISC_AgsInitialize> as Ptr,
        wrapper::<RSF_MISC_AgsDriverExtensionsDX12CreateDevice> as Ptr,
    ];
}
