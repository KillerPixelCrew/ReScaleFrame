// SPDX-License-Identifier: MIT
use crate::d3d11_output_abi::{rsf_d3d11_output_texture_desc, rsf_d3d11_texture_desc};
use std::ffi::c_void;
use std::sync::Mutex;

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
