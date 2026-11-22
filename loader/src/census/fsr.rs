// SPDX-License-Identifier: MIT
// FSR 2.1.2, FidelityFX API, XeSS, XeFG and XeLL diagnostic wrappers.
#![allow(non_upper_case_globals)]
use super::decode::rsf_census_dxgi_format_name;
use super::hooks::rsf_census_hook_original;
use super::json::{
    Json, emit_decimal, rsf_census_json_bytes, rsf_census_json_string, rsf_census_json_wide,
};
use super::platform;
use super::rsf_census_call_state;
use super::session::{rsf_census_call_begin, rsf_census_call_finish};
use crate::sys::*;
use std::ffi::{CStr, c_void};
use std::mem::{MaybeUninit, transmute};
use std::ptr::null_mut;
use std::sync::OnceLock;
use windows::Win32::Graphics::Dxgi::{DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_DESC1};
use windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;

type Ptr = *mut c_void;
type Args = [u64; 8];
type Entry = rsf_census_fsr_entry;
const COUNT: usize = RSF_FSR_COUNT as usize;
const FSR2_RESOURCE: usize = 184;
const FSR2_DISPATCH: usize = 1352;
const API_RESOURCE: usize = 48;
const RESOURCE_KEYS: [&CStr; 7] = [
    c"color",
    c"depth",
    c"motion_vectors",
    c"exposure",
    c"reactive",
    c"transparency_and_composition",
    c"output",
];

struct Services {
    entries: [u16; COUNT],
}
static SERVICES: OnceLock<Services> = OnceLock::new();
fn id(entry: Entry) -> u16 {
    SERVICES.get().unwrap().entries[entry as usize]
}

macro_rules! caller {
    () => {{
        let mut caller = null_mut();
        unsafe { RtlCaptureStackBackTrace(1, 1, &mut caller, null_mut()) };
        caller
    }};
}
macro_rules! original {
    ($entry:expr, $signature:ty) => {{
        let target = rsf_census_hook_original(id($entry));
        if target.is_null() {
            None
        } else {
            Some(unsafe { transmute::<Ptr, $signature>(target) })
        }
    }};
}
fn start(
    slot: &mut MaybeUninit<rsf_census_call_state>,
    entry: Entry,
    caller: Ptr,
) -> *mut rsf_census_call_state {
    let call = slot.as_mut_ptr();
    unsafe { rsf_census_call_begin(call, id(entry), caller) };
    call
}

fn plausible(address: u64) -> bool {
    (0x1_0000..0x0000_7fff_ffff_0000).contains(&address)
}
fn read<const N: usize>(address: u64) -> Option<[u8; N]> {
    if !plausible(address) {
        return None;
    }
    let mut bytes = [0; N];
    unsafe { platform().read.unwrap()(address as _, bytes.as_mut_ptr().cast(), N) }.then_some(bytes)
}
fn dword(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}
fn word(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}
fn float(data: &[u8], offset: usize) -> f32 {
    f32::from_bits(dword(data, offset))
}
fn pointed_word(address: u64) -> Option<u64> {
    read::<8>(address).map(u64::from_le_bytes)
}

trait FsrJson {
    fn flag(&mut self, key: &CStr, on: bool);
    fn decimal(&mut self, key: &CStr, value: f32);
    fn fields(&mut self, data: &[u8], fields: &[(&CStr, usize)]);
    fn decimals(&mut self, data: &[u8], fields: &[(&CStr, usize)]);
    fn ptr(&mut self, key: &CStr, address: u64);
    fn output(&mut self, key: &CStr, address: u64);
    fn resource(&mut self, key: &CStr, address: u64, require_slot: bool);
    fn args(&mut self, args: &[u64]);
}

