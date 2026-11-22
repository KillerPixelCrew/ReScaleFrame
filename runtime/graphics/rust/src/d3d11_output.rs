// SPDX-License-Identifier: MIT
//! D3D11 binding snapshots, texture descriptions, device identity and the per-owner cache.
use crate::com_ref::ComRef;
use crate::sys::{
    rsf_d3d11_output_child_device, rsf_d3d11_output_texture_desc, rsf_d3d11_state,
    rsf_d3d11_texture_desc,
};
use std::ffi::c_void;
use std::sync::Mutex;

pub type SaveBindings = unsafe extern "C" fn(*mut c_void, *mut rsf_d3d11_state) -> u32;
pub type RestoreBindings = unsafe extern "C" fn(*mut c_void, *mut rsf_d3d11_state);

pub struct SavedBindings {
    context: *mut c_void,
    saved: rsf_d3d11_state,
    restore: RestoreBindings,
}

impl SavedBindings {
    /// # Safety
    ///
    /// `context` must be a live `ID3D11DeviceContext` that outlives the snapshot, and `save` and
    /// `restore` must accept it.
    pub unsafe fn new(
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

/// # Safety
///
/// `texture` is null or a live `ID3D11Texture2D`.
pub unsafe fn texture_desc(texture: *mut c_void) -> rsf_d3d11_texture_desc {
    let mut desc = rsf_d3d11_texture_desc::default();
    unsafe { rsf_d3d11_output_texture_desc(texture, &mut desc) };
    desc
}

/// # Safety
///
/// A non-null `child` must be a live `ID3D11DeviceChild` interface.
pub unsafe fn same_device(device: *mut c_void, child: *mut c_void) -> bool {
    !child.is_null()
        && unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(child)) }
            .is_some_and(|owner| owner.as_ptr() == device)
}

pub struct GraphicsCache<T>(Mutex<Option<T>>);

impl<T> GraphicsCache<T> {
    pub const fn new(state: T) -> Self {
        Self(Mutex::new(Some(state)))
    }

    pub fn with<R>(&self, run: impl FnOnce(&mut T) -> R) -> Option<R> {
        // The supplied stream belongs to one graphics execution owner. During
        // that owner's operation, refuse reentry without holding a driver-call lock.
        let mut state = self.0.lock().ok()?.take()?;
        let result = run(&mut state);
        *self.0.lock().ok()? = Some(state);
        Some(result)
    }
}
