// SPDX-License-Identifier: MIT
//! Rendering-thread ownership of Godot's motion resolve resources.
use core::cell::UnsafeCell;
use core::ffi::{c_char, c_void};
use core::mem::{offset_of, size_of};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Constants {
    matrix: [f32; 16],
    jitter: [f32; 2],
    size: [u32; 2],
}

const _: () = {
    assert!(size_of::<Constants>() == 80);
    assert!(offset_of!(Constants, jitter) == 64 && offset_of!(Constants, size) == 72);
};

#[repr(C)]
pub struct Calls {
    shader: unsafe extern "C" fn(*mut c_void, *const c_char) -> u64,
    pipeline: unsafe extern "C" fn(*mut c_void, u64) -> u64,
    sampler: unsafe extern "C" fn(*mut c_void) -> u64,
    free_rid: unsafe extern "C" fn(*mut c_void, u64),
    texture: unsafe extern "C" fn(*mut c_void, u32) -> u64,
    dispatch: unsafe extern "C" fn(*mut c_void, u64, u64, u64, u64, u64, *const Constants),
}

#[derive(Clone, Copy)]
struct Decoder {
    shader: u64,
    pipeline: u64,
    sampler: u64,
}

struct RenderState(UnsafeCell<Option<Decoder>>);
// SAFETY: both exports are called on the Godot rendering server thread.
unsafe impl Sync for RenderState {}
static DECODER: RenderState = RenderState(UnsafeCell::new(None));

/// # Safety
/// The callbacks, context, facts and output must remain valid for this synchronous
/// rendering-thread call. The callbacks must not reenter motion resolve or release.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_motion_resolve(
    calls: *const Calls,
    context: *mut c_void,
    normalized: u32,
    facts: *const Constants,
    out_depth: *mut u64,
) -> u64 {
    let (calls, mut constants) = unsafe { (&*calls, *facts) };
    unsafe { out_depth.write(0) };
    let state = unsafe { &mut *DECODER.0.get() };
    let decoder = if let Some(decoder) = *state {
        decoder
    } else {
        let source = if normalized != 0 {
            concat!(
                "\n#version 450\n#define RSF_NORMALIZE_DEPTH\n",
                include_str!("motion.glsl"),
                "\0"
            )
        } else {
            concat!("\n#version 450\n", include_str!("motion.glsl"), "\0")
        };
        let shader = unsafe { (calls.shader)(context, source.as_ptr().cast()) };
        if shader == 0 {
            return 0;
        }
        let pipeline = unsafe { (calls.pipeline)(context, shader) };
        let sampler = unsafe { (calls.sampler)(context) };
        if pipeline == 0 || sampler == 0 {
            // Godot retires the pipeline with its owning shader.
            unsafe { (calls.free_rid)(context, shader) };
            if sampler != 0 {
                unsafe { (calls.free_rid)(context, sampler) };
            }
            return 0;
        }
        let decoder = Decoder {
            shader,
            pipeline,
            sampler,
        };
        *state = Some(decoder);
        decoder
    };

    let motion = unsafe { (calls.texture)(context, 0) };
    if motion == 0 {
        return 0;
    }
    let depth = if normalized != 0 {
        let depth = unsafe { (calls.texture)(context, 1) };
        if depth == 0 {
            return 0;
        }
        depth
    } else {
        0
    };
    constants.jitter = [constants.jitter[0] * 0.5, constants.jitter[1] * 0.5];
    unsafe {
        (calls.dispatch)(
            context,
            decoder.shader,
            decoder.pipeline,
            decoder.sampler,
            motion,
            depth,
            &constants,
        );
        out_depth.write(depth);
    }
    motion
}

/// # Safety
/// Godot must call this on its synchronized rendering thread with valid callbacks and context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_motion_release(calls: *const Calls, context: *mut c_void) {
    let decoder = unsafe { (*DECODER.0.get()).take() };
    if let Some(decoder) = decoder {
        let free = unsafe { (*calls).free_rid };
        unsafe {
            free(context, decoder.shader);
            free(context, decoder.sampler);
        }
    }
}
