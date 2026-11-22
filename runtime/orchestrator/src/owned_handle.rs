// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use std::ptr::NonNull;

/// Owns an opaque platform handle and releases it with the supplied destructor.
pub(super) struct OwnedHandle {
    handle: NonNull<c_void>,
    release: unsafe extern "C" fn(*mut c_void),
}

impl OwnedHandle {
    pub(super) fn new(
        handle: *mut c_void,
        release: unsafe extern "C" fn(*mut c_void),
    ) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self { handle, release })
    }

    pub(super) fn get(&self) -> *mut c_void {
        self.handle.as_ptr()
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { (self.release)(self.get()) };
    }
}
