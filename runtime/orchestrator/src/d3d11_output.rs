// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output_abi::{
    rsf_d3d11_output_child_device, rsf_d3d11_output_texture_desc, rsf_d3d11_state,
    rsf_d3d11_texture_desc, rsf_fullscreen_pass,
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

pub(super) type SaveBindings = unsafe extern "C" fn(*mut c_void, *mut rsf_d3d11_state) -> u32;
pub(super) type RestoreBindings = unsafe extern "C" fn(*mut c_void, *mut rsf_d3d11_state);

pub(super) struct SavedBindings {
    context: *mut c_void,
    saved: rsf_d3d11_state,
    restore: RestoreBindings,
}

impl SavedBindings {
    pub(super) unsafe fn new(
        context: *mut c_void,
        save: SaveBindings,
        restore: RestoreBindings,
    ) -> Option<Self> {
        let mut saved = rsf_d3d11_state::default();
        if unsafe { save(context, &mut saved) } == 0 {
            return None;
        }
        Some(Self {
            context,
            saved,
            restore,
        })
    }
}

impl Drop for SavedBindings {
    fn drop(&mut self) {
        unsafe { (self.restore)(self.context, &mut self.saved) };
    }
}

pub(super) unsafe fn texture_desc(texture: *mut c_void) -> rsf_d3d11_texture_desc {
    let mut desc = rsf_d3d11_texture_desc::default();
    unsafe { rsf_d3d11_output_texture_desc(texture, &mut desc) };
    desc
}

// A non-null child must be a live ID3D11DeviceChild interface.
pub(super) unsafe fn same_device(device: *mut c_void, child: *mut c_void) -> bool {
    !child.is_null()
        && unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(child)) }
            .is_some_and(|owner| owner.as_ptr() == device)
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
