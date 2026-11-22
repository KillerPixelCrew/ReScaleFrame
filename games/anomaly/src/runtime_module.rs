// SPDX-License-Identifier: MIT
//! Windows module leases and by-name access to the Runtime already loaded by Anomaly's carrier.
use std::ffi::{CStr, c_void};
use std::ptr::{NonNull, null_mut};
use windows_sys::Win32::Foundation::{FARPROC, FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleExW, GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32,
    LoadLibraryExW,
};
use windows_sys::core::w;

pub(crate) struct Module(NonNull<c_void>);

// A counted Windows module reference can be released from another thread.
unsafe impl Send for Module {}

impl Module {
    /// Adopt one reference transferred by a successful Windows loader call.
    pub(crate) unsafe fn from_owned(handle: HMODULE) -> Option<Self> {
        NonNull::new(handle).map(Self)
    }

    pub(crate) unsafe fn loaded(name: *const u16) -> Option<Self> {
        let mut handle = null_mut();
        // Lookup the named module and acquire its reference in the same call.
        if unsafe { GetModuleHandleExW(0, name, &mut handle) } == 0 {
            return None;
        }
        unsafe { Self::from_owned(handle) }
    }

    pub(crate) unsafe fn system(name: *const u16) -> Option<Self> {
        let handle = unsafe { LoadLibraryExW(name, null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        unsafe { Self::from_owned(handle) }
    }

    pub(crate) fn symbol(&self, name: &CStr) -> FARPROC {
        unsafe { GetProcAddress(self.0.as_ptr(), name.as_ptr().cast()) }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0.as_ptr()) };
    }
}

pub(crate) extern "C" fn rsf_anomaly_runtime_module() -> HMODULE {
    // The carrier keeps this module mapped until the adapter retires.
    unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) }
}

/// For the NUL-terminated byte-string export names bindgen emits from the Runtime headers.
/// The module must be the carrier's borrowed handle, and F must be the named C export's signature.
pub(crate) unsafe fn export<F: Copy>(module: HMODULE, name: &[u8]) -> Option<F> {
    let name = CStr::from_bytes_with_nul(name).ok()?;
    unsafe { rsf_game_sys::win32::typed_export(module, name) }
}

pub(crate) extern "C" fn rsf_anomaly_runtime_set_colour_transport(transport: bool) {
    let runtime = rsf_anomaly_runtime_module();
    let set = unsafe {
        rsf_game_sys::win32::typed_export::<unsafe extern "C" fn(u32)>(
            runtime,
            c"rsf_dlss_pipeline_set_colour_transport",
        )
    };
    if let Some(set) = set {
        unsafe { set(u32::from(transport)) };
    }
}
