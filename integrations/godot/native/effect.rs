// SPDX-License-Identifier: MIT
//! The `ReScaleFrameEffect` compositor probe owner, one per effect instance
//! (`gdextension/effect.rs`): the pending motion capture request, the scene buffer admission, the
//! capture admission, the classification of the readback, the callback count, the snapshot and
//! capture each callback publishes, and the probe's published stage, reason and inactive pipeline
//! claims.
//!
//! The class keeps its registration and callback contract (`POST_TRANSPARENT`, resolved color and
//! depth, motion vectors), the scene buffer, scene data and RenderingDevice reads, the readback
//! compute calls and the Variant conversions. Scene
//! objects, RIDs and native pointers stay in the leaves' callback record; this owner keeps copied
//! CPU facts only, and no resource lease or GPU completion is implied. The effect reads no
//! Runtime settings or status: `ReScaleFrameRuntime` reports active SR, FG and latency.
#[cfg(not(test))]
use crate::gdextension::effect::{leaf_frame, leaf_read_motion, leaf_resources};
use crate::platform::{
    EFFECT_MOTION_FORMAT, EffectFrame, EffectReadback, EffectResources, EffectStatus,
};
use crate::properties::Properties;
use crate::sys::*;
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
#[cfg(test)]
use tests::{leaf_frame, leaf_read_motion, leaf_resources};

pub(crate) const STAGE: &CStr = c"renderer_probe";
pub(crate) const REASON: &CStr = c"Compositor motion producer/probe only. Query ReScaleFrameRuntime for active SR, FG and latency.";
pub(crate) const SNAPSHOT_SCOPE: &CStr = c"last_post_transparent_callback";

/// One readback texel: the shader's `vec2` in a std430 array, two little-endian `f32`.
const SAMPLE_BYTES: usize = 8;
/// A finite texel moves when either component's magnitude exceeds this, in UV.
const MOVING_UV: f64 = 0.00001;

// Convert the sampleable velocity texture into an owned readback buffer.
const MOTION_SHADER: &CStr = c"#version 450
layout(local_size_x=8, local_size_y=8) in;
layout(set=0,binding=0) uniform sampler2D motion_tex;
layout(set=0,binding=1,std430) writeonly buffer Samples { vec2 velocity[]; };
void main() {
    ivec2 p=ivec2(gl_GlobalInvocationID.xy), size=textureSize(motion_tex,0);
    if(any(greaterThanEqual(p,size))) return;
    velocity[p.y*size.x+p.x]=texelFetch(motion_tex,p,0).xy;
}";

/// What the last callback published.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) callbacks: u64,
    pub(crate) frame: EffectFrame,
    pub(crate) resources: EffectResources,
    pub(crate) buffers_available: bool,
}

/// The last requested capture. An attempt that read nothing is still counted, unsucceeded.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Capture {
    pub(crate) count: u64,
    /// The callback count the capture was taken in.
    pub(crate) callback: u64,
    pub(crate) pixels: u64,
    pub(crate) moving_pixels: u64,
    pub(crate) nonfinite_pixels: u64,
    pub(crate) unwritten_pixels: u64,
    pub(crate) max_abs_uv: f64,
    pub(crate) succeeded: bool,
}

#[derive(Default)]
struct Published {
    snapshot: Snapshot,
    capture: Capture,
}

#[derive(Default)]
pub(crate) struct Effect {
    /// Set by `request_motion_capture` on any thread, taken by the next callback.
    requested: AtomicBool,
    published: Mutex<Published>,
}

