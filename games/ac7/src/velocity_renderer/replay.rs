// SPDX-License-Identifier: MIT
//! Scope facts and synchronous replay leases for the native D3D11 submission leaves.
use super::{lock, release_renderer};
use crate::scene_color_abi::rsf_frame_tap_geometry;
use crate::velocity_commit::{Log, set_log};
use crate::velocity_stream::draw_admitted;
use rsf_game_sys::rsf_game_render_pass;
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void};
use std::sync::Mutex;

static REPLAY: Mutex<()> = Mutex::new(());

thread_local! {
    static SCOPE: RefCell<Option<rsf_game_render_pass>> = const { RefCell::new(None) };
    static MERGING: Cell<bool> = const { Cell::new(false) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_set_log(
    log: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    user: *mut c_void,
) {
    let _lease = lock(&REPLAY);
    set_log(log.map(|write| Log { write, user }));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_scope(scope: *const rsf_game_render_pass) {
    // The queued renderer lends the exact pass. Only its leading size is read before admission.
    let pass = if !scope.is_null()
        && unsafe { (*scope).struct_size as usize } >= size_of::<rsf_game_render_pass>()
    {
        Some(unsafe { scope.read() })
    } else {
        None
    };
    let _ = SCOPE.replace(pass);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_replay_draw(
    draw: *const rsf_frame_tap_geometry,
    submit: unsafe extern "C" fn(*const rsf_game_render_pass, *const rsf_frame_tap_geometry),
) {
    if MERGING.get() {
        return;
    }
    const { assert!(!std::mem::needs_drop::<rsf_game_render_pass>()) };
    // Copy only the borrowed record. Release the TLS borrow before calling D3D11.
    let scope = SCOPE.with_borrow(|pass| {
        pass.as_ref()
            .map(|pass| unsafe { std::ptr::read(std::ptr::from_ref(pass)) })
    });
    let (Some(scope), Some(draw)) = (scope, unsafe { draw.as_ref() }) else {
        return;
    };
    if !draw_admitted(&scope, draw) {
        return;
    }
    let _lease = lock(&REPLAY);
    // The geometry observation and scope snapshot are borrowed through submission.
    unsafe { submit(&scope, draw) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_replay_commit(
    context: *mut c_void,
    pass: *const rsf_game_render_pass,
    submit: unsafe extern "C" fn(*mut c_void, *const rsf_game_render_pass) -> i32,
) -> i32 {
    let _lease = lock(&REPLAY);
    unsafe { submit(context, pass) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_replay_merge(
    context: *mut c_void,
    draw: unsafe extern "C" fn(*mut c_void),
) {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            MERGING.set(self.0);
        }
    }
    let _restore = Restore(MERGING.replace(true));
    unsafe { draw(context) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_replay_frame_end(retire: unsafe extern "C" fn()) {
    let _lease = lock(&REPLAY);
    unsafe { retire() };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_replay_count(count: unsafe extern "C" fn() -> u32) -> u32 {
    let _lease = lock(&REPLAY);
    unsafe { count() }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_velocity_release() {
    let _lease = lock(&REPLAY);
    release_renderer();
    let _ = SCOPE.replace(None);
}
