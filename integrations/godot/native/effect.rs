// SPDX-License-Identifier: MIT
//! The `ReScaleFrameEffect` compositor probe owner, one per effect instance (`effect.cpp`): the
//! pending motion capture request, the scene buffer admission, the capture admission, the
//! classification of the readback, the callback count, the snapshot and capture each callback
//! publishes, and the probe's published stage, reason and inactive pipeline claims.
//!
//! The class keeps its godot-cpp registration and callback contract (`POST_TRANSPARENT`, resolved
//! color and depth, motion vectors), the scene buffer, scene data and RenderingDevice reads, the
//! readback compute pass and the status Dictionary keys, and lends them as `EffectLeaves`. Scene
//! objects, RIDs and native pointers stay in the leaves' callback record; this owner keeps copied
//! CPU facts only, and no resource lease or GPU completion is implied. The effect reads no
//! Runtime settings or status: `ReScaleFrameRuntime` reports active SR, FG and latency.
use crate::platform::{
    EFFECT_MOTION_FORMAT, EFFECT_VERSION, EffectFrame, EffectLeaves, EffectReadback,
    EffectResources, EffectStatus,
};
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

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
    pub(crate) fn render(&self, leaves: &EffectLeaves, call: *mut c_void) {
        let requested = self.requested.swap(false, Ordering::SeqCst);
        let mut next = Snapshot::default();
        let mut sampled = Capture::default();
        if let Some(frame) = leaves.frame(call) {
            next.frame = frame;
            next.buffers_available = buffers_admitted(&frame);
            if next.buffers_available {
                next.resources = leaves.resources(call);
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
                        sampled = leaves.read_motion(call, &plan, |bytes| classify(bytes, pixels));
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
            struct_size: size_of::<EffectStatus>() as u32,
            version: EFFECT_VERSION,
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

/// The SDK owns RIDs and bytes through this synchronous call. Record only after
/// shader creation and all four input leases have succeeded.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_effect_record_motion(
    call: *mut c_void,
    plan: *const EffectReadback,
    compile: unsafe extern "C" fn(*mut c_void, *const EffectReadback) -> u32,
    prepare: unsafe extern "C" fn(*mut c_void, *const EffectReadback, *mut u32) -> u32,
    dispatch: unsafe extern "C" fn(*mut c_void, *const EffectReadback) -> u32,
) -> u32 {
    let mut valid = [0; 4];
    if unsafe { compile(call, plan) } == 0
        || unsafe { prepare(call, plan, valid.as_mut_ptr()) } == 0
        || valid.contains(&0)
    {
        return 0;
    }
    unsafe { dispatch(call, plan) }
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

#[cfg(test)]
mod tests;
