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
// Transparent, so Option<ComRef> is a nullable interface pointer in C structures.
#[repr(transparent)]
pub(super) struct ComRef(pub(super) NonNull<c_void>);

impl ComRef {
    pub(super) unsafe fn from_owned(object: *mut c_void) -> Option<Self> {
        NonNull::new(object).map(Self)
    }

    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    pub(super) fn into_raw(self) -> *mut c_void {
        let raw = self.as_ptr();
        std::mem::forget(self);
        raw
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
        query_interface:
            unsafe extern "system" fn(*mut Fake, *const c_void, *mut *mut c_void) -> i32,
        add_ref: unsafe extern "system" fn(*mut Fake) -> u32,
        release: unsafe extern "system" fn(*mut Fake) -> u32,
    }

    #[repr(C)]
    pub(crate) struct Fake {
        vtable: &'static Vtable,
        pub(crate) refs: Cell<u32>,
        last_release: Option<unsafe fn(*mut Fake)>,
    }

    pub(crate) unsafe extern "system" fn add_ref(object: *mut Fake) -> u32 {
        let refs = unsafe { &(*object).refs };
        refs.set(refs.get() + 1);
        refs.get()
    }

    unsafe extern "system" fn query_interface(
        _: *mut Fake,
        _: *const c_void,
        out: *mut *mut c_void,
    ) -> i32 {
        if !out.is_null() {
            unsafe { out.write(ptr::null_mut()) };
        }
        0x8000_4002_u32 as i32
    }

    pub(crate) unsafe extern "system" fn release(object: *mut Fake) -> u32 {
        let refs = unsafe { &(*object).refs };
        let count = refs
            .get()
            .checked_sub(1)
            .expect("over-released fake COM object");
        refs.set(count);
        if count == 0
            && let Some(released) = unsafe { (*object).last_release }
        {
            unsafe { released(object) };
        }
        count
    }

    static VTABLE: Vtable = Vtable {
        query_interface,
        add_ref,
        release,
    };

    impl Fake {
        pub(crate) fn new() -> Self {
            Self::with_last_release(None)
        }

        pub(crate) fn with_last_release(last_release: Option<unsafe fn(*mut Self)>) -> Self {
            Self {
                vtable: &VTABLE,
                refs: Cell::new(1),
                last_release,
            }
        }
    }

    pub(crate) fn fake() -> Box<Fake> {
        Box::new(Fake::new())
    }

    pub(crate) fn raw(object: &Fake) -> *mut c_void {
        ptr::from_ref(object).cast_mut().cast()
    }
}
