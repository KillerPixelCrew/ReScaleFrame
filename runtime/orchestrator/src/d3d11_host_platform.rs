// SPDX-License-Identifier: MIT
//! Typed Windows and Rust owner calls for the host's production platform table.
use crate::auxiliary_sr::rsf_auxiliary_sr_platform;
use crate::d3d11_device_identity::{device_vendor, identity};
use crate::d3d11_host_abi::{
    DUMP_DENSE_MOTION, DUMP_DEPTH, DUMP_HDR, DUMP_RAW, Pass, rsf_d3d11h_pipeline_start,
    rsf_d3d11h_pipeline_status, rsf_d3d11h_platform, rsf_d3d11h_strings,
};
use crate::d3d11_output_platform::{
    rsf_d3d11_output_child_device, rsf_d3d11_output_device_ok, rsf_d3d11_output_is_immediate,
};
use crate::dlss_pipeline::exports::{
    rsf_core_dlss_pipeline_apply_sharpening, rsf_dlss_pipeline_get_status,
    rsf_dlss_pipeline_output_texture, rsf_dlss_pipeline_resize_output,
    rsf_dlss_pipeline_select_backend, rsf_dlss_pipeline_set_colour_correction,
    rsf_dlss_pipeline_set_colour_transport, rsf_dlss_pipeline_set_preset,
    rsf_dlss_pipeline_set_quality, rsf_dlss_pipeline_start_backend,
};
use crate::dlss_streamline::exports::rsf_dlss_native_present;
use crate::fg12_abi::rsf_streamline_host;
use crate::native_cpu::{rsf_native_cpu_dispatch, rsf_native_cpu_event};
use crate::native_fg::{
    rsf_native_fg_final, rsf_native_fg_options_set, rsf_native_fg_prepare, rsf_native_fg_present,
    rsf_native_fg_retire, rsf_native_fg_set_log,
};
use crate::native_fg_cpu::platform::{
    rsf_native_fg_cpu, rsf_native_fg_frame, rsf_native_fg_rendered_source_set,
    rsf_native_fg_submission, rsf_native_latency_window_bind,
};
use crate::native_fg_cpu_abi::rsf_native_latency_window;
use crate::native_scene::{rsf_native_scene_pass, rsf_native_scene_present_path};
use crate::native_window::{
    rsf_native_window_last_present, rsf_native_window_match, rsf_native_window_present,
    rsf_native_window_scope,
};
use crate::overlay_input::rsf_overlay_input_poll_toggle;
use crate::render_abi::rsf_render_settings;
use crate::scene_abi::rsf_observer_present_event;
use rsf_game_sys::sr_abi::{
    RSF_DLSS_PIPELINE_ABI_VERSION, rsf_dlss_pipeline_setup, rsf_dlss_pipeline_status,
};
use rsf_game_sys::{rsf_frame_record, rsf_game_cpu_event, rsf_game_log_fn};
use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::{IDXGIDeviceSubObject, IDXGISwapChain};
use windows::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, WritePrivateProfileStringW,
};
use windows::core::{IUnknown, Interface, PCWSTR};

type Raw = *mut c_void;

#[path = "d3d11_host_foreign_abi.rs"]
mod foreign;
use foreign::*;

