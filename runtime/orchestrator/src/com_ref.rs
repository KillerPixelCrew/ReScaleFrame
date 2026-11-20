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

#[cfg(test)]
pub(crate) mod test {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::ptr;

    #[repr(C)]
    struct Vtable {
        query_interface: usize,
        add_ref: unsafe extern "system" fn(*mut Fake) -> u32,
        release: unsafe extern "system" fn(*mut Fake) -> u32,
    }

    #[repr(C)]
    pub(crate) struct Fake {
        vtable: &'static Vtable,
        pub(crate) refs: Cell<u32>,
    }

    pub(crate) unsafe extern "system" fn add_ref(object: *mut Fake) -> u32 {
        let refs = unsafe { &(*object).refs };
        refs.set(refs.get() + 1);
        refs.get()
    }

    unsafe extern "system" fn release(object: *mut Fake) -> u32 {
        let refs = unsafe { &(*object).refs };
        refs.set(refs.get() - 1);
        refs.get()
    }

    static VTABLE: Vtable = Vtable {
        query_interface: 0,
        add_ref,
        release,
    };

    pub(crate) fn fake() -> Box<Fake> {
        Box::new(Fake {
            vtable: &VTABLE,
            refs: Cell::new(1),
        })
    }

    pub(crate) fn raw(object: &Fake) -> *mut c_void {
        ptr::from_ref(object).cast_mut().cast()
    }
}
