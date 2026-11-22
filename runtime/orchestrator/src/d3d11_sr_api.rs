// SPDX-License-Identifier: MIT
use crate::dlss_pipeline::exports::{
    rsf_dlss_pipeline_get_status, rsf_dlss_pipeline_output_texture, rsf_dlss_pipeline_request_dump,
    rsf_dlss_pipeline_resize_output, rsf_dlss_pipeline_select_backend,
    rsf_dlss_pipeline_set_preset, rsf_dlss_pipeline_set_quality, rsf_dlss_pipeline_set_sharpening,
    rsf_dlss_pipeline_start, rsf_dlss_pipeline_stop,
};
use crate::native_sr::rsf_native_sr_evaluate;
use crate::sr_history::{rsf_core_sr_clear_reset, rsf_core_sr_evaluate, rsf_core_sr_request_reset};
use crate::sr_session::has_capacity;
use rsf_game_sys::rsf_game_render_pass as Pass;
use rsf_game_sys::{
    RSF_D3D11_SR_API_VERSION, RSF_DLSS_PIPELINE_ABI_VERSION, rsf_d3d11_sr_api as Api,
};
use std::ffi::c_void;

unsafe extern "C" fn evaluate(context: *mut c_void, pass: *const Pass) -> i32 {
    unsafe { rsf_core_sr_evaluate(context, pass, rsf_native_sr_evaluate) }
}

unsafe extern "C" fn stop() -> i32 {
    rsf_core_sr_clear_reset();
    unsafe { rsf_dlss_pipeline_stop() }
}

unsafe extern "C" fn set_quality(quality: u32, width: *mut u32, height: *mut u32) -> i32 {
    if quality > 4 {
        // RSF_DLSS_QUALITY_ULTRA_PERFORMANCE
        return -1;
    }
    unsafe { rsf_dlss_pipeline_set_quality(quality, width, height) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_get_d3d11_sr_api(api: *mut Api) -> i32 {
    if !unsafe { has_capacity(api) } {
        return -1;
    }
    if unsafe { (&raw const (*api).abi_version).read() } != RSF_D3D11_SR_API_VERSION {
        return -2;
    }
    unsafe {
        api.write(Api {
            struct_size: size_of::<Api>() as u32,
            abi_version: RSF_D3D11_SR_API_VERSION,
            pipeline_abi_version: RSF_DLSS_PIPELINE_ABI_VERSION,
            start: Some(rsf_dlss_pipeline_start),
            evaluate: Some(evaluate),
            reset: Some(rsf_core_sr_request_reset),
            stop: Some(stop),
            status: Some(rsf_dlss_pipeline_get_status),
            select_backend: Some(rsf_dlss_pipeline_select_backend),
            set_quality: Some(set_quality),
            set_preset: Some(rsf_dlss_pipeline_set_preset),
            resize_output: Some(rsf_dlss_pipeline_resize_output),
            output_texture: Some(rsf_dlss_pipeline_output_texture),
            request_dump: Some(rsf_dlss_pipeline_request_dump),
            set_sharpening: Some(rsf_dlss_pipeline_set_sharpening),
        });
    }
    0
}
