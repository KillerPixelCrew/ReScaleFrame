// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output_abi::{
    rsf_d3d11_output_texture_desc, rsf_d3d11_texture_desc, rsf_fullscreen_pass,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

pub(super) type CreateBlit = unsafe extern "C" fn(*mut c_void) -> *mut rsf_fullscreen_pass;
pub(super) type DrawBlit = unsafe extern "C" fn(
    *mut rsf_fullscreen_pass,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    u32,
    u32,
) -> u32;
pub(super) type DestroyBlit = unsafe extern "C" fn(*mut rsf_fullscreen_pass);

pub(super) struct FullscreenBlit {
    pass: NonNull<rsf_fullscreen_pass>,
    draw: DrawBlit,
    destroy: DestroyBlit,
}

impl FullscreenBlit {
    pub(super) unsafe fn new(
        device: &ComRef,
        create: CreateBlit,
        draw: DrawBlit,
        destroy: DestroyBlit,
    ) -> Option<Self> {
        let pass = NonNull::new(unsafe { create(device.as_ptr()) })?;
        Some(Self {
            pass,
            draw,
            destroy,
        })
    }

    pub(super) unsafe fn draw(
        &self,
        context: *mut c_void,
        target: *mut c_void,
        source: *mut c_void,
        width: u32,
        height: u32,
    ) -> bool {
        unsafe { (self.draw)(self.pass.as_ptr(), context, target, source, width, height) != 0 }
    }
}

impl Drop for FullscreenBlit {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.pass.as_ptr()) };
    }
}

pub(super) unsafe fn texture_desc(texture: *mut c_void) -> rsf_d3d11_texture_desc {
    let mut desc = rsf_d3d11_texture_desc::default();
    unsafe { rsf_d3d11_output_texture_desc(texture, &mut desc) };
    desc
}

pub(super) struct GraphicsCache<T>(Mutex<Option<T>>);

impl<T> GraphicsCache<T> {
    pub(super) const fn new(state: T) -> Self {
        Self(Mutex::new(Some(state)))
    }

    pub(super) fn with<R>(&self, run: impl FnOnce(&mut T) -> R) -> Option<R> {
        // The supplied stream belongs to one graphics execution owner. During
        // that owner's operation, refuse reentry without holding a driver-call lock.
        let mut state = self.0.lock().ok()?.take()?;
        let result = run(&mut state);
        *self.0.lock().ok()? = Some(state);
        Some(result)
    }
}
