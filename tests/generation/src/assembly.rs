// SPDX-License-Identifier: MIT
use crate::abi::*;
use std::ffi::c_void;
use std::ptr::{null_mut, without_provenance_mut};

unsafe extern "C" {
    fn rsf_assemble_dlss_frame(
        camera: *const rsf_pipeline_camera_frame,
        resources: *const rsf_frame_resources,
        frame: *mut rsf_dlss_frame,
    ) -> i32;
}

fn camera() -> rsf_pipeline_camera_frame {
    let mut identity = [0.0; 16];
    for i in (0..16).step_by(5) {
        identity[i] = 1.0;
    }
    let with_marker = |value| {
        let mut matrix = identity;
        matrix[3] = value;
        matrix
    };
    rsf_pipeline_camera_frame {
        struct_size: size_of::<rsf_pipeline_camera_frame>() as u32,
        abi_version: RSF_FRAME_ASSEMBLY_ABI_VERSION,
        view_to_clip: with_marker(1.5),
        clip_to_view: with_marker(2.5),
        clip_to_prev_clip: with_marker(3.5),
        prev_clip_to_clip: with_marker(4.5),
        camera_position: [260_968.45, 0.0, 0.0],
        camera_forward: [0.0, 0.0, 1.0],
        camera_up: [0.0, 1.0, 0.0],
        camera_right: [1.0, 0.0, 0.0],
        near_plane: 1.0,
        far_plane: 0.0,
        vertical_fov: 0.6632,
        aspect_ratio: 16.0 / 9.0,
        jitter_pixels: [-0.430, 0.157],
        has_jitter: 1,
        depth_inverted: 1,
        camera_motion_included: 0,
        motion_decoded: 1,
        has_motion_sentinel: 1,
        motion_sentinel: -1000.0,
        motion_scale: [1.0; 2],
        reset: 0,
    }
}

fn resources() -> rsf_frame_resources {
    // Assembly only copies these borrowed pointers.
    let texture = |address| without_provenance_mut::<c_void>(address);
    rsf_frame_resources {
        struct_size: size_of::<rsf_frame_resources>() as u32,
        color_in: texture(0x1000),
        color_out: texture(0x2000),
        depth: texture(0x3000),
        motion: texture(0x4000),
        exposure: texture(0x5000),
        render_width: 1024,
        render_height: 576,
        output_width: 2048,
        output_height: 1152,
        quality: RSF_TEST_DLSS_PERFORMANCE as u32,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_frame_assembly() -> i32 {
    let camera = camera();
    let resources = resources();
    let size = size_of::<rsf_dlss_frame>() as u32;
    // The generated C record uses integer, float and pointer fields.
    let mut frame: rsf_dlss_frame = unsafe { std::mem::zeroed() };
    let mut passed = true;
    let mut check = |condition: bool, message: &str| {
        if !condition {
            passed = false;
            eprintln!("FAIL: {message}");
        }
    };
    let assemble = |camera: &rsf_pipeline_camera_frame,
                    resources: &rsf_frame_resources,
                    frame: &mut rsf_dlss_frame| unsafe {
        rsf_assemble_dlss_frame(camera, resources, frame)
    };
    for short_size in [0, size - 1] {
        frame.struct_size = short_size;
        frame.abi_version = u32::MAX;
        check(
            assemble(&camera, &resources, &mut frame) == -1
                && frame.struct_size == short_size
                && frame.abi_version == u32::MAX,
            "short output is refused without writes",
        );
    }
    frame.struct_size = size;
    check(
        assemble(&camera, &resources, &mut frame) == 0,
        "valid inputs assemble",
    );
    check(
        frame.abi_version == RSF_DLSS_ABI_VERSION && frame.struct_size == size,
        "the assembled ABI and size match the backend",
    );
    check(
        frame.color_in == resources.color_in
            && frame.color_out == resources.color_out
            && frame.depth == resources.depth
            && frame.motion == resources.motion
            && frame.exposure == resources.exposure,
        "resource slots preserve identity",
    );
    check(
        frame.render_width == 1024
            && frame.output_width == 2048
            && frame.quality == RSF_TEST_DLSS_PERFORMANCE as u32,
        "size and quality pass through",
    );
    check(
        frame.camera_view_to_clip[3] == 1.5
            && frame.clip_to_camera_view[3] == 2.5
            && frame.clip_to_prev_clip[3] == 3.5
            && frame.prev_clip_to_clip[3] == 4.5,
        "projection and reprojection matrices keep their roles",
    );
    check(
        frame.jitter_x == -0.430 && frame.jitter_y == 0.157,
        "both jitter axes pass through",
    );
    check(
        frame.depth_inverted == 1
            && frame.camera_motion_included == 0
            && frame.motion_invalid_value == -1000.0,
        "depth and motion conventions pass through",
    );
    check(
        frame.near_plane == 1.0 && frame.far_plane > 1.0e6,
        "infinite far plane becomes finite",
    );

    for (broken, result, message) in [
        (
            rsf_pipeline_camera_frame {
                has_jitter: 0,
                ..camera
            },
            -3,
            "missing jitter refused",
        ),
        (
            rsf_pipeline_camera_frame {
                motion_decoded: 0,
                ..camera
            },
            -4,
            "encoded motion refused",
        ),
        (
            rsf_pipeline_camera_frame {
                abi_version: RSF_FRAME_ASSEMBLY_ABI_VERSION + 1,
                ..camera
            },
            -2,
            "incompatible camera ABI refused",
        ),
        (
            rsf_pipeline_camera_frame {
                struct_size: 0,
                ..camera
            },
            -1,
            "short camera refused",
        ),
    ] {
        check(assemble(&broken, &resources, &mut frame) == result, message);
    }
    let smaller = rsf_frame_resources {
        output_width: 512,
        output_height: 288,
        ..resources
    };
    check(
        assemble(&camera, &smaller, &mut frame) == -3,
        "smaller output refused",
    );
    let missing = rsf_frame_resources {
        depth: null_mut(),
        ..resources
    };
    check(
        assemble(&camera, &missing, &mut frame) == -1,
        "missing depth refused",
    );
    let no_exposure = rsf_frame_resources {
        exposure: null_mut(),
        ..resources
    };
    check(
        assemble(&camera, &no_exposure, &mut frame) == 0 && frame.exposure.is_null(),
        "optional exposure remains absent",
    );
    let native = rsf_frame_resources {
        render_width: resources.output_width,
        render_height: resources.output_height,
        quality: RSF_TEST_DLSS_NATIVE as u32,
        ..resources
    };
    check(
        assemble(&camera, &native, &mut frame) == 0,
        "native resolution is accepted",
    );
    i32::from(!passed)
}
