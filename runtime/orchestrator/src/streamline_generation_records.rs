// SPDX-License-Identifier: MIT
//! D3D12 FG payloads and reports, built from the same checked SDK records as Vulkan.
use crate::streamline_policy::camera_forward;
use crate::streamline_sdk::{
    self as sdk, Constants, FeatureVersion, FgOptions, ReflexOptions, ReflexState,
    default_constants,
};
use crate::sys::{RSF_REFLEX_BOOST, rsf_backend_log_fn as Log, rsf_fg_frame, rsf_fg_status};
use std::ffi::{CStr, CString, c_char, c_void};

const REFLEX_ON: u32 = 1;

pub(crate) fn format_version(version: Option<&FeatureVersion>, status: &mut rsf_fg_status) {
    let text = if let Some(version) = version {
        let (ngx, sl) = (&version.ngx, &version.streamline);
        status.version_id =
            (u64::from(ngx.major) << 48) | (u64::from(ngx.minor) << 32) | u64::from(ngx.build);
        format!(
            "DLSS-G NGX {}.{}.{} (SL {}.{}.{})",
            ngx.major, ngx.minor, ngx.build, sl.major, sl.minor, sl.build
        )
    } else {
        String::from("DLSS-G runtime version unavailable")
    };
    let len = text.len().min(status.version_name.len() - 1);
    for (dst, src) in status.version_name[..len].iter_mut().zip(text.bytes()) {
        *dst = src as c_char;
    }
    status.version_name[len] = 0;
}

fn emit_line(log: Log, user: *mut c_void, text: String) {
    if let (Some(log), Ok(text)) = (log, CString::new(text)) {
        unsafe { log(user, text.as_ptr()) };
    }
}

pub(crate) fn report_support(fg: u32, reflex: u32, log: Log, user: *mut c_void) {
    emit_line(
        log,
        user,
        format!("DLSS-G support={fg} Reflex support={reflex}"),
    );
}

pub(crate) fn report_reflex_options(
    mode: u32,
    limit: u32,
    marker_optimization: u32,
    log: Log,
    user: *mut c_void,
) {
    emit_line(
        log,
        user,
        format!(
            "Reflex options applied: mode={mode} frameLimitUs={limit} markerOptimization={marker_optimization}"
        ),
    );
}

pub(crate) fn reflex_summary(
    state: &ReflexState,
    inputs: &[u64; 4],
    available: u32,
    log: Log,
    user: *mut c_void,
) {
    let reports = &state.reports;
    let latest = reports[1..].iter().fold(&reports[0], |best, candidate| {
        if candidate.frame_id > best.frame_id {
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
        format!(
            "Reflex driver report available={available} frame={} sim_ms={:.2} render_ms={:.2} present_ms={:.2} gpu_ms={:.2} gpu_active_us={} gpu_frame_us={} inputs_keyboard={keyboard} mouse={mouse} controller={controller} pcl_ping={ping}",
            latest.frame_id,
            ms(latest.simulation_end, latest.simulation_start),
            ms(latest.submit_end, latest.submit_start),
            ms(latest.present_end, latest.present_start),
            ms(latest.gpu_end, latest.gpu_start),
            latest.gpu_active_us,
            latest.gpu_frame_us,
        ),
    );
}

pub(crate) fn reflex_ring(
    state: &ReflexState,
    reason: &CStr,
    source: u64,
    log: Log,
    user: *mut c_void,
) {
    let reason = reason.to_string_lossy();
    emit_line(
        log,
        user,
        format!(
            "Reflex ring {reason} source={source}: one line per driver frame, microseconds from its SimulationStart; -1 means not recorded",
        ),
    );
    let mut previous_sim = 0;
    for (sample, extra) in state
        .reports
        .iter()
        .zip(&state.reports2)
        .filter(|(sample, _)| sample.frame_id != 0 && sample.simulation_start != 0)
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
            format!(
                "Reflex ring frame={} sim_interval={sim_interval} sim_end={} submit={}..{} present={}..{} driver={}..{} os_queue={}..{} gpu={}..{} gpu_active={} gpu_frame={} copy={}",
                sample.frame_id,
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
                sample.gpu_active_us,
                sample.gpu_frame_us,
                extra.cross_adapter_copy_us,
            ),
        );
        previous_sim = sample.simulation_start;
    }
}

/// `sl::DLSSGOptions` v5 with eRetainResourcesWhenOff and the requested mode, frame count, UI
/// recomposition and dynamic target rate.
pub(crate) fn pack_options(
    mode: u32,
    frames: u32,
    ui_recomposition: u32,
    dynamic_target_fps: f32,
) -> FgOptions {
    FgOptions {
        mode,
        generated: frames,
        flags: 8, // eRetainResourcesWhenOff
        ui_recomposition: u8::from(ui_recomposition != 0),
        target_rate: dynamic_target_fps,
        ..sdk::fg_options(None, 0, 0, 0, 0)
    }
}

/// Off, On or Boost (other modes select Off) with a frame limit in microseconds.
pub(crate) fn pack_reflex(mode: u32, limit: u32) -> ReflexOptions {
    let mode = match mode {
        RSF_REFLEX_BOOST => 2,
        REFLEX_ON => 1,
        _ => 0,
    };
    sdk::reflex_options(mode, limit, 0)
}

// The shared FG validator admitted the frame and its camera before packing.
pub(crate) unsafe fn pack_fg_camera(frame: &rsf_fg_frame, reset: u32) -> Constants {
    let camera = &unsafe { &*frame.record }.camera;
    let world = &camera.view_to_world;
    Constants {
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
        ..default_constants()
    }
}

/// One frame-generation input to tag.
#[derive(Clone, Copy)]
pub(crate) struct PlannedTag {
    pub(crate) resource: *mut c_void,
    pub(crate) state: u32,
    pub(crate) kind: u32,
    /// Top, left, width and height.
    pub(crate) extent: [u32; 4],
}

/// The frame's present inputs in tag order: backbuffer, depth, motion, HUD-less color and UI.
pub(crate) fn plan_tags(frame: &rsf_fg_frame) -> ([PlannedTag; 5], u32) {
    let mut out = [PlannedTag {
        resource: std::ptr::null_mut(),
        state: 0,
        kind: 0,
        extent: [0; 4],
    }; 5];
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
        out[count] = PlannedTag {
            resource: input.resource,
            state: input.state,
            kind,
            extent: [input.y, input.x, input.width, input.height],
        };
        count += 1;
    }
    (out, count as u32)
}