impl FsrJson for Json<'_> {
    fn flag(&mut self, key: &CStr, on: bool) {
        self.raw(key, if on { c"true" } else { c"false" });
    }
    fn decimal(&mut self, key: &CStr, value: f32) {
        unsafe {
            emit_decimal(
                self.0,
                key.as_ptr(),
                value.into(),
                platform().decimal.unwrap(),
            )
        };
    }
    fn fields(&mut self, data: &[u8], fields: &[(&CStr, usize)]) {
        for &(key, offset) in fields {
            self.number(key, dword(data, offset));
        }
    }
    fn decimals(&mut self, data: &[u8], fields: &[(&CStr, usize)]) {
        for &(key, offset) in fields {
            self.decimal(key, float(data, offset));
        }
    }
    fn ptr(&mut self, key: &CStr, address: u64) {
        if address == 0 {
            self.raw(key, c"null");
        } else {
            self.hex(Some(key), address);
        }
    }
    fn output(&mut self, key: &CStr, address: u64) {
        if let Some(value) = pointed_word(address) {
            self.hex(Some(key), value);
        } else {
            self.raw(key, c"null");
        }
    }
    fn resource(&mut self, key: &CStr, address: u64, require_slot: bool) {
        if require_slot
            && (address == 0 || unsafe { platform().slot.unwrap()(address as _, 10) }.is_null())
        {
            return;
        }
        unsafe {
            super::decode::rsf_census_json_resource(
                self.0,
                key.as_ptr(),
                address as _,
                true,
                platform(),
            )
        };
    }
    fn args(&mut self, args: &[u64]) {
        self.open(Some(c"raw_args"), b'[');
        for &value in args {
            self.hex(None, value);
        }
        self.close(b']');
    }
}

fn fsr2_name(resource: &[u8]) -> [u16; 65] {
    let mut name = [0u16; 65];
    for (unit, bytes) in name.iter_mut().zip(resource[8..136].chunks_exact(2)) {
        *unit = u16::from_le_bytes(bytes.try_into().unwrap());
    }
    name
}

fn ascii_name_ok(resource: &[u8]) -> bool {
    for unit in fsr2_name(resource).into_iter().take(64) {
        if unit == 0 {
            return true;
        }
        if !(0x20..=0x7e).contains(&unit) {
            return false;
        }
    }
    false
}

fn finite(data: &[u8], offset: usize, cap: f32) -> bool {
    float(data, offset).abs() <= cap
}

fn fsr2_check(data: &[u8; FSR2_DISPATCH]) -> Option<&'static CStr> {
    let plausible_resources = data[8..1296]
        .chunks_exact(FSR2_RESOURCE)
        .filter(|resource| {
            ascii_name_ok(resource)
                && dword(resource, 136) <= 3
                && dword(resource, 140) < 64
                && dword(resource, 144) <= 16_384
                && dword(resource, 148) <= 16_384
                && dword(resource, 164) >> 16 == 0
                && resource[168] <= 1
        })
        .count();
    if plausible_resources < 5 {
        return Some(c"resource_names_or_descriptions_implausible");
    }
    if word(data, 8) == 0 || word(data, 1112) == 0 {
        return Some(c"color_or_output_null");
    }
    if !finite(data, 1296, 16.0) || !finite(data, 1300, 16.0) {
        return Some(c"jitter_implausible");
    }
    if !finite(data, 1304, 1e6) || !finite(data, 1308, 1e6) {
        return Some(c"motion_vector_scale_implausible");
    }
    if ![1312, 1316]
        .into_iter()
        .all(|offset| (1..=16_384).contains(&dword(data, offset)))
    {
        return Some(c"render_size_implausible");
    }
    if data[1320] > 1 || data[1336] > 1 {
        return Some(c"bool_implausible");
    }
    if !finite(data, 1324, 8.0) || !finite(data, 1328, 1e6) || !finite(data, 1332, 1e9) {
        return Some(c"scalar_implausible");
    }
    if !finite(data, 1340, 1e9) || !finite(data, 1344, 1e12) || !finite(data, 1348, 7.0) {
        return Some(c"camera_implausible");
    }
    None
}

fn fsr2_resource(json: &mut Json<'_>, key: &CStr, resource: &[u8]) {
    json.open(Some(key), b'{');
    let address = word(resource, 0);
    json.ptr(c"ptr", address);
    let name = fsr2_name(resource);
    unsafe { rsf_census_json_wide(json.0, c"name".as_ptr(), name.as_ptr()) };
    json.fields(
        resource,
        &[
            (c"type", 136),
            (c"format", 140),
            (c"width", 144),
            (c"height", 148),
            (c"depth", 152),
            (c"mips", 156),
        ],
    );
    json.hex(Some(c"flags"), dword(resource, 160).into());
    json.hex(Some(c"state"), dword(resource, 164).into());
    json.flag(c"is_depth", resource[168] != 0);
    json.resource(c"d3d12", address, true);
    json.close(b'}');
}

