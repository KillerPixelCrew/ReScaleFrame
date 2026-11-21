// SPDX-License-Identifier: MIT
//! NativeBridge value translation: the Java camera packet to an SR render pass, the Sodium
//! settings to Runtime options, and the Runtime state to the status array the Java renderer reads.
//! No calls; lib.rs supplies the Runtime values.

use crate::platform_abi::{
    RSF_FG_FIXED, RSF_FG_OFF, RSF_MINECRAFT_CAMERA_FLOATS, RSF_MINECRAFT_STATUS_FIELDS,
    RSF_MINECRAFT_SURFACE_FIELDS, rsf_gl_pipeline_options, rsf_gl_pipeline_status,
    rsf_gl_pipeline_surfaces, rsf_minecraft_configuration, rsf_native_fg_options,
    rsf_native_latency_status, rsf_sharpening_options,
};
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RESET,
    RSF_GAME_RENDER_SR, rsf_camera_frame, rsf_game_render_pass,
};

pub(crate) const CAMERA_FLOATS: usize = RSF_MINECRAFT_CAMERA_FLOATS as usize;
pub(crate) const SURFACE_FIELDS: usize = RSF_MINECRAFT_SURFACE_FIELDS as usize;
pub(crate) const STATUS_FIELDS: usize = RSF_MINECRAFT_STATUS_FIELDS as usize;

/// TemporalRenderer's camera packet: five 4x4 matrices (view-to-clip, clip-to-view,
/// view-to-world, world-to-view, clip-to-previous-clip), current and previous jitter in pixels,
/// near, far, vertical FOV in radians and frame time in seconds.
pub(crate) type CameraPacket = [f32; CAMERA_FLOATS];

const fn struct_size<T>() -> u32 {
    size_of::<T>() as u32
}

/// A pass with only its size. The Runtime refuses it; a missing or short camera buffer
/// submitted this before the port as well.
pub(crate) fn empty_pass() -> rsf_game_render_pass {
    // SAFETY: rsf_game_render_pass holds only integers, floats, arrays of them and raw pointers,
    // for which all-zero is valid; C++ value-initialised it the same way.
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.struct_size = struct_size::<rsf_game_render_pass>();
    pass
}

fn matrix(packet: &CameraPacket, index: usize) -> [f32; 16] {
    let mut value = [0.0; 16];
    value.copy_from_slice(&packet[index * 16..index * 16 + 16]);
    value
}

/// The primary SR pass for a camera packet and the pipeline's current surfaces. Frame identity is
/// the Java CPU frame; view and history keys are the single Minecraft world view.
pub(crate) fn pass(
    packet: &CameraPacket,
    surfaces: &rsf_gl_pipeline_surfaces,
    frame: u64,
    reset: bool,
    screen: u32,
) -> rsf_game_render_pass {
    let mut pass = empty_pass();
    let camera: &mut rsf_camera_frame = &mut pass.camera;
    camera.struct_size = struct_size::<rsf_camera_frame>();
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.view_to_clip = matrix(packet, 0);
    camera.clip_to_view = matrix(packet, 1);
    camera.view_to_world = matrix(packet, 2);
    camera.world_to_view = matrix(packet, 3);
    camera.clip_to_previous_clip = matrix(packet, 4);
    camera.jitter_pixels = [packet[80], packet[81]];
    camera.previous_jitter_pixels = [packet[82], packet[83]];
    camera.near_plane = packet[84];
    camera.far_plane = packet[85];
    camera.vertical_fov_radians = packet[86];
    camera.frame_time_seconds = packet[87];
    camera.render_width = surfaces.render_width;
    camera.render_height = surfaces.render_height;
    camera.output_width = surfaces.output_width;
    camera.output_height = surfaces.output_height;
    pass.role = RSF_GAME_RENDER_SR;
    pass.native_frame = frame;
    pass.source_frame_id = frame;
    pass.camera_valid = 1;
    pass.view_key = 1;
    pass.history_key = 1;
    pass.flags = RSF_GAME_RENDER_PRIMARY | if reset { RSF_GAME_RENDER_RESET } else { 0 };
    pass.screen = screen;
    pass.render_rect[2] = surfaces.render_width as i32;
    pass.render_rect[3] = surfaces.render_height as i32;
    pass.output_rect[2] = surfaces.output_width as i32;
    pass.output_rect[3] = surfaces.output_height as i32;
    pass.motion_to_uv = [
        1.0 / surfaces.render_width as f32,
        1.0 / surfaces.render_height as f32,
    ];
    pass.motion_camera_included = 1;
    pass
}

