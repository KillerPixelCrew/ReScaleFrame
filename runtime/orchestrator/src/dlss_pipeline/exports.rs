// SPDX-License-Identifier: MIT
use super::platform::LEAVES;
use super::*;

mod fixture;

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_start_backend(
    device: Raw,
    setup: *const Setup,
    backend: u32,
) -> i32 {
    unsafe { PROCESS.start_backend(&LEAVES, device, setup, backend) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_pipeline_on_frame(context: Raw, frame: *const Frame) -> i32 {
    unsafe { PROCESS.on_frame(&LEAVES, context, frame) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_dlss_pipeline_output_texture() -> Raw {
    PROCESS.output_texture()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_pipeline_request_dump(prefix: *const c_char) -> i32 {
    unsafe { PROCESS.request_dump(prefix) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_get_status(status: *mut Status) -> i32 {
    unsafe { PROCESS.status(status) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_pipeline_prepare_layer(
    width: u32,
    height: u32,
    output: *mut Raw,
) -> i32 {
    unsafe { PROCESS.prepare_layer(&LEAVES, width, height, output) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_pipeline_on_layer(context: Raw, layer: *const Layer) -> i32 {
    unsafe { PROCESS.on_layer(&LEAVES, context, layer) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_dlss_pipeline_layer_output() -> Raw {
    PROCESS.layer_texture()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_dlss_pipeline_set_colour_correction(enabled: u32) {
    PROCESS.set_colour_correction(enabled);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_dlss_pipeline_set_colour_transport(enabled: u32) {
    PROCESS.set_colour_transport(enabled);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_pipeline_set_sharpening(options: *const Sharpening) -> i32 {
    unsafe { PROCESS.set_sharpening(options) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_pipeline_apply_sharpening(
    settings: &crate::render_abi::rsf_render_settings,
) {
    let options = crate::settings_store::sharpening_of(settings, &sharpening_policy::defaults());
    let _ = unsafe { PROCESS.set_sharpening(&raw const options) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_set_preset(preset: u32) -> i32 {
    unsafe { PROCESS.set_preset(&LEAVES, preset) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_set_quality(
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { PROCESS.set_quality(&LEAVES, quality, width, height) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_resize_output(
    output_width: u32,
    output_height: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { PROCESS.resize_output(&LEAVES, output_width, output_height, width, height) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_select_backend(
    backend: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { PROCESS.select_backend(&LEAVES, backend, width, height) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_pipeline_stop() -> i32 {
    unsafe { PROCESS.stop(&LEAVES) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_pipeline_start(device: Raw, setup: *const Setup) -> i32 {
    unsafe { rsf_dlss_pipeline_start_backend(device, setup, SR_DLSS) }
}