pub(super) fn linked_platform() -> rsf_d3d11h_platform {
    rsf_d3d11h_platform {
        same_object: rsf_d3d11h_same_object,
        child_device: rsf_d3d11_output_child_device,
        is_immediate: rsf_d3d11_output_is_immediate,
        device_ok: rsf_d3d11_output_device_ok,
        immediate_context: rsf_d3d11h_immediate_context,
        device_vendor: rsf_d3d11h_device_vendor,
        chain_device: rsf_d3d11h_chain_device,
        chain_window: rsf_d3d11h_chain_window,
        chain_buffer: rsf_d3d11h_chain_buffer,
        present_install: rsf_d3d11h_guard_present_install,
        present_prepare: rsf_d3d11h_guard_present_prepare,
        present_install_scoped: rsf_d3d11h_guard_present_install_scoped,
        fg_prepare,
        fg_retire: rsf_native_fg_retire,
        fg_latency_event: rsf_d3d11h_fg_latency_event,
        fg_rendered_source_set: rsf_native_fg_rendered_source_set,
        fg_set_log: rsf_native_fg_set_log,
        fg_options_set: rsf_native_fg_options_set,
        observer_install: rsf_d3d11h_guard_observer_install,
        present_is_owner: rsf_d3d11h_guard_present_is_owner,
        chain_dispatch: rsf_d3d11h_guard_chain_dispatch,
        creation_begin: rsf_d3d11h_guard_creation_begin,
        creation_end: rsf_d3d11h_guard_creation_end,
        wrap_native: rsf_d3d11h_guard_wrap_native,
        com_identity: rsf_d3d11h_com_identity,
        chain_present: rsf_d3d11h_chain_present,
        com_release: rsf_d3d11h_com_release,
        fg_frame: rsf_native_fg_frame,
        fg_submission: rsf_native_fg_submission,
        fg_final: rsf_native_fg_final,
        fg_capture: rsf_native_sr_capture_fg,
        scene_pass: rsf_native_scene_pass,
        scene_present_path: rsf_native_scene_present_path,
        window_scope: rsf_native_window_scope,
        window_present: rsf_native_window_present,
        window_last_present: rsf_native_window_last_present,
        window_match: rsf_native_window_match,
        latency_window_bind: rsf_d3d11h_latency_window_bind,
        cpu_dispatch_fg: rsf_d3d11h_cpu_dispatch_fg,
        cpu_event: rsf_d3d11h_cpu_event,
        dlss_native_present: rsf_dlss_native_present,
        pipeline_start: rsf_d3d11h_start_pipeline,
        pipeline_resize: rsf_d3d11h_pipeline_resize,
        pipeline_select: rsf_d3d11h_pipeline_select,
        pipeline_quality: rsf_d3d11h_pipeline_quality,
        pipeline_preset: rsf_dlss_pipeline_set_preset,
        pipeline_sharpen,
        pipeline_status: rsf_d3d11h_read_pipeline_status,
        pipeline_colour: rsf_d3d11h_pipeline_colour,
        pipeline_output: rsf_dlss_pipeline_output_texture,
        sr_evaluate: rsf_native_sr_evaluate_ordered_guarded,
        dump_configure,
        dump_request: rsf_d3d11h_guard_dump_request,
        dump_requested: rsf_d3d11h_guard_dump_requested,
        dump_poll: rsf_d3d11h_guard_dump_poll,
        dump_begin: rsf_d3d11h_guard_dump_begin,
        dump_wants: rsf_d3d11h_guard_dump_wants,
        dump_inputs,
        dump_pass: rsf_d3d11h_guard_dump_pass,
        dump_texture,
        dump_result: rsf_d3d11h_guard_dump_result,
        dump_complete: rsf_d3d11h_guard_dump_complete,
        overlay_configure: rsf_d3d11h_guard_overlay_configure,
        overlay_present: rsf_d3d11h_guard_overlay_present,
        overlay_poll_toggle: rsf_overlay_input_poll_toggle,
        ini_read: rsf_d3d11h_ini_read,
        ini_write: rsf_d3d11h_ini_write,
        auxiliary: rsf_auxiliary_sr_platform(),
    }
}

unsafe extern "C" fn fg_prepare(
    user: Raw,
    chain: Raw,
    context: Raw,
    list: Raw,
    buffer: Raw,
    host: *mut rsf_streamline_host,
    session: Raw,
    sync: u32,
) {
    unsafe {
        rsf_native_fg_prepare(
            user,
            chain,
            context,
            list,
            buffer,
            host.cast(),
            session,
            sync,
        )
    };
}

unsafe extern "C" fn pipeline_sharpen(settings: *const rsf_render_settings) {
    rsf_core_dlss_pipeline_apply_sharpening(unsafe { &*settings });
}

unsafe extern "C" fn dump_configure(
    text: *const rsf_d3d11h_strings,
    log: rsf_game_log_fn,
    user: Raw,
) {
    let text = unsafe { &*text };
    unsafe {
        rsf_d3d11h_guard_dump_configure(text.captures, text.identity, text.fingerprint, log, user)
    };
}

