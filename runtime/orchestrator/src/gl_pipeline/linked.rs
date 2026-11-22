// SPDX-License-Identifier: MIT
//! Bind the production OpenGL pipeline to its sibling runtime owners.
use super::*;
use crate::fg12_abi::rsf_native_latency_frame;
use crate::native_fg_cpu_abi::rsf_streamline_latency_status;
use crate::{d3d11_present as present, fg12, gl_shared_surface as gl, sr12};

unsafe extern "C" {
    fn rsf_d3d11_present_is_owner(chain: Handle) -> c_int;
    fn rsf_d3d11_present_abort(frame: u64) -> i32;
    fn rsf_streamline_host_latency_status(
        host: Handle,
        status: *mut rsf_streamline_latency_status,
    ) -> i32;
    fn rsf_native_latency_frame_get(out: *mut rsf_native_latency_frame) -> c_int;
    fn rsf_native_latency_end_session(session: u64);
    fn rsf_input_dump_cancel(session: u64);
    fn rsf_gl_contract_dlss_preset_valid(preset: u32) -> c_int;
    fn rsf_gl_contract_fg_default_backend(vendor: u32, software: u32) -> u32;
    fn rsf_gl_contract_depth_domains_requested(pass: *const Pass) -> u32;
    fn rsf_overlay_configure_bound(setup: *const rsf_overlay_service_setup) -> i32;
    fn rsf_overlay_present_bound(
        device: Handle,
        queue: Handle,
        chain: Handle,
        backbuffer: Handle,
        window: Handle,
        log: Log,
        user: Handle,
    ) -> i32;
    fn rsf_overlay_notify_bound(reason: u32) -> i32;
}

unsafe extern "C" fn gl_open(log: Log, user: Handle, out: *mut Handle) -> i32 {
    let setup = rsf_gl_setup {
        struct_size: size_of::<rsf_gl_setup>() as u32,
        abi_version: 1,
        device: ptr::null_mut(),
        queue: ptr::null_mut(),
        log,
        user,
    };
    unsafe { gl::rsf_gl_open(&setup, out.cast()) }
}

unsafe extern "C" fn fg_install(setup: *const rsf_fg12_setup) -> i32 {
    fg12::bind_native_platform();
    unsafe { fg12::rsf_core_fg12_install_explicit(setup) }
}

unsafe extern "C" fn latency_completed(frame: u64) -> i32 {
    let mut timing = rsf_native_latency_frame {
        struct_size: size_of::<rsf_native_latency_frame>() as u32,
        version: 1,
        source: frame,
        generation: 0,
        ready_qpc: 0,
        paced: 0,
        failed: 0,
        completed: 0,
    };
    i32::from(unsafe { rsf_native_latency_frame_get(&mut timing) } != 0 && timing.completed != 0)
}

unsafe extern "C" fn pcl_message() -> u32 {
    // The service status consists exclusively of integers.
    let mut status: rsf_streamline_latency_status = unsafe { std::mem::zeroed() };
    status.struct_size = size_of::<rsf_streamline_latency_status>() as u32;
    let host = present::rsf_core_d3d11p_host();
    if unsafe { rsf_streamline_host_latency_status(host, &mut status) } == OK {
        status.pcl_message_id
    } else {
        0
    }
}

unsafe extern "C" fn gl_graphics(ctx: Handle, device: *mut Handle, queue: *mut Handle) -> i32 {
    unsafe { gl::rsf_gl_graphics(ctx.cast(), device, queue) }
}

unsafe extern "C" fn gl_texture_create(
    ctx: Handle,
    width: u32,
    height: u32,
    format: u32,
    out: *mut Handle,
) -> i32 {
    unsafe { gl::rsf_gl_texture_create(ctx.cast(), width, height, format, out.cast()) }
}

unsafe extern "C" fn gl_texture_get(texture: Handle, out: *mut rsf_gl_texture_info) -> i32 {
    unsafe { gl::rsf_gl_texture_get(texture.cast(), out) }
}

unsafe extern "C" fn gl_texture_destroy(texture: Handle) {
    unsafe { gl::rsf_gl_texture_destroy(texture.cast()) }
}

unsafe extern "C" fn gl_begin(
    ctx: Handle,
    count: u32,
    textures: *const Handle,
    out: *mut Handle,
    slot: *mut u32,
) -> i32 {
    unsafe { gl::rsf_gl_begin(ctx.cast(), count, textures.cast(), out, slot) }
}

