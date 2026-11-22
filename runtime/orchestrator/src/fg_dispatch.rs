// SPDX-License-Identifier: MIT
use crate::render_abi::rsf_dlss_frame as Frame;
use crate::sr_session::has_capacity;
use rsf_game_sys::{
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_SUBMISSION,
    RSF_GAME_RENDER_TEXTURE_BINDING, RSF_GAME_RENDER_WINDOW, rsf_game_render_pass as Pass,
};
use std::ffi::c_void;

#[repr(C)]
struct PassCallbacks {
    frame: unsafe extern "C" fn(*const Pass, u32),
    submission: unsafe extern "C" fn(*const Pass, u32),
    scene: unsafe extern "C" fn(*const Pass, u32) -> i32,
    final_scene: unsafe extern "C" fn(*mut c_void, *const Pass),
    texture_binding: unsafe extern "C" fn(*const Pass) -> i32,
    window_end: unsafe extern "C" fn(*const Pass),
    window_scope: unsafe extern "C" fn(*const Pass, u32) -> i32,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg_dispatch_pass(
    context: *mut c_void,
    pass: *const Pass,
    begin: u32,
    callbacks: &PassCallbacks,
) -> i32 {
    if !unsafe { has_capacity(pass) } {
        return 0;
    }
    // These inputs are borrowed for the duration of the native call.
    unsafe {
        match (*pass).role {
            RSF_GAME_RENDER_FRAME => (callbacks.frame)(pass, begin),
            RSF_GAME_RENDER_SUBMISSION => (callbacks.submission)(pass, begin),
            RSF_GAME_RENDER_FINAL_SCENE => {
                let accepted = (callbacks.scene)(pass, begin);
                if begin == 0 && !context.is_null() {
                    (callbacks.final_scene)(context, pass);
                }
                return accepted;
            }
            RSF_GAME_RENDER_TEXTURE_BINDING if begin == 0 => {
                return (callbacks.texture_binding)(pass);
            }
            RSF_GAME_RENDER_WINDOW => {
                if begin == 0 {
                    (callbacks.window_end)(pass);
                }
                return (callbacks.window_scope)(pass, begin);
            }
            _ => {}
        }
    }
    1
}

type SwapScene = unsafe extern "C" fn(*mut Pass) -> i32;
type SetScene = unsafe extern "C" fn(*const Pass);
type Capture = unsafe extern "C" fn(*mut c_void, *const Frame);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg_capture_scoped(
    context: *mut c_void,
    identity: *const Pass,
    frame: *const Frame,
    swap_scene: SwapScene,
    set_scene: SetScene,
    capture: Capture,
) {
    if context.is_null() || !unsafe { has_capacity(identity) && has_capacity(frame) } {
        return;
    }
    // The native setter normalizes identity against a bound source. Save and
    // restore the exact outer identity around that synchronous operation.
    let mut previous = Pass {
        struct_size: size_of::<Pass>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        if swap_scene(&mut previous) == 0 {
            return;
        }
        set_scene(identity);
        capture(context, frame);
        swap_scene(&mut previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static SOURCE: Cell<u64> = const { Cell::new(0) };
    }

    unsafe extern "C" fn swap(pass: *mut Pass) -> i32 {
        unsafe { (*pass).source_frame_id = SOURCE.replace((*pass).source_frame_id) };
        1
    }

    unsafe extern "C" fn set(pass: *const Pass) {
        SOURCE.set(unsafe { (*pass).source_frame_id });
    }

    unsafe extern "C" fn capture(context: *mut c_void, _frame: *const Frame) {
        unsafe { context.cast::<u64>().write(SOURCE.get()) };
    }

    #[test]
    fn capture_restores_the_outer_scene_and_refuses_a_short_frame() {
        let identity = Pass {
            struct_size: size_of::<Pass>() as u32,
            source_frame_id: 42,
            ..unsafe { std::mem::zeroed() }
        };
        let mut frame: Frame = unsafe { std::mem::zeroed() };
        let mut captured = 0_u64;
        let context = std::ptr::from_mut(&mut captured).cast();
        SOURCE.set(27);
        unsafe { rsf_core_fg_capture_scoped(context, &identity, &frame, swap, set, capture) };
        assert_eq!(captured, 0);
        assert_eq!(SOURCE.get(), 27);

        frame.struct_size = size_of::<Frame>() as u32;
        unsafe { rsf_core_fg_capture_scoped(context, &identity, &frame, swap, set, capture) };
        assert_eq!(captured, 42);
        assert_eq!(SOURCE.get(), 27);
    }
}