impl Effect {
    fn published(&self) -> MutexGuard<'_, Published> {
        self.published
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn request_capture(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    /// One `POST_TRANSPARENT` callback on the render thread. The request is taken first, so a
    /// callback that cannot read the motion still answers it with an unsucceeded capture.
    pub(crate) fn render(&self, call: *mut c_void) {
        let requested = self.requested.swap(false, Ordering::SeqCst);
        let mut next = Snapshot::default();
        let mut sampled = Capture::default();
        if let Some(frame) = frame(call) {
            next.frame = frame;
            next.buffers_available = buffers_admitted(&frame);
            if next.buffers_available {
                next.resources = resources(call);
                if requested && next.resources.motion_format == EFFECT_MOTION_FORMAT {
                    // Explicit bounded diagnostics only: the readback stalls the GPU.
                    let pixels = pixels(&frame);
                    if let Some(bytes) = pixels
                        .checked_mul(SAMPLE_BYTES as u64)
                        .and_then(|bytes| u32::try_from(bytes).ok())
                    {
                        let plan = EffectReadback {
                            shader: MOTION_SHADER.as_ptr(),
                            bytes,
                            groups_x: frame.internal_width.unsigned_abs().div_ceil(8),
                            groups_y: frame.internal_height.unsigned_abs().div_ceil(8),
                        };
                        sampled = read_motion(call, &plan, |bytes| classify(bytes, pixels));
                    }
                }
            }
        }
        let mut published = self.published();
        next.callbacks = published.snapshot.callbacks.wrapping_add(1);
        if requested {
            sampled.count = published.capture.count.wrapping_add(1);
            sampled.callback = next.callbacks;
            published.capture = sampled;
        }
        published.snapshot = next;
    }

    pub(crate) fn status(&self) -> EffectStatus {
        let (snapshot, capture) = {
            let published = self.published();
            (published.snapshot, published.capture)
        };
        EffectStatus {
            callbacks: snapshot.callbacks,
            frame: snapshot.frame,
            resources: snapshot.resources,
            buffers_available: snapshot.buffers_available.into(),
            capture_succeeded: capture.succeeded.into(),
            captures: capture.count,
            capture_callback: capture.callback,
            capture_pixels: capture.pixels,
            capture_moving_pixels: capture.moving_pixels,
            capture_nonfinite_pixels: capture.nonfinite_pixels,
            capture_unwritten_pixels: capture.unwritten_pixels,
            capture_max_abs_uv: capture.max_abs_uv,
            stage: STAGE.as_ptr(),
            reason: REASON.as_ptr(),
            snapshot_scope: SNAPSHOT_SCOPE.as_ptr(),
            // A probe: SR, FG and latency are the Runtime's to report.
            rendering_ready: 0,
            sr_active: 0,
            fg_active: 0,
            latency_active: 0,
        }
    }
}

/// Only the SDK leaves can allocate, record and release Godot resources. Finish
/// by releasing the resources created up to that point, even after a refusal.
// Called only by the gdextension entry points, which unit tests do not build.
#[cfg_attr(test, allow(dead_code))]
pub(crate) unsafe extern "C" fn rsf_godot_effect_record_motion(
    call: *mut c_void,
    plan: *const EffectReadback,
    calls: *const rsf_godot_motion_calls,
) -> u32 {
    let Some(rsf_godot_motion_calls {
        compile: Some(compile),
        prepare: Some(prepare),
        dispatch: Some(dispatch),
        read: Some(read),
        free_resource: Some(free),
    }) = (unsafe { calls.as_ref() })
    else {
        return 0;
    };
    let mut valid = [0; 4];
    let mut completed = unsafe { compile(call, plan) } != 0
        && unsafe { prepare(call, plan, valid.as_mut_ptr()) } != 0
        && !valid.contains(&0)
        && unsafe { dispatch(call, plan) } != 0
        && unsafe { read(call) } != 0;

    for resource in [
        RSF_GODOT_MOTION_UNIFORMS,
        RSF_GODOT_MOTION_BUFFER,
        RSF_GODOT_MOTION_SAMPLER,
        RSF_GODOT_MOTION_PIPELINE,
        RSF_GODOT_MOTION_SHADER,
    ] {
        completed &= unsafe { free(call, resource) } != 0;
    }
    u32::from(completed)
}

// Called only by the gdextension entry points, which unit tests do not build.
#[cfg_attr(test, allow(dead_code))]
pub(crate) unsafe extern "C" fn rsf_godot_effect_properties(
    effect: *const Effect,
    version: *const c_char,
    user: *mut c_void,
    emit: rsf_godot_property_emit,
) {
    if version.is_null() {
        return;
    }
    let (Some(effect), Some(fields)) = (unsafe { effect.as_ref() }, unsafe {
        Properties::new(user, emit)
    }) else {
        return;
    };
    let status = effect.status();
    fields.text(c"version", unsafe { CStr::from_ptr(version) });
    fields.text(c"stage", STAGE);
    fields.boolean(c"rendering_ready", status.rendering_ready != 0);
    fields.boolean(c"sr_active", status.sr_active != 0);
    fields.boolean(c"fg_active", status.fg_active != 0);
    fields.boolean(c"latency_active", status.latency_active != 0);
    fields.text(c"reason", REASON);
    fields.renderer(c"renderer");
    fields.driver(c"driver");
    fields.text(c"snapshot_scope", SNAPSHOT_SCOPE);
    fields.integer(c"callbacks", status.callbacks as i64);
    let frame = &status.frame;
    fields.integer(c"render_target_id", frame.render_target as i64);
    fields.integer(c"view_count", i64::from(frame.views));
    fields.extent(
        c"internal_size",
        [frame.internal_width, frame.internal_height],
    );
    fields.extent(c"output_size", [frame.output_width, frame.output_height]);
    fields.transform(c"camera_transform", &frame.camera_transform);
    fields.projection(c"jittered_projection", &frame.jittered_projection);
    let resources = &status.resources;
    fields.integer(c"color_format", i64::from(resources.color_format));
    fields.integer(c"depth_format", i64::from(resources.depth_format));
    fields.integer(c"motion_format", i64::from(resources.motion_format));
    fields.boolean(c"buffers_available", status.buffers_available != 0);
    fields.boolean(c"native_device_available", resources.native_device != 0);
    fields.boolean(c"native_queue_available", resources.native_queue != 0);
    fields.boolean(c"native_textures_available", resources.native_textures != 0);
    fields.integer(c"motion_captures", status.captures as i64);
    fields.integer(c"capture_callback", status.capture_callback as i64);
    fields.boolean(c"capture_succeeded", status.capture_succeeded != 0);
    fields.integer(c"capture_pixels", status.capture_pixels as i64);
    fields.integer(
        c"capture_moving_pixels",
        status.capture_moving_pixels as i64,
    );
    fields.integer(
        c"capture_nonfinite_pixels",
        status.capture_nonfinite_pixels as i64,
    );
    fields.integer(
        c"capture_unwritten_pixels",
        status.capture_unwritten_pixels as i64,
    );
    fields.real(c"capture_max_abs_uv", status.capture_max_abs_uv);
}

/// One view with a nonempty internal extent and valid color, depth and velocity layers.
pub(crate) fn buffers_admitted(frame: &EffectFrame) -> bool {
    frame.views == 1
        && frame.internal_width > 0
        && frame.internal_height > 0
        && frame.color_valid != 0
        && frame.depth_valid != 0
        && frame.motion_valid != 0
}

/// The readback's texel count at the internal extent, which the leaf's buffer is sized for.
fn pixels(frame: &EffectFrame) -> u64 {
    u64::from(frame.internal_width.unsigned_abs()) * u64::from(frame.internal_height.unsigned_abs())
}

/// Classifies a readback of `pixels` texels of R16G16_SFLOAT motion in UV, decoded as Godot's
/// `PackedByteArray.decode_float` does (little-endian `f32` widened to `f64`). A readback of any
/// other length fails and counts nothing. Each texel is non-finite when either component is;
/// otherwise unwritten when both are at most -1 (Forward+ clears the velocity-only framebuffer to
/// (-1, -1) before its motion pass in the pinned 4.7.2 source); otherwise its larger magnitude
/// feeds the maximum and it moves above `MOVING_UV`. Valid zero motion is neither.
pub(crate) fn classify(bytes: &[u8], pixels: u64) -> Capture {
    let mut capture = Capture {
        pixels,
        succeeded: bytes.len() as u64 == pixels.wrapping_mul(SAMPLE_BYTES as u64),
        ..Capture::default()
    };
    if !capture.succeeded {
        return capture;
    }
    for &[x0, x1, x2, x3, y0, y1, y2, y3] in bytes.as_chunks::<SAMPLE_BYTES>().0 {
        let x = f64::from(f32::from_le_bytes([x0, x1, x2, x3]));
        let y = f64::from(f32::from_le_bytes([y0, y1, y2, y3]));
        if !x.is_finite() || !y.is_finite() {
            capture.nonfinite_pixels += 1;
        } else if x <= -1.0 && y <= -1.0 {
            capture.unwritten_pixels += 1;
        } else {
            let magnitude = x.abs().max(y.abs());
            if magnitude > MOVING_UV {
                capture.moving_pixels += 1;
            }
            capture.max_abs_uv = capture.max_abs_uv.max(magnitude);
        }
    }
    capture
}

// The scene and RenderingDevice reads of gdextension/effect.rs (the recording fakes under test).
// SAFETY for every unsafe block below: `call` is the render callback's live record, which outlives
// the owner's render call.

/// The frame facts; `None` without a RenderingDevice, scene buffers or scene data, or when the
/// leaf threw.
fn frame(call: *mut c_void) -> Option<EffectFrame> {
    let mut frame = EffectFrame::default();
    (unsafe { leaf_frame(call, &mut frame) } != 0).then_some(frame)
}

/// The formats and native handle facts; the defaults when the leaf threw.
fn resources(call: *mut c_void) -> EffectResources {
    let mut resources = EffectResources::default();
    if unsafe { leaf_resources(call, &mut resources) } == 0 {
        resources = EffectResources::default();
    }
    resources
}

/// Runs `read` over the readback bytes, which `call` keeps alive until the callback ends.
/// An empty slice when the readback was refused or the leaf threw.
fn read_motion<R>(call: *mut c_void, plan: &EffectReadback, read: impl FnOnce(&[u8]) -> R) -> R {
    let (mut data, mut size): (*const u8, u64) = (std::ptr::null(), 0);
    let completed = unsafe { leaf_read_motion(call, plan, &mut data, &mut size) } != 0;
    if !completed || data.is_null() || size == 0 {
        return read(&[]);
    }
    // SAFETY: the leaf's bytes stay in `call` for the callback.
    read(unsafe { std::slice::from_raw_parts(data, size as usize) })
}
#[cfg(test)]
mod tests;
