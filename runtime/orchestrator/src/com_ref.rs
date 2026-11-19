// SPDX-License-Identifier: MIT
use std::borrow::Borrow;
use std::ffi::c_void;
use std::ptr::NonNull;

unsafe extern "C" {
    fn rsf_com_add_ref(object: *mut c_void);
    fn rsf_com_release(object: *mut c_void);
}

// Owns one reference to a live COM interface. Returned COM getters transfer
// a retained reference to this type without incrementing it again.
pub(super) struct ComRef(pub(super) NonNull<c_void>);

impl ComRef {
    pub(super) unsafe fn from_owned(object: *mut c_void) -> Option<Self> {
        NonNull::new(object).map(Self)
    }

    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    pub(super) unsafe fn retain(object: *mut c_void) -> Self {
        let object = NonNull::new(object).expect("live COM interface");
        unsafe { rsf_com_add_ref(object.as_ptr()) };
        Self(object)
    }
}

impl Clone for ComRef {
    fn clone(&self) -> Self {
        unsafe { Self::retain(self.0.as_ptr()) }
    }
}

impl Drop for ComRef {
    fn drop(&mut self) {
        unsafe { rsf_com_release(self.0.as_ptr()) };
    }
}

impl Borrow<NonNull<c_void>> for ComRef {
    fn borrow(&self) -> &NonNull<c_void> {
        &self.0
    }
}
