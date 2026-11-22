// SPDX-License-Identifier: MIT
//! The translucent velocity pass's per-device renderer lifetime: which renderer is published and
//! for which D3D11 device, admission of a draw's device, replacement when the device changes,
//! the history, variant and profile resets that follow a publication, and teardown at release.
//! The replay lock spans admission through the final use of the D3D11 variants and history resources.

use crate::{velocity_commit, velocity_history, velocity_pass_cache};
use std::ffi::c_void;
#[cfg(test)]
use std::mem::offset_of;
use std::ptr::{NonNull, null_mut};
use std::sync::{LazyLock, Mutex, MutexGuard};

#[cfg(windows)]
mod d3d11;
#[cfg(windows)]
pub(crate) mod replay;

/// Creates and initializes a renderer for the device, or returns null. Must not throw.
#[cfg(test)]
type CreateRenderer = unsafe extern "C" fn(device: *mut c_void) -> *mut c_void;
/// Destroys a renderer the create leaf made. Must not throw.
type DestroyRenderer = unsafe extern "C" fn(renderer: *mut c_void);

/// Synthetic admission fixture leaves.
#[cfg(test)]
#[repr(C)]
struct RendererLeaves {
    create: Option<CreateRenderer>,
    destroy: Option<DestroyRenderer>,
}

#[cfg(test)]
const _: () = {
    assert!(size_of::<RendererLeaves>() == 16 && offset_of!(RendererLeaves, destroy) == 8);
};

/// A native renderer and the leaf that destroys it.
struct Owned {
    renderer: NonNull<c_void>,
    destroy: DestroyRenderer,
}

// SAFETY: The renderer is only an address here. The native pass uses it on its draw, commit and
// frame-end callbacks and this owner destroys it through its leaf; neither needs the thread that
// made it.
unsafe impl Send for Owned {}

impl Owned {
    fn destroy(self) {
        // SAFETY: The create leaf made this renderer and nothing published it again since.
        unsafe { (self.destroy)(self.renderer.as_ptr()) };
    }
}

struct Published {
    owned: Owned,
    // Identity only; the renderer holds its own reference to the device.
    device: usize,
}

#[derive(Default)]
struct Lifetime {
    published: Option<Published>,
}

impl Lifetime {
    fn for_device(&self, device: usize) -> Option<NonNull<c_void>> {
        self.published
            .as_ref()
            .filter(|published| published.device == device)
            .map(|published| published.owned.renderer)
    }
}

static LIFETIME: LazyLock<Mutex<Lifetime>> = LazyLock::new(Mutex::default);

fn lock<T>(state: &Mutex<T>) -> MutexGuard<'_, T> {
    state.lock().unwrap_or_else(|error| error.into_inner())
}

/// The renderer for the draw's device. A different device gets a new renderer: once it is made,
/// it replaces and destroys the published one and `on_publish` runs. A failed creation leaves the
/// published renderer in place and returns null.
fn admit(
    state: &Mutex<Lifetime>,
    device: *mut c_void,
    create: impl FnOnce() -> *mut c_void,
    destroy: DestroyRenderer,
    on_publish: impl FnOnce(),
) -> *mut c_void {
    if device.is_null() {
        return null_mut();
    }
    let device = device.addr();
    let existing = lock(state).for_device(device);
    if let Some(renderer) = existing {
        return renderer.as_ptr();
    }
    // Creation compiles HLSL and creates D3D11 objects whose creation callbacks report back into
    // the source cache, so it runs without the lock.
    let Some(renderer) = NonNull::new(create()) else {
        return null_mut();
    };
    let made = Owned { renderer, destroy };
    let replaced = {
        let mut lifetime = lock(state);
        // Another callback published one for this device while this one was being made.
        let current = lifetime.for_device(device);
        if let Some(current) = current {
            drop(lifetime);
            made.destroy();
            return current.as_ptr();
        }
        lifetime.published.replace(Published {
            owned: made,
            device,
        })
    };
    // As before, the old renderer is destroyed before the resets.
    if let Some(old) = replaced {
        old.owned.destroy();
    }
    on_publish();
    renderer.as_ptr()
}

/// Destroys the published renderer, then runs `released`.
fn release(state: &Mutex<Lifetime>, released: impl FnOnce()) {
    let old = lock(state).published.take();
    if let Some(old) = old {
        old.owned.destroy();
    }
    released();
}

/// The published renderer, or null.
fn published(state: &Mutex<Lifetime>) -> *mut c_void {
    lock(state)
        .published
        .as_ref()
        .map_or(null_mut(), |published| published.owned.renderer.as_ptr())
}

/// The published renderer when it was made for this device, or null.
#[cfg(test)]
fn published_for(state: &Mutex<Lifetime>, device: *mut c_void) -> *mut c_void {
    if device.is_null() {
        return null_mut();
    }
    lock(state)
        .for_device(device.addr())
        .map_or(null_mut(), NonNull::as_ptr)
}

/// A new renderer starts without variants, model or view history, or profile.
fn reset_for_renderer() {
    velocity_pass_cache::reset_variants();
    velocity_history::reset();
    velocity_commit::reset_profile();
}

/// The renderer for the draw context's device, made and published when the device differs from
/// the published one. Null when the device is null, the leaves are incomplete or creation failed.
#[cfg(test)]
unsafe fn rsf_ac7_velocity_renderer_admit(
    device: *mut c_void,
    leaves: *const RendererLeaves,
) -> *mut c_void {
    // SAFETY: The draw callback lends its leaves for this call.
    let Some(leaves) = (unsafe { leaves.as_ref() }) else {
        return null_mut();
    };
    let (Some(create), Some(destroy)) = (leaves.create, leaves.destroy) else {
        return null_mut();
    };
    admit(
        &LIFETIME,
        device,
        // SAFETY: The leaf receives the draw context's device for this call.
        || unsafe { create(device) },
        destroy,
        reset_for_renderer,
    )
}

/// Destroys the published renderer and forgets the observed sources, layouts, variants, history
/// and profile.
fn release_renderer() {
    release(&LIFETIME, || {
        velocity_pass_cache::release();
        velocity_history::reset();
        velocity_commit::reset_profile();
    });
}

#[cfg(test)]
mod tests;
