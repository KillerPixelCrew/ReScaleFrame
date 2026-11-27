// SPDX-License-Identifier: MIT
use crate::fg_scene::rsf_native_fg_bind_source;
use crate::native_cpu::rsf_native_cpu_dispatch;
use crate::native_fg::{
    rsf_native_fg_capture_scoped, rsf_native_fg_dump_inputs, rsf_native_fg_final,
    rsf_native_fg_inputs_ready, rsf_native_fg_options_set, rsf_native_fg_present,
    rsf_native_fg_retire, rsf_native_fg_set_log, rsf_native_fg_status_get,
    rsf_native_fg_window_end,
};
use crate::native_fg_cpu::platform::{
    rsf_native_fg_cpu, rsf_native_fg_frame, rsf_native_fg_reflex_ordering_set,
    rsf_native_fg_submission,
};
use crate::native_scene::{rsf_native_scene_pass, rsf_native_scene_texture_binding};
use crate::native_window::{rsf_native_window_present, rsf_native_window_scope};
use crate::sr_session::has_capacity;
use crate::sys::rsf_native_fg_prepare;
use crate::sys::rsf_observer_present_event as Present;
use crate::sys::{
    RSF_D3D11_FG_API_VERSION, RSF_D3D11_FG_INPUTS_VERSION, rsf_d3d11_fg_api, rsf_d3d11_fg_inputs,
};
use crate::sys::{RSF_DLSS_ABI_VERSION, rsf_dlss_frame as Frame};
use rsf_game_sys::backend_result::{ABI_MISMATCH, INVALID_ARGUMENT};
use rsf_game_sys::rsf_game_cpu_event;
use rsf_game_sys::{
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_RESET,
    RSF_GAME_RENDER_SUBMISSION, RSF_GAME_RENDER_TEXTURE_BINDING, RSF_GAME_RENDER_WINDOW,
    rsf_game_render_pass as Pass,
};
use std::ffi::c_void;

unsafe extern "C" fn cpu_sink(_: *mut c_void, event: *const rsf_game_cpu_event) {
    unsafe { rsf_native_fg_cpu(event) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_dispatch_cpu(event: *const rsf_game_cpu_event) -> i32 {
    unsafe { rsf_native_cpu_dispatch(event, Some(cpu_sink), std::ptr::null_mut()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_dispatch_pass(
    context: *mut c_void,
    pass: *const Pass,
    begin: u32,
) -> i32 {
    if !unsafe { has_capacity(pass) } {
        return 0;
    }
    // These inputs are borrowed for the duration of the native call.
    unsafe {
        match (*pass).role {
            RSF_GAME_RENDER_FRAME => rsf_native_fg_frame(pass, begin),
            RSF_GAME_RENDER_SUBMISSION => rsf_native_fg_submission(pass, begin),
            RSF_GAME_RENDER_FINAL_SCENE => {
                let accepted = rsf_native_scene_pass(pass, begin);
                if begin == 0 && !context.is_null() {
                    rsf_native_fg_final(context, pass);
                }
                return accepted;
            }
            RSF_GAME_RENDER_TEXTURE_BINDING if begin == 0 => {
                return rsf_native_scene_texture_binding(pass);
            }
            RSF_GAME_RENDER_WINDOW => {
                if begin == 0 {
                    rsf_native_fg_window_end(pass);
                }
                return rsf_native_window_scope(pass, begin);
            }
            _ => {}
        }
    }
    1
}

unsafe extern "C" fn capture(
    context: *mut c_void,
    pass: *const Pass,
    inputs: *const rsf_d3d11_fg_inputs,
) {
    if context.is_null() || !unsafe { has_capacity(pass) && has_capacity(inputs) } {
        return;
    }
    let (pass, inputs) = unsafe { (&*pass, &*inputs) };
    if inputs.abi_version != RSF_D3D11_FG_INPUTS_VERSION
        || inputs.depth.is_null()
        || inputs.motion.is_null()
        || pass.camera_valid == 0
    {
        return;
    }
    let extent = |rect: [i32; 4]| {
        let width = u32::try_from(i64::from(rect[2]) - i64::from(rect[0])).ok()?;
        let height = u32::try_from(i64::from(rect[3]) - i64::from(rect[1])).ok()?;
        (width != 0 && height != 0).then_some((width, height))
    };
    let Some((render_width, render_height)) = extent(pass.render_rect) else {
        return;
    };
    let Some((output_width, output_height)) = extent(pass.output_rect) else {
        return;
    };
    let frame = Frame {
        struct_size: size_of::<Frame>() as u32,
        abi_version: RSF_DLSS_ABI_VERSION,
        depth: inputs.depth,
        motion: inputs.motion,
        render_width,
        render_height,
        output_width,
        output_height,
        camera_motion_included: 1,
        reset: u32::from(pass.flags & RSF_GAME_RENDER_RESET != 0),
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { rsf_native_fg_capture_scoped(context, pass, &frame) };
}

unsafe extern "C" fn present_event(_: *mut c_void, event: *const Present) {
    unsafe { rsf_native_window_present(event) };
}

unsafe extern "C" fn latency_event(_: *mut c_void, event: *const Present) {
    unsafe { rsf_native_fg_present(event) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_get_d3d11_fg_api(api: *mut rsf_d3d11_fg_api) -> i32 {
    if !unsafe { has_capacity(api) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*api).abi_version } != RSF_D3D11_FG_API_VERSION {
        return ABI_MISMATCH;
    }
    unsafe {
        api.write(rsf_d3d11_fg_api {
            struct_size: size_of::<rsf_d3d11_fg_api>() as u32,
            abi_version: RSF_D3D11_FG_API_VERSION,
            set_log: Some(rsf_native_fg_set_log),
            options: Some(rsf_native_fg_options_set),
            status: Some(rsf_native_fg_status_get),
            reflex_ordering: Some(rsf_native_fg_reflex_ordering_set),
            cpu: Some(rsf_native_fg_dispatch_cpu),
            pass: Some(rsf_native_fg_dispatch_pass),
            bind_source: Some(rsf_native_fg_bind_source),
            capture: Some(capture),
            inputs_ready: Some(rsf_native_fg_inputs_ready),
            dump_inputs: Some(rsf_native_fg_dump_inputs),
            prepare: Some(rsf_native_fg_prepare),
            retire: Some(rsf_native_fg_retire),
            present_event: Some(present_event),
            latency_event: Some(latency_event),
        })
    };
    0
}