/// A TaaSettings enumerated sharpening field: its ordinal when it is a finite 0..=4, otherwise
/// a value the Runtime's validity check refuses.
fn choice(value: f32) -> u32 {
    if value.is_finite() && (0.0..=4.0).contains(&value) {
        value as u32
    } else {
        u32::MAX
    }
}

/// TaaSettings.sharpening() in its Java order. Size and ABI version come from the Runtime defaults.
pub(crate) fn sharpening(
    values: &[f32; 12],
    defaults: rsf_sharpening_options,
) -> rsf_sharpening_options {
    rsf_sharpening_options {
        mode: choice(values[0]),
        sharpness: values[1],
        mas: choice(values[2]),
        motion_sharpness: values[3],
        motion_threshold: values[4],
        motion_scale_limit: values[5],
        contrast_enabled: choice(values[6]),
        contrast: values[7],
        depth_scale: values[8],
        depth_bias: values[9],
        clamp_output: choice(values[10]),
        debug: choice(values[11]),
        ..defaults
    }
}

/// The Runtime options for one NativeBridge.configure call. A positive frame limit is a rendered
/// frame rate; the Runtime receives its minimum interval in microseconds.
pub(crate) fn options(
    configuration: &rsf_minecraft_configuration,
    sharpening_defaults: rsf_sharpening_options,
) -> rsf_gl_pipeline_options {
    let limit = configuration.limit;
    rsf_gl_pipeline_options {
        struct_size: struct_size::<rsf_gl_pipeline_options>(),
        sr_backend: configuration.sr as u32,
        quality: configuration.quality as u32,
        fg_backend: configuration.fg as u32,
        generation: rsf_native_fg_options {
            struct_size: struct_size::<rsf_native_fg_options>(),
            mode: if configuration.fg != 0 {
                RSF_FG_FIXED
            } else {
                RSF_FG_OFF
            },
            generated_frames: configuration.count as u32,
            reflex_mode: configuration.reflex as u32,
            frame_limit_us: if limit > 0 {
                1_000_000 / limit as u32
            } else {
                0
            },
            debug: u32::from(configuration.debug != 0),
        },
        dlss_preset: configuration.preset as u32,
        sharpening: sharpening(&configuration.sharpening, sharpening_defaults),
    }
}

/// NativeRenderer's surfaces array: world size, display size, then the shared GL textures.
pub(crate) fn surface_values(
    surfaces: &rsf_gl_pipeline_surfaces,
    width: i32,
    height: i32,
) -> [i32; SURFACE_FIELDS] {
    [
        surfaces.render_width as i32,
        surfaces.render_height as i32,
        width,
        height,
        surfaces.scene as i32,
        surfaces.depth as i32,
        surfaces.motion as i32,
        surfaces.output as i32,
        surfaces.hudless as i32,
        surfaces.composed as i32,
    ]
}

/// NativeRenderer's nativeStatus array. The first 14 values keep their original positions;
/// FG capacity (14, 15) and latency facts (16..25) were appended.
pub(crate) fn status_values(
    status: &rsf_gl_pipeline_status,
    latency: &rsf_native_latency_status,
) -> [i64; STATUS_FIELDS] {
    let fg = &status.generation;
    let vendor = &fg.vendor;
    let events = &latency.events;
    [
        i64::from(status.requested_sr),
        i64::from(status.effective_sr),
        i64::from(status.requested_fg),
        i64::from(status.effective_fg),
        i64::from(status.sr_result),
        i64::from(status.fg_switch_result),
        status.evaluated as i64,
        i64::from(fg.reason),
        i64::from(fg.last_result),
        i64::from(vendor.effective_generated_frames),
        fg.tagged_frames as i64,
        vendor.generated_presented as i64,
        i64::from(vendor.active),
        i64::from(vendor.valid_statistics),
        i64::from(vendor.max_generated_frames),
        i64::from(vendor.supported),
        i64::from(latency.owner),
        latency.paced_frames as i64,
        events.input_events[0] as i64,
        events.input_events[1] as i64,
        events.input_events[2] as i64,
        events.pings as i64,
        events.substitutes as i64,
        events.order_violations as i64,
        latency.derived_boundaries as i64,
    ]
}

#[cfg(test)]
mod tests;