unsafe extern "C" fn dump_inputs(
    context: Raw,
    pass: *const Pass,
    record: *const rsf_frame_record,
    original: Raw,
) {
    if unsafe { rsf_d3d11h_guard_dump_inputs_start(context, pass) } == 0 {
        return;
    }
    let pass = unsafe { &*pass };
    let colour = if original.is_null() {
        pass.color_input
    } else {
        original
    };
    for (name, description, texture, view, scale) in [
        (
            c"native_color",
            c"Native pre-tonemap scene colour",
            colour,
            DUMP_HDR,
            1.0,
        ),
        (
            c"native_temporal_mask",
            c"Native numeric temporal rejection mask before weighted normalization",
            pass.temporal_mask,
            DUMP_RAW,
            1.0,
        ),
        (
            c"native_depth",
            c"Native scene device depth",
            pass.depth,
            DUMP_DEPTH,
            1.0,
        ),
        (
            c"native_motion",
            c"Native motion in adapter-declared encoding",
            pass.motion,
            DUMP_DENSE_MOTION,
            8.0,
        ),
    ] {
        if view == DUMP_RAW && texture.is_null() {
            continue;
        }
        // A capture refusal leaves the rest of the batch eligible. A native
        // exception stopped the old batch, so the guard reports it separately.
        if unsafe {
            rsf_d3d11h_guard_dump_texture(
                context,
                record,
                name.as_ptr(),
                description.as_ptr(),
                texture,
                view,
                scale,
            )
        } < 0
        {
            break;
        }
    }
}

unsafe extern "C" fn dump_texture(
    context: Raw,
    record: *const rsf_frame_record,
    name: *const c_char,
    description: *const c_char,
    texture: Raw,
    view: u32,
    scale: f32,
) -> i32 {
    unsafe {
        rsf_d3d11h_guard_dump_texture(context, record, name, description, texture, view, scale)
    }
    .max(0)
}

unsafe extern "C" fn rsf_d3d11h_fg_latency_event(_: Raw, event: *const rsf_observer_present_event) {
    unsafe { rsf_native_fg_present(event) };
}

unsafe extern "C" fn rsf_d3d11h_latency_window_bind(session_id: u64, window: Raw) {
    let binding = rsf_native_latency_window {
        struct_size: size_of::<rsf_native_latency_window>() as u32,
        version: 1,
        session_id,
        window_key: window.addr() as u64,
    };
    let _ = unsafe { rsf_native_latency_window_bind(&binding) };
}

unsafe extern "C" fn forward_fg(_: Raw, event: *const rsf_game_cpu_event) {
    unsafe { rsf_native_fg_cpu(event) };
}

unsafe extern "C" fn rsf_d3d11h_cpu_dispatch_fg(event: *const rsf_game_cpu_event) -> i32 {
    unsafe { rsf_native_cpu_dispatch(event, Some(forward_fg), null_mut()) }
}

unsafe extern "C" fn rsf_d3d11h_cpu_event(event: *const rsf_game_cpu_event) {
    let _ = unsafe { rsf_native_cpu_event(event) };
}

unsafe extern "C" fn rsf_d3d11h_start_pipeline(
    device: Raw,
    request: *const rsf_d3d11h_pipeline_start,
) -> i32 {
    let request = unsafe { &*request };
    let setup = unsafe { &*request.setup };
    let text = unsafe { &*request.strings };
    let create = rsf_dlss_pipeline_setup {
        struct_size: size_of::<rsf_dlss_pipeline_setup>() as u32,
        abi_version: RSF_DLSS_PIPELINE_ABI_VERSION,
        streamline_directory_utf8: text.streamline,
        streamline_log_directory_utf8: null(),
        require_signature: 1,
        output_width: request.output_width,
        output_height: request.output_height,
        quality: request.quality,
        motion: setup.motion,
        log: request.log,
        log_user: request.user,
        engine: setup.engine_type,
        engine_version_utf8: text.engine_version,
        project_id_utf8: text.project,
        fsr2_directory_utf8: text.fidelityfx,
        fsr3_directory_utf8: text.fidelityfx,
        fsr4_directory_utf8: text.fidelityfx,
        xess_directory_utf8: text.xess,
        view_space_to_meters: setup.units_to_meters,
        standard_depth: u32::from(setup.depth_inverted == 0),
    };
    unsafe { rsf_dlss_pipeline_start_backend(device, &create, request.backend) }
}

