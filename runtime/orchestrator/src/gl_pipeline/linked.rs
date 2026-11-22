// SPDX-License-Identifier: MIT
//! Bind the production OpenGL pipeline to its sibling runtime owners.
use super::*;
use crate::sys::rsf_native_latency_frame;
use crate::sys::rsf_streamline_latency_status;
use crate::{d3d11_present as present, fg12, gl_shared_surface as gl, sr12};

// The leaves gl_pipeline.rs calls as `native::*`; gl_pipeline/tests.rs fakes the same names.
pub(super) use super::dxgi::{
    chain_create, rsf_core_gl_adapter_identity as adapter_identity,
    rsf_core_gl_chain_associate as chain_associate,
    rsf_core_gl_chain_back_buffer as chain_back_buffer, rsf_core_gl_chain_present as chain_present,
    rsf_core_gl_chain_resize as chain_resize, rsf_core_gl_copy_resource as copy_resource,
    rsf_core_gl_transition as transition,
};
pub(super) use super::window::{
    rsf_core_gl_window_child_of as window_child_of, rsf_core_gl_window_resize as window_resize,
    rsf_core_gl_window_show as window_show, rsf_core_gl_window_valid as window_valid,
    rsf_gl_window_create as window_create, rsf_gl_window_destroy as window_destroy,
};
pub(super) use crate::d3d11_present::{
    rsf_d3d11_present_backend as present_backend,
    rsf_d3d11_present_backend_choices as present_backend_choices,
    rsf_d3d11_present_abort as present_abort, rsf_d3d11_present_host as present_host,
    rsf_d3d11_present_is_owner as present_is_owner, rsf_d3d11_present_request as present_request,
    rsf_d3d11_present_switch_result as present_switch_result,
};
pub(super) use crate::fg_choice::rsf_fg_default_backend as fg_default_backend;
pub(super) use crate::fg12::{
    rsf_core_fg12_cpu as fg_cpu, rsf_core_fg12_hudless as fg_hudless,
    rsf_core_fg12_options as fg_options, rsf_core_fg12_status as fg_status,
    rsf_core_fg12_submitted as fg_submitted, rsf_core_fg12_window as fg_window,
};
pub(super) use crate::native_cpu::rsf_native_cpu_read as cpu_read;
pub(super) use crate::overlay_service::{
    rsf_overlay_configure_bound as overlay_configure, rsf_overlay_notify_bound as overlay_notify,
    rsf_overlay_present_bound as overlay_present,
};

pub(super) use crate::input_dump::abi::rsf_input_dump_cancel as dump_cancel;
pub(super) use crate::native_fg_cpu::platform::rsf_native_latency_end_session as latency_end_session;
use crate::native_fg_cpu::platform::rsf_native_latency_frame_get;
use crate::streamline_host::rsf_streamline_host_latency_status;

pub(super) unsafe extern "C" fn dlss_preset_valid(preset: u32) -> c_int {
    c_int::from(crate::streamline_sr::preset_valid(preset))
}

pub(super) unsafe extern "C" fn gl_open(log: Log, user: Handle, out: *mut Handle) -> i32 {
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

pub(super) unsafe extern "C" fn fg_install(setup: *const rsf_fg12_setup) -> i32 {
    unsafe { fg12::rsf_core_fg12_install_explicit(setup) }
}

pub(super) unsafe extern "C" fn latency_completed(frame: u64) -> i32 {
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

pub(super) unsafe extern "C" fn pcl_message() -> u32 {
    // The service status consists exclusively of integers.
    let mut status: rsf_streamline_latency_status = unsafe { std::mem::zeroed() };
    status.struct_size = size_of::<rsf_streamline_latency_status>() as u32;
    let host = present::rsf_d3d11_present_host();
    if unsafe { rsf_streamline_host_latency_status(host.cast(), &mut status) } == OK {
        status.pcl_message_id
    } else {
        0
    }
}

pub(super) unsafe extern "C" fn gl_graphics(
    ctx: Handle,
    device: *mut Handle,
    queue: *mut Handle,
) -> i32 {
    unsafe { gl::rsf_gl_graphics(ctx.cast(), device, queue) }
}

pub(super) unsafe extern "C" fn gl_texture_create(
    ctx: Handle,
    width: u32,
    height: u32,
    format: u32,
    out: *mut Handle,
) -> i32 {
    unsafe { gl::rsf_gl_texture_create(ctx.cast(), width, height, format, out.cast()) }
}

pub(super) unsafe extern "C" fn gl_texture_get(
    texture: Handle,
    out: *mut rsf_gl_texture_info,
) -> i32 {
    unsafe { gl::rsf_gl_texture_get(texture.cast(), out) }
}

pub(super) unsafe extern "C" fn gl_texture_destroy(texture: Handle) {
    unsafe { gl::rsf_gl_texture_destroy(texture.cast()) }
}

pub(super) unsafe extern "C" fn gl_begin(
    ctx: Handle,
    count: u32,
    textures: *const Handle,
    out: *mut Handle,
    slot: *mut u32,
) -> i32 {
    unsafe { gl::rsf_gl_begin(ctx.cast(), count, textures.cast(), out, slot) }
}

pub(super) unsafe extern "C" fn gl_submit(ctx: Handle, count: u32, textures: *const Handle) -> i32 {
    unsafe { gl::rsf_gl_submit(ctx.cast(), count, textures.cast()) }
}

pub(super) unsafe extern "C" fn gl_drain(ctx: Handle) -> i32 {
    unsafe { gl::rsf_gl_drain(ctx.cast()) }
}

pub(super) unsafe extern "C" fn gl_close(ctx: Handle) -> i32 {
    unsafe { gl::rsf_gl_close(ctx.cast()) }
}

pub(super) unsafe extern "C" fn sr_create(setup: *const rsf_sr12_setup, out: *mut Handle) -> i32 {
    unsafe { sr12::rsf_core_sr12_create(setup, out.cast()) }
}

pub(super) unsafe extern "C" fn sr_plan(owner: Handle, width: *mut u32, height: *mut u32) -> i32 {
    unsafe { sr12::rsf_core_sr12_plan(owner.cast(), width, height) }
}

pub(super) unsafe extern "C" fn sr_evaluate(
    owner: Handle,
    list: Handle,
    pass: *const Pass,
    slot: u32,
) -> i32 {
    unsafe { sr12::rsf_core_sr12_evaluate(owner.cast(), list, pass, slot) }
}

pub(super) unsafe extern "C" fn sr_set_sharpening(
    owner: Handle,
    options: *const rsf_sharpening_options,
) -> i32 {
    unsafe { sr12::rsf_core_sr12_set_sharpening(owner.cast(), options) }
}

pub(super) unsafe extern "C" fn sr_destroy(owner: Handle) {
    unsafe { sr12::rsf_core_sr12_destroy(owner.cast()) }
}

pub(super) unsafe extern "C" fn depth_domains_requested(pass: *const Pass) -> u32 {
    u32::from(rsf_game_sys::depth_domains_requested(unsafe {
        &(*pass).depth_domains
    }))
}