fn fsr2_dispatch(json: &mut Json<'_>, args: &Args) {
    json.hex(Some(c"context"), args[0]);
    json.text(
        Some(c"layout"),
        c"fsr2-2.1.2 (dispatch description 1352 bytes)",
    );
    let Some(data) = read::<FSR2_DISPATCH>(args[1]) else {
        json.text(Some(c"layout_check"), c"unreadable");
        json.number(c"raw_bytes_len", 0u32);
        return;
    };
    if let Some(reason) = fsr2_check(&data) {
        json.text(Some(c"layout_check"), c"failed");
        json.text(Some(c"reason"), reason);
        json.number(c"raw_bytes_len", FSR2_DISPATCH as u64);
        return;
    }
    json.text(Some(c"layout_check"), c"ok");
    json.hex(Some(c"command_list"), word(&data, 0));
    json.open(Some(c"resources"), b'{');
    for (key, resource) in RESOURCE_KEYS
        .into_iter()
        .zip(data[8..1296].chunks_exact(FSR2_RESOURCE))
    {
        fsr2_resource(json, key, resource);
    }
    json.close(b'}');
    json.open(Some(c"scalars"), b'{');
    json.decimals(
        &data,
        &[
            (c"jitter_x", 1296),
            (c"jitter_y", 1300),
            (c"mv_scale_x", 1304),
            (c"mv_scale_y", 1308),
        ],
    );
    json.fields(&data, &[(c"render_width", 1312), (c"render_height", 1316)]);
    json.flag(c"enable_sharpening", data[1320] != 0);
    json.decimals(
        &data,
        &[
            (c"sharpness", 1324),
            (c"frame_time_delta_ms", 1328),
            (c"pre_exposure", 1332),
        ],
    );
    json.flag(c"reset", data[1336] != 0);
    json.decimals(
        &data,
        &[
            (c"camera_near", 1340),
            (c"camera_far", 1344),
            (c"camera_fov_vertical", 1348),
        ],
    );
    json.close(b'}');
}

fn fsr2_create(json: &mut Json<'_>, args: &Args) {
    json.hex(Some(c"context"), args[0]);
    if let Some(data) = read::<28>(args[1]) {
        let flags = dword(&data, 0);
        json.hex(Some(c"flags"), flags.into());
        json.open(Some(c"flag_names"), b'[');
        for (index, key) in [
            c"HDR",
            c"DisplayResolutionMotionVectors",
            c"MotionVectorsJitterCancellation",
            c"DepthInverted",
            c"DepthInfinite",
            c"AutoExposure",
            c"DynamicResolution",
            c"Texture1DUsage",
        ]
        .into_iter()
        .enumerate()
        {
            if flags & (1 << index) != 0 {
                json.text(None, key);
            }
        }
        json.close(b']');
        json.fields(
            &data,
            &[
                (c"max_render_width", 4),
                (c"max_render_height", 8),
                (c"display_width", 12),
                (c"display_height", 16),
            ],
        );
    }
}

fn api_type_name(kind: u64) -> Option<&'static CStr> {
    Some(match kind {
        0x00010000 => c"create_upscale",
        0x00010001 => c"dispatch_upscale",
        0x00010002 => c"query_upscale_ratio",
        0x00010003 => c"query_render_resolution",
        0x00010004 => c"query_jitter_phase_count",
        0x00010005 => c"query_jitter_offset",
        0x00010006 => c"dispatch_generate_reactive_mask",
        0x00010007 => c"configure_upscale_keyvalue",
        0x0001000b => c"create_upscale_version",
        0x00020001 => c"create_framegeneration",
        0x00020002 => c"configure_framegeneration",
        0x00020003 => c"dispatch_framegeneration",
        0x00020004 => c"dispatch_framegeneration_prepare",
        0x00020005 => c"callback_framegeneration_present",
        0x00020006 => c"configure_framegeneration_keyvalue",
        0x00020008 => c"configure_register_distortion",
        0x00020009 => c"create_framegeneration_hudless",
        0x0002000a => c"dispatch_framegeneration_prepare_camerainfo",
        0x0002000c => c"dispatch_framegeneration_prepare_v2",
        0x00000002 => c"create_backend_dx12",
        0x00000003 => c"create_backend_dx12_allocation_callbacks",
        0x00030001 => c"create_swapchain_wrap_dx12",
        0x00030002 => c"configure_swapchain_register_ui_dx12",
        0x00030005 => c"create_swapchain_new_dx12",
        0x00030006 => c"create_swapchain_for_hwnd_dx12",
        0x00030007 => c"dispatch_swapchain_wait_for_presents_dx12",
        0x00030008 => c"configure_swapchain_keyvalue_dx12",
        0x0003000b => c"create_swapchain_version_dx12",
        0x00000007 => c"configure_global_debug",
        _ => return None,
    })
}

