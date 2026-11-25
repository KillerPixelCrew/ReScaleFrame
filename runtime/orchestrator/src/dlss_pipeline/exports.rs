// SPDX-License-Identifier: MIT
use super::*;

#[cfg(feature = "test-fixtures")]
#[path = "../../../../tests/sr_backend_switch_device.rs"]
mod backend_switch_fixture;
#[cfg(feature = "test-fixtures")]
mod fixture;

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_start_backend(
    device: Raw,
    setup: *const Setup,
    backend: u32,
) -> i32 {
    unsafe { PROCESS.start_backend(device, setup, backend) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_on_frame(
    context: Raw,
    frame: *const Frame,
) -> i32 {
    unsafe { PROCESS.on_frame(context, frame) }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_dlss_pipeline_output_texture() -> Raw {
    PROCESS.output_texture()
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_request_dump(prefix: *const c_char) -> i32 {
    unsafe { PROCESS.request_dump(prefix) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_get_status(status: *mut Status) -> i32 {
    unsafe { PROCESS.status(status) }
}

/// SR backends this machine can serve as a choices mask (bit n for backend id n), Auto included.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_dlss_pipeline_backend_choices() -> u32 {
    PROCESS.backend_choices()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_pipeline_prepare_layer(
    width: u32,
    height: u32,
    output: *mut Raw,
) -> i32 {
    unsafe { PROCESS.prepare_layer(width, height, output) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_pipeline_on_layer(context: Raw, layer: *const Layer) -> i32 {
    unsafe { PROCESS.on_layer(context, layer) }
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
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_set_sharpening(
    options: *const Sharpening,
) -> i32 {
    unsafe { PROCESS.set_sharpening(options) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_set_preset(preset: u32) -> i32 {
    unsafe { PROCESS.set_preset(preset) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_set_quality(
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { PROCESS.set_quality(quality, width, height) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_resize_output(
    output_width: u32,
    output_height: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { PROCESS.resize_output(output_width, output_height, width, height) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_select_backend(
    backend: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { PROCESS.select_backend(backend, width, height) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_stop() -> i32 {
    unsafe { PROCESS.stop() }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_pipeline_start(device: Raw, setup: *const Setup) -> i32 {
    unsafe { rsf_dlss_pipeline_start_backend(device, setup, RSF_SR_DLSS) }
}