unsafe extern "C" fn rsf_d3d11h_pipeline_resize(width: u32, height: u32) -> i32 {
    unsafe { rsf_dlss_pipeline_resize_output(width, height, null_mut(), null_mut()) }
}

unsafe extern "C" fn rsf_d3d11h_pipeline_select(backend: u32) -> i32 {
    unsafe { rsf_dlss_pipeline_select_backend(backend, null_mut(), null_mut()) }
}

unsafe extern "C" fn rsf_d3d11h_pipeline_quality(quality: u32) -> i32 {
    unsafe { rsf_dlss_pipeline_set_quality(quality, null_mut(), null_mut()) }
}

unsafe extern "C" fn rsf_d3d11h_read_pipeline_status(
    output: *mut rsf_d3d11h_pipeline_status,
) -> i32 {
    // The wire record contains only integer fields.
    let mut status = rsf_dlss_pipeline_status {
        struct_size: size_of::<rsf_dlss_pipeline_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let result = unsafe { rsf_dlss_pipeline_get_status(&mut status) };
    unsafe {
        output.write(rsf_d3d11h_pipeline_status {
            running: status.running,
            output_width: status.output_width,
            output_height: status.output_height,
            render_width: status.render_width,
            render_height: status.render_height,
        });
    }
    result
}

extern "C" fn rsf_d3d11h_pipeline_colour(transport: u32, correction: u32) {
    rsf_dlss_pipeline_set_colour_transport(transport);
    rsf_dlss_pipeline_set_colour_correction(correction);
}

unsafe extern "C" fn rsf_d3d11h_same_object(left: Raw, right: Raw) -> i32 {
    if right.is_null() {
        return 0;
    }
    let Some(left) = (unsafe { identity(left) }) else {
        return 0;
    };
    i32::from(unsafe { identity(right) }.is_some_and(|right| left.as_raw() == right.as_raw()))
}

unsafe extern "C" fn rsf_d3d11h_immediate_context(device: Raw) -> Raw {
    unsafe { ID3D11Device::from_raw_borrowed(&device) }
        .and_then(|device| unsafe { device.GetImmediateContext() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn rsf_d3d11h_device_vendor(device: Raw, vendor: *mut u32) -> i32 {
    let Some(id) = (unsafe { device_vendor(device) }) else {
        return 0;
    };
    unsafe { vendor.write(id) };
    1
}

unsafe extern "C" fn rsf_d3d11h_chain_device(chain: Raw) -> Raw {
    unsafe { IDXGIDeviceSubObject::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetDevice::<ID3D11Device>() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn rsf_d3d11h_chain_window(chain: Raw) -> Raw {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetDesc() }.ok())
        .map_or(null_mut(), |desc| desc.OutputWindow.0)
}

unsafe extern "C" fn rsf_d3d11h_chain_buffer(chain: Raw) -> Raw {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn rsf_d3d11h_com_identity(object: Raw) -> Raw {
    unsafe { identity(object) }.map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn rsf_d3d11h_chain_present(chain: Raw) -> Raw {
    // Log the entry from this receiver's own vtable, without dispatching it.
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .map_or(null_mut(), |chain| chain.vtable().Present as Raw)
}

unsafe extern "C" fn rsf_d3d11h_com_release(object: Raw) -> u32 {
    let Some(object) = (unsafe { IUnknown::from_raw_borrowed(&object) }) else {
        return 0;
    };
    unsafe { (object.vtable().Release)(object.as_raw()) }
}

unsafe extern "C" fn rsf_d3d11h_ini_read(
    path: *const u16,
    section: *const u16,
    key: *const u16,
    fallback: i32,
) -> i32 {
    unsafe { GetPrivateProfileIntW(PCWSTR(section), PCWSTR(key), fallback, PCWSTR(path)) }
}

unsafe extern "C" fn rsf_d3d11h_ini_write(
    path: *const u16,
    section: *const u16,
    key: *const u16,
    value: *const u16,
) -> i32 {
    i32::from(unsafe {
        WritePrivateProfileStringW(PCWSTR(section), PCWSTR(key), PCWSTR(value), PCWSTR(path))
            .is_ok()
    })
}