fn api_resource(json: &mut Json<'_>, key: &CStr, data: &[u8]) {
    json.open(Some(key), b'{');
    let address = word(data, 0);
    json.ptr(c"ptr", address);
    json.fields(
        data,
        &[
            (c"type", 8),
            (c"format", 12),
            (c"width", 16),
            (c"height", 20),
            (c"depth", 24),
            (c"mips", 28),
        ],
    );
    json.hex(Some(c"flags"), dword(data, 32).into());
    json.hex(Some(c"usage"), dword(data, 36).into());
    json.hex(Some(c"state"), dword(data, 40).into());
    json.resource(c"d3d12", address, true);
    json.close(b'}');
}

// The requested Windows structure must accept arbitrary native bit patterns.
unsafe fn sdk_desc<T: Default>(address: u64) -> Option<T> {
    let mut value = T::default();
    unsafe { platform().read.unwrap()(address as _, (&raw mut value).cast(), size_of::<T>()) }
        .then_some(value)
}

fn chain_desc(json: &mut Json<'_>, address: u64, legacy: bool) {
    if !plausible(address) {
        return;
    }
    json.open(
        Some(if legacy {
            c"swapchain_desc"
        } else {
            c"swapchain_desc1"
        }),
        b'{',
    );
    let fields = if legacy {
        unsafe { sdk_desc::<DXGI_SWAP_CHAIN_DESC>(address) }.map(|d| {
            (
                d.BufferDesc.Width,
                d.BufferDesc.Height,
                d.BufferDesc.Format.0,
                d.BufferCount,
                d.SwapEffect.0,
                d.Flags,
            )
        })
    } else {
        unsafe { sdk_desc::<DXGI_SWAP_CHAIN_DESC1>(address) }.map(|d| {
            (
                d.Width,
                d.Height,
                d.Format.0,
                d.BufferCount,
                d.SwapEffect.0,
                d.Flags,
            )
        })
    };
    if let Some((width, height, format, buffers, effect, flags)) = fields {
        json.number(c"width", width);
        json.number(c"height", height);
        json.number(c"format", format as u32);
        unsafe {
            rsf_census_json_string(
                json.0,
                c"format_name".as_ptr(),
                rsf_census_dxgi_format_name(format as u32),
            )
        };
        json.number(c"buffer_count", buffers);
        json.number(c"swap_effect", effect as u32);
        json.hex(Some(c"flags"), flags.into());
    }
    json.close(b'}');
}

fn dispatch_upscale(json: &mut Json<'_>, data: &[u8; 432]) {
    json.hex(Some(c"command_list"), word(data, 16));
    json.open(Some(c"resources"), b'{');
    for (key, resource) in RESOURCE_KEYS
        .into_iter()
        .zip(data[24..360].chunks_exact(API_RESOURCE))
    {
        api_resource(json, key, resource);
    }
    json.close(b'}');
    json.open(Some(c"scalars"), b'{');
    json.decimals(
        data,
        &[
            (c"jitter_x", 360),
            (c"jitter_y", 364),
            (c"mv_scale_x", 368),
            (c"mv_scale_y", 372),
        ],
    );
    json.fields(
        data,
        &[
            (c"render_width", 376),
            (c"render_height", 380),
            (c"upscale_width", 384),
            (c"upscale_height", 388),
        ],
    );
    json.flag(c"enable_sharpening", data[392] != 0);
    json.decimals(
        data,
        &[
            (c"sharpness", 396),
            (c"frame_time_delta_ms", 400),
            (c"pre_exposure", 404),
        ],
    );
    json.flag(c"reset", data[408] != 0);
    json.decimals(
        data,
        &[
            (c"camera_near", 412),
            (c"camera_far", 416),
            (c"camera_fov_vertical", 420),
            (c"view_space_to_meters", 424),
        ],
    );
    json.hex(Some(c"flags"), dword(data, 428).into());
    json.close(b'}');
}

fn framegen_dispatch(json: &mut Json<'_>, data: &[u8; 312]) {
    json.hex(Some(c"command_list"), word(data, 16));
    api_resource(json, c"present_color", &data[24..72]);
    api_resource(json, c"output0", &data[72..120]);
    json.number(c"num_generated_frames", dword(data, 264));
    json.flag(c"reset", data[268] != 0);
    json.number(c"backbuffer_transfer_function", dword(data, 272));
    json.decimals(data, &[(c"min_luminance", 276), (c"max_luminance", 280)]);
    json.number(c"frame_id", word(data, 304));
}

