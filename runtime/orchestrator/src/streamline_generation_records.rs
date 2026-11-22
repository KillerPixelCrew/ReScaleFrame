// SPDX-License-Identifier: MIT
//! D3D12 FG payloads using the same checked SDK value layouts as Vulkan.
use crate::render_abi::{RSF_REFLEX_BOOST, rsf_backend_log_fn as Log, rsf_fg_frame, rsf_fg_status};
use crate::streamline_policy::camera_forward;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;

#[path = "streamline_generation_records_abi.rs"]
mod abi;
#[path = "../../vulkan/streamline/lifecycle/src/sdk_camera_defaults.rs"]
mod sdk_camera_defaults;
#[allow(dead_code)]
#[path = "../../vulkan/streamline/lifecycle/src/sdk_fg_options_abi.rs"]
mod sdk_fg_options_abi;
#[allow(dead_code)]
#[path = "../../vulkan/streamline/lifecycle/src/sdk_records_abi.rs"]
mod sdk_records_abi;

use abi::{rsf_slhost_reflex_sample, rsf_slhost_tag, rsf_slhost_version};
use sdk_fg_options_abi::rsf_slvk_sdk_fg_options;
use sdk_records_abi::{
    rsf_slvk_sdk_constants, rsf_slvk_sdk_header, rsf_slvk_sdk_pcl_options,
    rsf_slvk_sdk_reflex_options,
};

const REFLEX_ON: u32 = 1;
const REFLEX_SAMPLES: usize = 64;
type Emit = unsafe extern "C" fn(Log, *mut c_void, *const c_char);

#[unsafe(no_mangle)]
extern "C" fn rsf_slhost_format_version(
    version: Option<&rsf_slhost_version>,
    status: &mut rsf_fg_status,
) {
    let text = if let Some(version) = version {
        let [major, minor, build] = version.ngx;
        let [sl_major, sl_minor, sl_build] = version.streamline;
        status.version_id = (u64::from(major) << 48) | (u64::from(minor) << 32) | u64::from(build);
        format!("DLSS-G NGX {major}.{minor}.{build} (SL {sl_major}.{sl_minor}.{sl_build})")
    } else {
        String::from("DLSS-G runtime version unavailable")
    };
    let len = text.len().min(status.version_name.len() - 1);
    for (dst, src) in status.version_name[..len].iter_mut().zip(text.bytes()) {
        *dst = src as c_char;
    }
    status.version_name[len] = 0;
}

