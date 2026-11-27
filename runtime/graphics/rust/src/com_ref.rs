// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use windows::core::{HRESULT, IUnknown, Interface};

/// Borrow the generated vtable for a live interface of type `I`.
///
/// # Safety
///
/// `object` must hold a live COM interface of type `I`.
pub unsafe fn vtable<'a, I: Interface + 'a>(object: &'a *mut c_void) -> &'a I::Vtable {
    unsafe { I::from_raw_borrowed(object) }
        .expect("live COM interface")
        .vtable()
}

// COM references are windows-crate interfaces: `IUnknown` owns one reference, clones with AddRef
// and releases on drop, reading only those vtable slots, so test doubles need no QueryInterface.
// It is a transparent non-null pointer, so `Option<IUnknown>` is a nullable pointer in C records.
// These take raw pointers that C callers and the Windows bindings hand over.

/// # Safety
///
/// `object` is null or a live COM interface whose reference the caller transfers.
pub unsafe fn owned(object: *mut c_void) -> Option<IUnknown> {
    (!object.is_null()).then(|| unsafe { IUnknown::from_raw(object) })
}

/// # Safety
///
/// `object` must be a live COM interface; the result holds one more reference.
pub unsafe fn retain(object: *mut c_void) -> IUnknown {
    unsafe { IUnknown::from_raw_borrowed(&object) }
        .expect("live COM interface")
        .clone()
}

/// # Safety
///
/// `object` is null or a live COM interface.
pub unsafe fn identity(object: *mut c_void) -> Option<IUnknown> {
    unsafe { IUnknown::from_raw_borrowed(&object) }?.cast().ok()
}

/// Every non-null slot transfers one reference, including partial results.
///
/// # Safety
///
/// Each slot is null or a live COM interface whose reference the caller transfers.
pub unsafe fn adopt_all<const N: usize>(objects: [*mut c_void; N]) -> Option<[IUnknown; N]> {
    let resources = objects.map(|object| unsafe { owned(object) });
    if resources.iter().any(Option::is_none) {
        return None;
    }
    Some(resources.map(Option::unwrap))
}

/// An output reference belongs to the caller even if the COM method reports failure.
///
/// # Safety
///
/// `object` is null or a live COM interface whose reference the caller transfers.
pub unsafe fn from_result(status: HRESULT, object: *mut c_void) -> Option<IUnknown> {
    let owned = unsafe { owned(object) };
    if status.is_ok() { owned } else { None }
}
// The AC7 carrier is C and drops the back buffer reference it was handed here.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_resource_release(resource: *mut c_void) {
    drop(unsafe { owned(resource) });
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
    unsafe { from_result(status, buffer) }.map_or(std::ptr::null_mut(), Interface::into_raw)
}

// Fake COM objects for unit tests here and in the Runtime (its dev-dependency enables the feature).
#[cfg(any(test, feature = "test-support"))]
pub mod test {
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
    pub struct Fake {
        vtable: &'static Vtable,
        pub refs: Cell<u32>,
        last_release: Option<unsafe fn(*mut Fake)>,
    }

    /// # Safety
    ///
    /// `object` must point to a live `Fake`.
    pub unsafe extern "system" fn add_ref(object: *mut Fake) -> u32 {
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

    /// # Safety
    ///
    /// `object` must point to a live `Fake`.
    pub unsafe extern "system" fn release(object: *mut Fake) -> u32 {
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

    // Each fake is built explicitly with its release hook; a Default would hide that choice.
    #[allow(clippy::new_without_default)]
    impl Fake {
        pub fn new() -> Self {
            Self::with_last_release(None)
        }

        pub fn with_last_release(last_release: Option<unsafe fn(*mut Self)>) -> Self {
            Self {
                vtable: &VTABLE,
                refs: Cell::new(1),
                last_release,
            }
        }
    }

    pub fn fake() -> Box<Fake> {
        Box::new(Fake::new())
    }

    pub fn raw(object: &Fake) -> *mut c_void {
        ptr::from_ref(object).cast_mut().cast()
    }

    #[test]
    fn partial_adoption_releases_every_returned_reference() {
        use windows::core::Interface;
        let (first, last) = (fake(), fake());
        let result = unsafe {
            super::adopt_all([
                super::retain(raw(&first)).into_raw(),
                ptr::null_mut(),
                super::retain(raw(&last)).into_raw(),
            ])
        };
        assert!(result.is_none());
        assert_eq!(first.refs.get(), 1);
        assert_eq!(last.refs.get(), 1);
    }
}
