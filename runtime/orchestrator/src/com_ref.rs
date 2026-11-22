// SPDX-License-Identifier: MIT
use std::borrow::Borrow;
use std::ffi::c_void;
use std::ptr::NonNull;

use windows::core::{HRESULT, IUnknown_Vtbl, Interface};

/// Borrow the generated vtable for a live interface of type `I`.
pub(super) unsafe fn vtable<I: Interface>(object: &*mut c_void) -> &I::Vtable {
    unsafe { I::from_raw_borrowed(object) }
        .expect("live COM interface")
        .vtable()
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

    // Every non-null slot transfers one reference, including partial results.
    pub(super) unsafe fn adopt_all<const N: usize>(objects: [*mut c_void; N]) -> Option<[Self; N]> {
        let resources = objects.map(|object| unsafe { Self::from_owned(object) });
        if resources.iter().any(Option::is_none) {
            return None;
        }
        Some(resources.map(Option::unwrap))
    }

    pub(super) unsafe fn from_result(status: HRESULT, object: *mut c_void) -> Option<Self> {
        let owned = unsafe { Self::from_owned(object) };
        if status.is_ok() { owned } else { None }
    }

    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    pub(super) fn into_raw(self) -> *mut c_void {
        let raw = self.as_ptr();
        std::mem::forget(self);
        raw
    }

    unsafe fn unknown_vtable(raw: *mut c_void) -> *const IUnknown_Vtbl {
        unsafe { *raw.cast::<*const IUnknown_Vtbl>() }
    }

    pub(super) unsafe fn retain(object: *mut c_void) -> Self {
        let object = NonNull::new(object).expect("live COM interface");
        let raw = object.as_ptr();
        // Read only the invoked slot. Test doubles need not implement QueryInterface.
        unsafe { ((*Self::unknown_vtable(raw)).AddRef)(raw) };
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
        let raw = self.as_ptr();
        unsafe { ((*Self::unknown_vtable(raw)).Release)(raw) };
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

    #[test]
    fn partial_adoption_releases_every_returned_reference() {
        use super::ComRef;
        let (first, last) = (fake(), fake());
        let result = unsafe {
            ComRef::adopt_all([
                ComRef::retain(raw(&first)).into_raw(),
                ptr::null_mut(),
                ComRef::retain(raw(&last)).into_raw(),
            ])
        };
        assert!(result.is_none());
        assert_eq!(first.refs.get(), 1);
        assert_eq!(last.refs.get(), 1);
    }
}

// C callers own these leases explicitly. Forget the Rust owner on retain and adopt
// that same reference on release. Both entry points accept an absent resource.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_resource_retain(resource: *mut c_void) {
    if !resource.is_null() {
        unsafe { ComRef::retain(resource) }.into_raw();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_resource_release(resource: *mut c_void) {
    drop(unsafe { ComRef::from_owned(resource) });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_create_render_target_view(
    device: *mut c_void,
    texture: *mut c_void,
) -> *mut c_void {
    if device.is_null() || texture.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { crate::d3d11_output_abi::rsf_d3d11_output_create_rtv(device, texture) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_swapchain_back_buffer(swapchain: *mut c_void) -> *mut c_void {
    use windows::Win32::Graphics::{Direct3D11::ID3D11Texture2D, Dxgi::IDXGISwapChain};

    if swapchain.is_null() {
        return std::ptr::null_mut();
    }
    let mut buffer = std::ptr::null_mut();
    let status = unsafe {
        (vtable::<IDXGISwapChain>(&swapchain).GetBuffer)(
            swapchain,
            0,
            &ID3D11Texture2D::IID,
            &mut buffer,
        )
    };
    unsafe { ComRef::from_result(status, buffer) }.map_or(std::ptr::null_mut(), ComRef::into_raw)
}
