// SPDX-License-Identifier: MIT
use crate::camera;
use crate::sys::{
    RSF_ANOMALY_SR_MAIN_VIEW_KEY, RSF_ANOMALY_SR_MOTION_CAMERA_INCLUDED,
    RSF_ANOMALY_SR_MOTION_TO_UV, rsf_anomaly_sr_pass_inputs,
};
use rsf_game_sys::{
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR,
    rsf_game_render_pass,
};
use std::mem::MaybeUninit;

pub(crate) use crate::sys::rsf_anomaly_sr_reset_facts as ResetFacts;
pub(crate) type PassInputs = rsf_anomaly_sr_pass_inputs;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) enum ResetReason {
    None,
    First,
    Sequence,
    Cut,
    Settings,
    Device,
}

pub(crate) fn reset_reason(facts: &ResetFacts) -> ResetReason {
    if facts.first {
        ResetReason::First
    } else if facts.device {
        ResetReason::Device
    } else if facts.settings {
        ResetReason::Settings
    } else if facts.sequence {
        ResetReason::Sequence
    } else if facts.cut {
        ResetReason::Cut
    } else {
        ResetReason::None
    }
}

#[repr(u8)]
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub(crate) enum PassError {
    Sizes = 1,
    Camera,
    Projection,
    Jitter,
    Resources,
}

impl PassError {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Sizes => "sizes",
            Self::Camera => "camera",
            Self::Projection => "projection",
            Self::Jitter => "jitter",
            Self::Resources => "resources",
        }
    }
}

pub(crate) fn build_pass(input: &PassInputs) -> Result<rsf_game_render_pass, PassError> {
    let frame = &input.frame;
    let (rw, rh, ow, oh) = (
        frame.render_width,
        frame.render_height,
        frame.output_width,
        frame.output_height,
    );
    if rw == 0 || rh == 0 || rw > ow || rh > oh {
        return Err(PassError::Sizes);
    }
    if [input.color, input.color_output, input.depth, input.motion]
        .iter()
        .any(|texture| texture.is_null())
    {
        return Err(PassError::Resources);
    }
    let current = &frame.current;
    if !current.valid {
        return Err(PassError::Camera);
    }
    let jitter = &frame.jitter;
    // The projection used for rasterization must carry this main view's render-pixel jitter.
    if !jitter.applied
        || jitter.scope
        || [jitter.render_width, jitter.render_height] != [rw, rh]
        || !jitter.current_x.is_finite()
        || !jitter.current_y.is_finite()
    {
        return Err(PassError::Jitter);
    }
    let planes = camera::projection_planes(&current.projection).ok_or(PassError::Projection)?;
    let previous = if input.reset || !frame.previous.valid {
        current
    } else {
        &frame.previous
    };
    let (mut camera, previous_clip_to_clip) = camera::frame(
        &current.view,
        &current.projection,
        &previous.view,
        &previous.projection,
        planes,
        [rw, rh, ow, oh],
        frame.frame_seconds,
    )
    .ok_or(PassError::Camera)?;
    camera.jitter_pixels = [jitter.current_x, jitter.current_y];
    if jitter.previous_applied {
        camera.previous_jitter_pixels = [jitter.previous_x, jitter.previous_y];
    }

    let key = u64::from(RSF_ANOMALY_SR_MAIN_VIEW_KEY);
    Ok(rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        role: RSF_GAME_RENDER_SR,
        session_id: input.session,
        family_key: key,
        view_key: key,
        pass_key: key,
        history_key: key,
        native_frame: frame.native_frame,
        resource_generation: frame.generation,
        flags: RSF_GAME_RENDER_PRIMARY
            | RSF_GAME_RENDER_NO_FG
            | if input.reset {
                RSF_GAME_RENDER_RESET
            } else {
                0
            },
        render_rect: [0, 0, rw as i32, rh as i32],
        output_rect: [0, 0, ow as i32, oh as i32],
        color_input: input.color,
        color_output: input.color_output,
        depth: input.depth,
        motion: input.motion,
        exposure: input.exposure,
        camera,
        previous_clip_to_clip,
        camera_valid: 1,
        motion_to_uv: [RSF_ANOMALY_SR_MOTION_TO_UV as f32; 2],
        motion_camera_included: RSF_ANOMALY_SR_MOTION_CAMERA_INCLUDED,
        // The remaining SDK fields consist of numbers, arrays and nullable raw pointers.
        ..unsafe { std::mem::zeroed() }
    })
}

// Private fixture entry points. Production calls the same functions directly in Rust.
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_build_pass(
    input: &PassInputs,
    out: &mut MaybeUninit<rsf_game_render_pass>,
) -> u8 {
    match build_pass(input) {
        Ok(pass) => {
            out.write(pass);
            0
        }
        Err(error) => {
            // A native caller may reuse the previous pass's storage after a refusal.
            unsafe { out.as_mut_ptr().write_bytes(0, 1) };
            error as u8
        }
    }
}

#[cfg(test)]
mod tests;