unsafe extern "C" fn gl_submit(ctx: Handle, count: u32, textures: *const Handle) -> i32 {
    unsafe { gl::rsf_gl_submit(ctx.cast(), count, textures.cast()) }
}

unsafe extern "C" fn gl_drain(ctx: Handle) -> i32 {
    unsafe { gl::rsf_gl_drain(ctx.cast()) }
}

unsafe extern "C" fn gl_close(ctx: Handle) -> i32 {
    unsafe { gl::rsf_gl_close(ctx.cast()) }
}

unsafe extern "C" fn sr_create(setup: *const rsf_sr12_setup, out: *mut Handle) -> i32 {
    unsafe { sr12::create_for_host(setup, out.cast()) }
}

unsafe extern "C" fn sr_plan(owner: Handle, width: *mut u32, height: *mut u32) -> i32 {
    unsafe { sr12::rsf_core_sr12_plan(owner.cast(), width, height) }
}

unsafe extern "C" fn sr_evaluate(owner: Handle, list: Handle, pass: *const Pass, slot: u32) -> i32 {
    unsafe { sr12::rsf_core_sr12_evaluate(owner.cast(), list, pass, slot) }
}

unsafe extern "C" fn sr_set_sharpening(
    owner: Handle,
    options: *const rsf_sharpening_options,
) -> i32 {
    unsafe { sr12::rsf_core_sr12_set_sharpening(owner.cast(), options) }
}

unsafe extern "C" fn sr_destroy(owner: Handle) {
    unsafe { sr12::rsf_core_sr12_destroy(owner.cast()) }
}

pub(super) static PLATFORM: Platform = Platform {
    gl_open,
    gl_graphics,
    gl_texture_create,
    gl_texture_get,
    gl_texture_destroy,
    gl_begin,
    gl_submit,
    gl_drain,
    gl_close,
    window_valid: window::rsf_core_gl_window_valid,
    window_create: window::rsf_gl_window_create,
    window_destroy: window::rsf_gl_window_destroy,
    window_child_of: window::rsf_core_gl_window_child_of,
    window_resize: window::rsf_core_gl_window_resize,
    window_show: window::rsf_core_gl_window_show,
    chain_create: dxgi::chain_create,
    chain_associate: dxgi::rsf_core_gl_chain_associate,
    chain_resize: dxgi::rsf_core_gl_chain_resize,
    chain_back_buffer: dxgi::rsf_core_gl_chain_back_buffer,
    chain_present: dxgi::rsf_core_gl_chain_present,
    adapter_identity: dxgi::rsf_core_gl_adapter_identity,
    transition: dxgi::rsf_core_gl_transition,
    copy_resource: dxgi::rsf_core_gl_copy_resource,
    sr_create,
    sr_plan,
    sr_evaluate,
    sr_set_sharpening,
    sr_destroy,
    fg_install,
    fg_options: fg12::rsf_core_fg12_options,
    fg_cpu: fg12::rsf_core_fg12_cpu,
    fg_hudless: fg12::rsf_core_fg12_hudless,
    fg_submitted: fg12::rsf_core_fg12_submitted,
    fg_window: fg12::rsf_core_fg12_window,
    fg_status: fg12::rsf_core_fg12_status,
    present_host: present::rsf_core_d3d11p_host,
    present_is_owner: rsf_d3d11_present_is_owner,
    present_backend: present::rsf_core_d3d11p_backend,
    present_backend_choices: present::rsf_core_d3d11p_backend_choices,
    present_request: present::rsf_core_d3d11p_request,
    present_switch_result: present::rsf_core_d3d11p_switch_result,
    present_abort: rsf_d3d11_present_abort,
    pcl_message,
    cpu_read: crate::native_cpu::rsf_native_cpu_read,
    latency_completed,
    latency_end_session: rsf_native_latency_end_session,
    dump_cancel: rsf_input_dump_cancel,
    dlss_preset_valid: rsf_gl_contract_dlss_preset_valid,
    fg_default_backend: rsf_gl_contract_fg_default_backend,
    depth_domains_requested: rsf_gl_contract_depth_domains_requested,
    overlay_configure: rsf_overlay_configure_bound,
    overlay_present: rsf_overlay_present_bound,
    overlay_notify: rsf_overlay_notify_bound,
};