fn framegen_configure(json: &mut Json<'_>, data: &[u8; 144]) {
    for (key, offset) in [
        (c"swap_chain", 16),
        (c"present_callback", 24),
        (c"frame_generation_callback", 40),
    ] {
        json.ptr(key, word(data, offset));
    }
    json.flag(c"frame_generation_enabled", data[56] != 0);
    json.flag(c"allow_async_workloads", data[57] != 0);
    api_resource(json, c"hudless_color", &data[64..112]);
    json.hex(Some(c"flags"), dword(data, 112).into());
    json.flag(c"only_present_generated", data[116] != 0);
    json.number(c"frame_id", word(data, 136));
}

fn framegen_prepare(json: &mut Json<'_>, data: &[u8; 232]) {
    json.number(c"frame_id", word(data, 16));
    json.hex(Some(c"flags"), dword(data, 24).into());
    json.hex(Some(c"command_list"), word(data, 32));
    json.fields(data, &[(c"render_width", 40), (c"render_height", 44)]);
    json.decimals(
        data,
        &[
            (c"jitter_x", 48),
            (c"jitter_y", 52),
            (c"mv_scale_x", 56),
            (c"mv_scale_y", 60),
            (c"frame_time_delta_ms", 64),
        ],
    );
    json.flag(c"reset", data[68] != 0);
    json.decimals(
        data,
        &[
            (c"camera_near", 72),
            (c"camera_far", 76),
            (c"camera_fov_vertical", 80),
        ],
    );
    api_resource(json, c"depth", &data[88..136]);
    api_resource(json, c"motion_vectors", &data[136..184]);
}

fn api_payload(json: &mut Json<'_>, kind: u64, address: u64) {
    match kind {
        0x0001_0000 => {
            if let Some(data) = read::<40>(address) {
                json.hex(Some(c"flags"), dword(&data, 16).into());
                json.fields(
                    &data,
                    &[
                        (c"max_render_width", 20),
                        (c"max_render_height", 24),
                        (c"max_upscale_width", 28),
                        (c"max_upscale_height", 32),
                    ],
                );
            }
        }
        0x0002_0001 => {
            if let Some(data) = read::<40>(address) {
                json.hex(Some(c"flags"), dword(&data, 16).into());
                json.fields(
                    &data,
                    &[
                        (c"display_width", 20),
                        (c"display_height", 24),
                        (c"max_render_width", 28),
                        (c"max_render_height", 32),
                        (c"back_buffer_format", 36),
                    ],
                );
            }
        }
        0x0003_0001 => {
            if let Some(data) = read::<32>(address) {
                json.ptr(c"game_queue", word(&data, 24));
                json.output(c"input_swapchain", word(&data, 16));
            }
        }
        0x0003_0005 => {
            if let Some(data) = read::<48>(address) {
                json.ptr(c"game_queue", word(&data, 40));
                json.ptr(c"factory", word(&data, 32));
                chain_desc(json, word(&data, 24), true);
            }
        }
        0x0003_0006 => {
            if let Some(data) = read::<64>(address) {
                json.ptr(c"hwnd", word(&data, 24));
                json.ptr(c"game_queue", word(&data, 56));
                json.ptr(c"factory", word(&data, 48));
                chain_desc(json, word(&data, 32), false);
            }
        }
        0x0000_0002 => {
            if let Some(data) = read::<24>(address) {
                json.ptr(c"device", word(&data, 16));
            }
        }
        0x0001_0001 => {
            if let Some(data) = read::<432>(address) {
                dispatch_upscale(json, &data);
            }
        }
        0x0002_0003 => {
            if let Some(data) = read::<312>(address) {
                framegen_dispatch(json, &data);
            }
        }
        0x0002_0002 => {
            if let Some(data) = read::<144>(address) {
                framegen_configure(json, &data);
            }
        }
        0x0002_000c => {
            if let Some(data) = read::<232>(address) {
                framegen_prepare(json, &data);
            }
        }
        _ => {}
    }
}

fn api_chain(json: &mut Json<'_>, mut address: u64) {
    json.open(Some(c"chain"), b'[');
    for _ in 0..12 {
        let Some(header) = read::<16>(address) else {
            break;
        };
        let kind = word(&header, 0);
        json.open(None, b'{');
        json.hex(Some(c"type"), kind);
        if let Some(name) = api_type_name(kind) {
            json.text(Some(c"name"), name);
        }
        api_payload(json, kind, address);
        json.close(b'}');
        address = word(&header, 8);
    }
    json.close(b']');
}

