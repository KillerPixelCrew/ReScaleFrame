// SPDX-License-Identifier: MIT
use super::*;
use crate::platform_abi::RSF_MINECRAFT_HUDLESS_SCREEN;

fn surfaces() -> rsf_gl_pipeline_surfaces {
    rsf_gl_pipeline_surfaces {
        struct_size: struct_size::<rsf_gl_pipeline_surfaces>(),
        render_width: 1280,
        render_height: 720,
        output_width: 1920,
        output_height: 1080,
        scene: 11,
        depth: 12,
        motion: 13,
        output: 14,
        hudless: 15,
        composed: 16,
    }
}

fn packet() -> CameraPacket {
    std::array::from_fn(|index| index as f32 + 0.5)
}

fn defaults() -> rsf_sharpening_options {
    rsf_sharpening_options {
        struct_size: struct_size::<rsf_sharpening_options>(),
        abi_version: 1,
        sharpness: 0.4,
        ..Default::default()
    }
}

#[test]
fn camera_packet_fills_the_primary_sr_pass() {
    let data = packet();
    let pass = pass(&data, &surfaces(), 42, true, 3);
    let camera = &pass.camera;
    assert_eq!(pass.struct_size, struct_size::<rsf_game_render_pass>());
    assert_eq!(camera.struct_size, struct_size::<rsf_camera_frame>());
    assert_eq!(camera.abi_version, RSF_GAME_CAMERA_ABI_VERSION);
    assert_eq!(camera.view_to_clip[..], data[0..16]);
    assert_eq!(camera.clip_to_view[..], data[16..32]);
    assert_eq!(camera.view_to_world[..], data[32..48]);
    assert_eq!(camera.world_to_view[..], data[48..64]);
    assert_eq!(camera.clip_to_previous_clip[..], data[64..80]);
    assert_eq!(camera.jitter_pixels, [80.5, 81.5]);
    assert_eq!(camera.previous_jitter_pixels, [82.5, 83.5]);
    assert_eq!(
        [
            camera.near_plane,
            camera.far_plane,
            camera.vertical_fov_radians,
            camera.frame_time_seconds
        ],
        [84.5, 85.5, 86.5, 87.5]
    );
    assert_eq!(
        [
            camera.render_width,
            camera.render_height,
            camera.output_width,
            camera.output_height
        ],
        [1280, 720, 1920, 1080]
    );
    assert_eq!(camera.depth_inverted, 0);
    assert_eq!(pass.role, RSF_GAME_RENDER_SR);
    assert_eq!((pass.native_frame, pass.source_frame_id), (42, 42));
    assert_eq!(
        (pass.view_key, pass.history_key, pass.camera_valid),
        (1, 1, 1)
    );
    assert_eq!(pass.flags, RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_RESET);
    assert_eq!(pass.screen, 3);
    assert_eq!(pass.render_rect, [0, 0, 1280, 720]);
    assert_eq!(pass.output_rect, [0, 0, 1920, 1080]);
    assert_eq!(pass.motion_to_uv, [1.0 / 1280.0, 1.0 / 720.0]);
    assert_eq!(pass.motion_camera_included, 1);
    assert!(pass.depth.is_null() && pass.swapchain.is_null());
}

#[test]
fn hudless_pass_without_reset_keeps_only_primary() {
    let pass = pass(
        &packet(),
        &surfaces(),
        7,
        false,
        RSF_MINECRAFT_HUDLESS_SCREEN,
    );
    assert_eq!(pass.flags, RSF_GAME_RENDER_PRIMARY);
    assert_eq!(pass.screen, 4);
}

#[test]
fn empty_pass_carries_only_its_size() {
    let pass = empty_pass();
    assert_eq!(pass.struct_size, struct_size::<rsf_game_render_pass>());
    assert_eq!(
        (pass.role, pass.camera_valid, pass.source_frame_id),
        (0, 0, 0)
    );
}

