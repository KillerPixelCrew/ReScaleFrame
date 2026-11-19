// SPDX-License-Identifier: MIT
use crate::d3d11_output_abi::{rsf_d3d11_output_texture_desc, rsf_d3d11_texture_desc};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

unsafe extern "C" {
    // The existing helpers use the IUnknown prefix for all COM interfaces.
    fn rsf_fg_lease_com_add_ref(object: *mut c_void);
    fn rsf_fg_lease_com_release(object: *mut c_void);
}

pub(super) struct ComRef(NonNull<c_void>);

// References can be retired during shared runtime teardown. Calls that record
// D3D11 commands are still restricted to the graphics execution owner.
unsafe impl Send for ComRef {}

impl ComRef {
    // The caller transfers one owned reference if the pointer is non-null.
    pub(super) unsafe fn from_owned(object: *mut c_void) -> Option<Self> {
        NonNull::new(object).map(Self)
    }

    pub(super) unsafe fn retain(object: NonNull<c_void>) -> Self {
        unsafe { rsf_fg_lease_com_add_ref(object.as_ptr()) };
        Self(object)
    }

    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for ComRef {
    fn drop(&mut self) {
        unsafe { rsf_fg_lease_com_release(self.as_ptr()) };
    }
}

impl Clone for ComRef {
    fn clone(&self) -> Self {
        unsafe { Self::retain(self.0) }
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