fn xess_init(json: &mut Json<'_>, args: &Args) {
    json.hex(Some(c"context"), args[0]);
    if let Some(data) = read::<24>(args[1]) {
        json.fields(
            &data,
            &[
                (c"output_width", 0),
                (c"output_height", 4),
                (c"quality_setting", 8),
            ],
        );
        json.hex(Some(c"init_flags"), dword(&data, 12).into());
        json.fields(
            &data,
            &[(c"creation_node_mask", 16), (c"visible_node_mask", 20)],
        );
    }
}

fn xess_execute(json: &mut Json<'_>, args: &Args) {
    json.hex(Some(c"context"), args[0]);
    json.hex(Some(c"command_list"), args[1]);
    let Some(data) = read::<136>(args[2]) else {
        json.flag(c"params_unreadable", true);
        return;
    };
    json.open(Some(c"resources"), b'{');
    for (index, key) in [
        c"color",
        c"velocity",
        c"depth",
        c"exposure_scale",
        c"responsive_mask",
        c"output",
    ]
    .into_iter()
    .enumerate()
    {
        json.resource(key, word(&data, index * 8), false);
    }
    json.close(b'}');
    json.open(Some(c"scalars"), b'{');
    json.decimals(
        &data,
        &[
            (c"jitter_x", 48),
            (c"jitter_y", 52),
            (c"exposure_scale", 56),
        ],
    );
    json.fields(
        &data,
        &[
            (c"reset_history", 60),
            (c"input_width", 64),
            (c"input_height", 68),
        ],
    );
    json.close(b'}');
}

fn pre(entry: Entry, json: &mut Json<'_>, args: &Args) {
    match entry {
        RSF_FSR_Fsr2ContextCreate => fsr2_create(json, args),
        RSF_FSR_Fsr2ContextDispatch => fsr2_dispatch(json, args),
        RSF_FSR_Fsr2GetJitterOffset => {
            json.signed(c"index", args[2] as i32);
            json.signed(c"phase_count", args[3] as i32);
        }
        RSF_FSR_Fsr2GetJitterPhaseCount => {
            json.signed(c"render_width", args[0] as i32);
            json.signed(c"display_width", args[1] as i32);
        }
        RSF_FSR_Fsr2GetRenderResolution => {
            json.number(c"display_width", args[2] as u32);
            json.number(c"display_height", args[3] as u32);
            json.signed(c"quality_mode", args[4] as i32);
        }
        RSF_FSR_FfxCreateContext => {
            json.hex(Some(c"context_out"), args[0]);
            api_chain(json, args[1]);
        }
        RSF_FSR_FfxConfigure
        | RSF_FSR_FfxQuery
        | RSF_FSR_FfxDispatch
        | RSF_FSR_FfxDestroyContext => {
            json.hex(Some(c"context"), pointed_word(args[0]).unwrap_or(0));
            if entry != RSF_FSR_FfxDestroyContext {
                api_chain(json, args[1]);
            }
        }
        RSF_FSR_XessD3D12CreateContext | RSF_FSR_XessD3D11CreateContext => {
            json.hex(Some(c"device"), args[0])
        }
        RSF_FSR_XessD3D12Init => xess_init(json, args),
        RSF_FSR_XessD3D12Execute => xess_execute(json, args),
        RSF_FSR_XellSleep | RSF_FSR_XellAddMarkerData => {
            json.hex(Some(c"context"), args[0]);
            json.number(c"frame_id", args[1] as u32);
            if entry == RSF_FSR_XellAddMarkerData {
                json.number(c"marker", args[2] as u32);
            }
        }
        RSF_FSR_XellSetSleepMode | RSF_FSR_XellGetSleepMode => {
            json.hex(Some(c"context"), args[0]);
            if let Some(data) = read::<16>(args[1]) {
                unsafe {
                    rsf_census_json_bytes(
                        json.0,
                        c"params_first_16_bytes".as_ptr(),
                        data.as_ptr().cast(),
                        data.len(),
                    )
                };
            }
        }
        RSF_FSR_Fsr2ContextGenerateReactiveMask
        | RSF_FSR_Fsr2ContextDestroy
        | RSF_FSR_FfxGetCommandListDX12
        | RSF_FSR_FfxGetDeviceDX12
        | RSF_FSR_XessDestroyContext
        | RSF_FSR_XefgSetEnabled
        | RSF_FSR_XefgSetPresentId
        | RSF_FSR_XefgSetLatencyReduction
        | RSF_FSR_XefgSetNumInterpolatedFrames
        | RSF_FSR_XefgDestroy
        | RSF_FSR_XellD3D12CreateContext
        | RSF_FSR_XellDestroyContext => json.args(&args[..2]),
        RSF_FSR_Fsr2GetInterfaceDX12
        | RSF_FSR_XessD3D11Init
        | RSF_FSR_XessD3D11Execute
        | RSF_FSR_XessVKCreateContext
        | RSF_FSR_XessVKInit
        | RSF_FSR_XessVKExecute
        | RSF_FSR_XefgCreateContext
        | RSF_FSR_XefgTagFrameConstants => json.args(&args[..4]),
        RSF_FSR_XefgInitFromSwapChain
        | RSF_FSR_XefgInitFromSwapChainDesc
        | RSF_FSR_XefgTagFrameResource
        | RSF_FSR_XefgSetUiCompositionState => json.args(&args[..6]),
        _ => {}
    }
}

