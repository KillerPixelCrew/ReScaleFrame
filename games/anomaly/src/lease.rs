// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use std::ptr::NonNull;

pub(crate) struct Lease {
    handle: NonNull<c_void>,
    release: unsafe extern "C" fn(*mut c_void),
}

impl Lease {
    pub(crate) fn new(
        handle: *mut c_void,
        release: unsafe extern "C" fn(*mut c_void),
    ) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self { handle, release })
    }

    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.handle.as_ptr()
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        unsafe { (self.release)(self.as_ptr()) };
    }
}