#[test]
fn sharpening_choices_refuse_out_of_range_and_non_finite_values() {
    let mut values = [0.25f32; 12];
    values[0] = 4.0;
    values[2] = 1.9;
    values[6] = -0.5;
    values[10] = f32::NAN;
    values[11] = 4.5;
    let decoded = sharpening(&values, defaults());
    assert_eq!(decoded.struct_size, struct_size::<rsf_sharpening_options>());
    assert_eq!(decoded.abi_version, 1);
    assert_eq!(
        [
            decoded.mode,
            decoded.mas,
            decoded.contrast_enabled,
            decoded.clamp_output,
            decoded.debug
        ],
        [4, 1, u32::MAX, u32::MAX, u32::MAX]
    );
    assert_eq!(decoded.sharpness, 0.25);
    assert_eq!(decoded.depth_bias, 0.25);
}

#[test]
fn options_map_generation_limit_and_debug() {
    let mut configuration = rsf_minecraft_configuration {
        width: 1920,
        height: 1080,
        sr: 1,
        quality: 2,
        preset: 13,
        fg: 3,
        count: 3,
        reflex: 2,
        limit: 60,
        debug: 1,
        sharpening: [0.0; 12],
    };
    let options = options(&configuration, defaults());
    assert_eq!(
        options.struct_size,
        struct_size::<rsf_gl_pipeline_options>()
    );
    assert_eq!(
        (options.sr_backend, options.quality, options.fg_backend),
        (1, 2, 3)
    );
    assert_eq!(options.dlss_preset, 13);
    let generation = options.generation;
    assert_eq!(
        generation.struct_size,
        struct_size::<rsf_native_fg_options>()
    );
    assert_eq!(generation.mode, RSF_FG_FIXED);
    assert_eq!(
        (generation.generated_frames, generation.reflex_mode),
        (3, 2)
    );
    assert_eq!(generation.frame_limit_us, 16_666);
    assert_eq!(generation.debug, 1);

    configuration.fg = 0;
    configuration.limit = 0;
    configuration.debug = 0;
    let generation = super::options(&configuration, defaults()).generation;
    assert_eq!(generation.mode, RSF_FG_OFF);
    assert_eq!((generation.frame_limit_us, generation.debug), (0, 0));
    configuration.limit = -5;
    assert_eq!(
        super::options(&configuration, defaults())
            .generation
            .frame_limit_us,
        0
    );
}

#[test]
fn surface_values_follow_the_java_layout() {
    assert_eq!(
        surface_values(&surfaces(), 1920, 1080),
        [1280, 720, 1920, 1080, 11, 12, 13, 14, 15, 16]
    );
}

#[test]
fn status_values_keep_the_legacy_prefix_and_append_capacity_and_latency() {
    let mut status = rsf_gl_pipeline_status {
        requested_sr: 7,
        effective_sr: 1,
        requested_fg: 7,
        effective_fg: 3,
        sr_result: -8,
        fg_switch_result: -1,
        evaluated: 900,
        ..Default::default()
    };
    status.generation.reason = 5;
    status.generation.last_result = -2;
    status.generation.tagged_frames = 800;
    status.generation.vendor.effective_generated_frames = 2;
    status.generation.vendor.generated_presented = 700;
    status.generation.vendor.active = 1;
    status.generation.vendor.valid_statistics = 1;
    status.generation.vendor.max_generated_frames = 3;
    status.generation.vendor.supported = 1;
    let mut latency = rsf_native_latency_status {
        owner: 1,
        paced_frames: 600,
        derived_boundaries: 9,
        ..Default::default()
    };
    latency.events.input_events = [10, 11, 12];
    latency.events.pings = 13;
    latency.events.substitutes = 14;
    latency.events.order_violations = 15;
    let values = status_values(&status, &latency);
    assert_eq!(
        values,
        [
            7, 1, 7, 3, -8, -1, 900, 5, -2, 2, 800, 700, 1, 1, 3, 1, 1, 600, 10, 11, 12, 13, 14,
            15, 9
        ]
    );
}