fn emit_line(log: Log, user: *mut c_void, emit: Emit, text: String) {
    if let Ok(text) = CString::new(text) {
        unsafe { emit(log, user, text.as_ptr()) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slhost_reflex_summary(
    samples: &[rsf_slhost_reflex_sample; REFLEX_SAMPLES],
    inputs: &[u64; 4],
    available: u32,
    log: Log,
    user: *mut c_void,
    emit: Emit,
) {
    let latest = samples[1..].iter().fold(&samples[0], |best, candidate| {
        if candidate.id > best.id {
            candidate
        } else {
            best
        }
    });
    let ms = |end: u64, start: u64| {
        if end >= start && start != 0 {
            (end - start) as f64 / 1_000.0
        } else {
            -1.0
        }
    };
    let [keyboard, mouse, controller, ping] = *inputs;
    emit_line(
        log,
        user,
        emit,
        format!(
            "Reflex driver report available={available} frame={} sim_ms={:.2} render_ms={:.2} present_ms={:.2} gpu_ms={:.2} gpu_active_us={} gpu_frame_us={} inputs_keyboard={keyboard} mouse={mouse} controller={controller} pcl_ping={ping}",
            latest.id,
            ms(latest.simulation_end, latest.simulation_start),
            ms(latest.submit_end, latest.submit_start),
            ms(latest.present_end, latest.present_start),
            ms(latest.gpu_end, latest.gpu_start),
            latest.gpu_active,
            latest.gpu_frame,
        ),
    );
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slhost_reflex_ring(
    samples: &[rsf_slhost_reflex_sample; REFLEX_SAMPLES],
    reason: *const c_char,
    source: u64,
    log: Log,
    user: *mut c_void,
    emit: Emit,
) {
    let reason = unsafe { CStr::from_ptr(reason) }.to_string_lossy();
    emit_line(
        log,
        user,
        emit,
        format!(
            "Reflex ring {reason} source={source}: one line per driver frame, microseconds from its SimulationStart; -1 means not recorded",
        ),
    );
    let mut previous_sim = 0;
    for sample in samples
        .iter()
        .filter(|sample| sample.id != 0 && sample.simulation_start != 0)
    {
        let at = |time: u64| {
            if time != 0 {
                time.wrapping_sub(sample.simulation_start) as i64
            } else {
                -1
            }
        };
        let sim_interval = if previous_sim == 0 {
            -1
        } else {
            sample.simulation_start.wrapping_sub(previous_sim) as i64
        };
        emit_line(
            log,
            user,
            emit,
            format!(
                "Reflex ring frame={} sim_interval={sim_interval} sim_end={} submit={}..{} present={}..{} driver={}..{} os_queue={}..{} gpu={}..{} gpu_active={} gpu_frame={} copy={}",
                sample.id,
                at(sample.simulation_end),
                at(sample.submit_start),
                at(sample.submit_end),
                at(sample.present_start),
                at(sample.present_end),
                at(sample.driver_start),
                at(sample.driver_end),
                at(sample.queue_start),
                at(sample.queue_end),
                at(sample.gpu_start),
                at(sample.gpu_end),
                sample.gpu_active,
                sample.gpu_frame,
                sample.cross_adapter_copy,
            ),
        );
        previous_sim = sample.simulation_start;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slhost_pack_options(
    mode: u32,
    frames: u32,
    ui_recomposition: u32,
    dynamic_target_fps: f32,
    out: &mut rsf_slvk_sdk_fg_options,
) {
    *out = rsf_slvk_sdk_fg_options {
        base: sdk_fg_options_abi::rsf_slvk_sdk_header {
            next: null_mut(),
            type_lo: 0xfac5_f1cb,
            type_mid: 0x2dfd,
            type_hi: 0x4f36,
            type_tail: [0xa1, 0xe6, 0x3a, 0x9e, 0x86, 0x52, 0x56, 0xc5],
            version: 5,
        },
        mode,
        generated: frames,
        flags: 8,            // eRetainResourcesWhenOff
        reserved_boolean: 2, // eInvalid
        ui_recomposition: u8::from(ui_recomposition != 0),
        target_rate: dynamic_target_fps,
        ..Default::default()
    };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slhost_pack_reflex(mode: u32, limit: u32, out: &mut rsf_slvk_sdk_reflex_options) {
    *out = rsf_slvk_sdk_reflex_options {
        base: rsf_slvk_sdk_header {
            next: null_mut(),
            type_lo: 0xf03a_f81a,
            type_mid: 0x6d0b,
            type_hi: 0x4902,
            type_tail: [0xa6, 0x51, 0xc4, 0x96, 0x5e, 0x21, 0x54, 0x34],
            version: 1,
        },
        mode: match mode {
            RSF_REFLEX_BOOST => 2,
            REFLEX_ON => 1,
            _ => 0,
        },
        interval_us: limit,
        ..Default::default()
    };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slhost_pack_pcl(out: &mut rsf_slvk_sdk_pcl_options) {
    *out = rsf_slvk_sdk_pcl_options {
        base: rsf_slvk_sdk_header {
            next: null_mut(),
            type_lo: 0xcfa3_2f9b,
            type_mid: 0x023c,
            type_hi: 0x420e,
            type_tail: [0x90, 0x56, 0x68, 0x32, 0xb7, 0x4f, 0x89, 0xb4],
            version: 1,
        },
        hotkey: 0, // eUsePingMessage
        thread_id: 0,
    };
}

// The shared FG validator admitted the frame and its camera before packing.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slhost_pack_fg_camera(
    frame: &rsf_fg_frame,
    reset: u32,
    out: &mut rsf_slvk_sdk_constants,
) {
    let camera = &unsafe { &*frame.record }.camera;
    let world = &camera.view_to_world;
    *out = rsf_slvk_sdk_constants {
        view_to_clip: camera.view_to_clip,
        clip_to_view: camera.clip_to_view,
        clip_to_previous: camera.clip_to_previous_clip,
        jitter: camera.jitter_pixels,
        motion_scale: [
            frame.motion_scale_x / frame.motion.width as f32,
            frame.motion_scale_y / frame.motion.height as f32,
        ],
        position: [world[12], world[13], world[14]],
        right: [world[0], world[1], world[2]],
        up: [world[4], world[5], world[6]],
        forward: camera_forward(world, &camera.view_to_clip, 1.0),
        near_plane: camera.near_plane,
        far_plane: camera.far_plane,
        fov: camera.vertical_fov_radians,
        aspect: camera.view_to_clip[5] / camera.view_to_clip[0],
        depth_inverted: u8::from(camera.depth_inverted != 0),
        motion_jittered: u8::from(camera.motion_jittered != 0),
        reset: u8::from(reset != 0),
        ..sdk_camera_defaults::constants()
    };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slhost_plan_tags(frame: &rsf_fg_frame, out: &mut [rsf_slhost_tag; 5]) -> u32 {
    let mut count = 0;
    for (input, kind) in [
        (&frame.backbuffer, 53),
        (&frame.depth, 0),
        (&frame.motion, 1),
        (&frame.hudless, 2),
        (&frame.ui, 23),
    ] {
        if input.resource.is_null() {
            continue;
        }
        out[count] = rsf_slhost_tag {
            resource: input.resource,
            state: input.state,
            kind,
            top: input.y,
            left: input.x,
            width: input.width,
            height: input.height,
        };
        count += 1;
    }
    count as u32
}
