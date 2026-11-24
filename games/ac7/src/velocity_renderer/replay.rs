// SPDX-License-Identifier: MIT
//! Scope facts and synchronous leases for AC7 D3D11 replay.
use super::d3d11::{Pipeline, Renderer, clock_ms};
#[cfg(feature = "warp-fixture")]
use super::release_renderer;
use super::{LIFETIME, admit, lock, published, reset_for_renderer};
use crate::sys::rsf_frame_tap_geometry;
use crate::velocity_commit::{Log, log_frame_end, refused, set_log};
use crate::velocity_history;
use crate::velocity_stream::draw_admitted;
use rsf_game_sys::ffi_util::try_box;
use rsf_game_sys::rsf_game_render_pass;
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void};
use std::ptr::null_mut;
use std::sync::Mutex;
use windows::Win32::Graphics::Direct3D11::{D3D11_DEVICE_CONTEXT_IMMEDIATE, ID3D11DeviceContext};
use windows::core::Interface;

static REPLAY: Mutex<()> = Mutex::new(());

thread_local! {
    static SCOPE: RefCell<Option<rsf_game_render_pass>> = const { RefCell::new(None) };
    static MERGING: Cell<bool> = const { Cell::new(false) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_velocity_set_log(
    log: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    user: *mut c_void,
) {
    let _lease = lock(&REPLAY);
    set_log(log.map(|write| Log { write, user }));
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_velocity_scope(scope: *const rsf_game_render_pass) {
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
pub(crate) unsafe extern "C" fn rsf_ac7_velocity_geometry(draw: *const rsf_frame_tap_geometry) {
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
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&draw.context) }) else {
        return;
    };
    if unsafe { context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE {
        return;
    }
    let entered = clock_ms();
    let Some(pipeline) = Pipeline::inspect(context, draw.topology) else {
        return;
    };
    let Ok(device) = (unsafe { context.GetDevice() }) else {
        return;
    };
    let renderer = admit(
        &LIFETIME,
        device.as_raw(),
        || {
            Renderer::new(&device)
                .and_then(try_box)
                .map_or(null_mut(), |renderer| Box::into_raw(renderer).cast())
        },
        destroy,
        reset_for_renderer,
    );
    let Some(renderer) = (unsafe { renderer.cast::<Renderer>().as_mut() }) else {
        refused(c"device-side pass initialization failed");
        return;
    };
    let _ = renderer.draw(context, &scope, draw, pipeline, entered);
}

unsafe fn destroy(renderer: *mut c_void) {
    drop(unsafe { Box::from_raw(renderer.cast::<Renderer>()) });
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_velocity_commit(
    context: *mut c_void,
    pass: *const rsf_game_render_pass,
) -> i32 {
    let _lease = lock(&REPLAY);
    // The queued renderer lends the pass; only its leading size is read before admission.
    if pass.is_null() || unsafe { (*pass).struct_size as usize } < size_of::<rsf_game_render_pass>()
    {
        return 0;
    }
    let (Some(renderer), Some(context)) = (
        unsafe { published(&LIFETIME).cast::<Renderer>().as_ref() },
        unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) },
    ) else {
        return 0;
    };
    i32::from(renderer.commit(context, unsafe { &*pass }).is_some())
}

pub(super) fn merge(draw: impl FnOnce()) {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            MERGING.set(self.0);
        }
    }
    let _restore = Restore(MERGING.replace(true));
    draw();
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_ac7_velocity_frame_end() {
    let _lease = lock(&REPLAY);
    let renderer = published(&LIFETIME);
    let Some(owner) = (unsafe { renderer.cast::<Renderer>().as_mut() }) else {
        return;
    };
    log_frame_end(owner.count());
    velocity_history::frame_end(|retired| owner.retire(retired));
}

#[cfg(feature = "warp-fixture")]
pub(crate) fn shader_creation_count() -> u32 {
    let _lease = lock(&REPLAY);
    unsafe { published(&LIFETIME).cast::<Renderer>().as_ref() }.map_or(0, Renderer::count)
}

/// Destroys the renderer between fixture cases. The carrier never retires it before exit.
#[cfg(feature = "warp-fixture")]
pub(crate) fn rsf_ac7_velocity_release() {
    let _lease = lock(&REPLAY);
    release_renderer();
    let _ = SCOPE.replace(None);
}