fn post(entry: Entry, json: &mut Json<'_>, args: &Args) {
    match entry {
        RSF_FSR_Fsr2GetJitterOffset => {
            for (key, address) in [(c"jitter_x", args[0]), (c"jitter_y", args[1])] {
                if let Some(data) = read::<4>(address) {
                    json.decimal(key, float(&data, 0));
                }
            }
        }
        RSF_FSR_Fsr2GetRenderResolution => {
            for (key, address) in [(c"render_width", args[0]), (c"render_height", args[1])] {
                if let Some(data) = read::<4>(address) {
                    json.number(key, u32::from_le_bytes(data));
                }
            }
        }
        RSF_FSR_FfxGetResourceDX12 => {
            // The hidden output pointer for the structure return occupies the first integer word.
            json.args(&args[..5]);
            if let Some(data) = read::<FSR2_RESOURCE>(args[0])
                && ascii_name_ok(&data)
                && dword(&data, 136) <= 3
            {
                fsr2_resource(json, c"returned_resource", &data);
            }
        }
        RSF_FSR_FfxCreateContext => json.output(c"context", args[0]),
        RSF_FSR_XessD3D12CreateContext | RSF_FSR_XessD3D11CreateContext => {
            json.output(c"context", args[1])
        }
        _ => {}
    }
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
    let original = original!(
        ENTRY,
        unsafe extern "system" fn(u64, u64, u64, u64, u64, u64, u64, u64) -> u64
    );
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

fn xess_scale(entry: Entry, pc: Ptr, context: Ptr, x: f32, y: f32) -> i32 {
    let original = original!(entry, unsafe extern "system" fn(Ptr, f32, f32) -> i32);
    let mut slot = MaybeUninit::uninit();
    let call = start(&mut slot, entry, pc);
    if unsafe { (*call).full } {
        let mut json = Json(unsafe { &mut (*call).json });
        json.hex(Some(c"context"), context as u64);
        json.decimal(c"x", x);
        json.decimal(c"y", y);
    }
    let result = original.map_or(0, |function| unsafe { function(context, x, y) });
    unsafe { rsf_census_call_finish(call, u64::from(result as u32)) };
    result
}

#[inline(never)]
extern "system" fn jitter_scale(context: Ptr, x: f32, y: f32) -> i32 {
    xess_scale(RSF_FSR_XessSetJitterScale, caller!(), context, x, y)
}

#[inline(never)]
extern "system" fn velocity_scale(context: Ptr, x: f32, y: f32) -> i32 {
    xess_scale(RSF_FSR_XessSetVelocityScale, caller!(), context, x, y)
}

#[inline(never)]
extern "system" fn exposure_scale(context: Ptr, scale: f32) -> i32 {
    let pc = caller!();
    let entry = RSF_FSR_XessSetExposureMultiplier;
    let original = original!(entry, unsafe extern "system" fn(Ptr, f32) -> i32);
    let mut slot = MaybeUninit::uninit();
    let call = start(&mut slot, entry, pc);
    if unsafe { (*call).full } {
        let mut json = Json(unsafe { &mut (*call).json });
        json.hex(Some(c"context"), context as u64);
        json.decimal(c"scale", scale);
    }
    let result = original.map_or(0, |function| unsafe { function(context, scale) });
    unsafe { rsf_census_call_finish(call, u64::from(result as u32)) };
    result
}

#[inline(never)]
extern "system" fn upscale_ratio(mode: i32) -> f32 {
    let pc = caller!();
    let entry = RSF_FSR_Fsr2GetUpscaleRatio;
    let original = original!(entry, unsafe extern "system" fn(i32) -> f32);
    let mut slot = MaybeUninit::uninit();
    let call = start(&mut slot, entry, pc);
    if unsafe { (*call).full } {
        Json(unsafe { &mut (*call).json }).signed(c"quality_mode", mode);
    }
    let result = original.map_or(0.0, |function| unsafe { function(mode) });
    if unsafe { (*call).full } {
        Json(unsafe { &mut (*call).json }).decimal(c"ratio", result);
    }
    unsafe { rsf_census_call_finish(call, 0) };
    result
}

pub(super) unsafe fn rsf_census_fsr_bind(entries: &[u16; COUNT], output: &mut [Ptr; COUNT]) {
    let _ = SERVICES.set(Services { entries: *entries });
    *output = [
        wrapper::<RSF_FSR_Fsr2ContextCreate> as Ptr,
        wrapper::<RSF_FSR_Fsr2ContextDispatch> as Ptr,
        wrapper::<RSF_FSR_Fsr2ContextGenerateReactiveMask> as Ptr,
        wrapper::<RSF_FSR_Fsr2ContextDestroy> as Ptr,
        wrapper::<RSF_FSR_Fsr2GetJitterOffset> as Ptr,
        wrapper::<RSF_FSR_Fsr2GetJitterPhaseCount> as Ptr,
        wrapper::<RSF_FSR_Fsr2GetRenderResolution> as Ptr,
        upscale_ratio as Ptr,
        wrapper::<RSF_FSR_Fsr2GetInterfaceDX12> as Ptr,
        wrapper::<RSF_FSR_Fsr2GetScratchMemorySizeDX12> as Ptr,
        wrapper::<RSF_FSR_FfxGetResourceDX12> as Ptr,
        wrapper::<RSF_FSR_FfxGetCommandListDX12> as Ptr,
        wrapper::<RSF_FSR_FfxGetDeviceDX12> as Ptr,
        wrapper::<RSF_FSR_FfxCreateContext> as Ptr,
        wrapper::<RSF_FSR_FfxDestroyContext> as Ptr,
        wrapper::<RSF_FSR_FfxConfigure> as Ptr,
        wrapper::<RSF_FSR_FfxQuery> as Ptr,
        wrapper::<RSF_FSR_FfxDispatch> as Ptr,
        wrapper::<RSF_FSR_XessD3D12CreateContext> as Ptr,
        wrapper::<RSF_FSR_XessD3D12Init> as Ptr,
        wrapper::<RSF_FSR_XessD3D12Execute> as Ptr,
        wrapper::<RSF_FSR_XessD3D11CreateContext> as Ptr,
        wrapper::<RSF_FSR_XessD3D11Init> as Ptr,
        wrapper::<RSF_FSR_XessD3D11Execute> as Ptr,
        wrapper::<RSF_FSR_XessVKCreateContext> as Ptr,
        wrapper::<RSF_FSR_XessVKInit> as Ptr,
        wrapper::<RSF_FSR_XessVKExecute> as Ptr,
        jitter_scale as Ptr,
        velocity_scale as Ptr,
        exposure_scale as Ptr,
        wrapper::<RSF_FSR_XessDestroyContext> as Ptr,
        wrapper::<RSF_FSR_XefgCreateContext> as Ptr,
        wrapper::<RSF_FSR_XefgInitFromSwapChain> as Ptr,
        wrapper::<RSF_FSR_XefgInitFromSwapChainDesc> as Ptr,
        wrapper::<RSF_FSR_XefgTagFrameResource> as Ptr,
        wrapper::<RSF_FSR_XefgTagFrameConstants> as Ptr,
        wrapper::<RSF_FSR_XefgSetEnabled> as Ptr,
        wrapper::<RSF_FSR_XefgSetPresentId> as Ptr,
        wrapper::<RSF_FSR_XefgSetLatencyReduction> as Ptr,
        wrapper::<RSF_FSR_XefgSetNumInterpolatedFrames> as Ptr,
        wrapper::<RSF_FSR_XefgSetUiCompositionState> as Ptr,
        wrapper::<RSF_FSR_XefgDestroy> as Ptr,
        wrapper::<RSF_FSR_XellD3D12CreateContext> as Ptr,
        wrapper::<RSF_FSR_XellDestroyContext> as Ptr,
        wrapper::<RSF_FSR_XellSetSleepMode> as Ptr,
        wrapper::<RSF_FSR_XellGetSleepMode> as Ptr,
        wrapper::<RSF_FSR_XellSleep> as Ptr,
        wrapper::<RSF_FSR_XellAddMarkerData> as Ptr,
    ];
}
