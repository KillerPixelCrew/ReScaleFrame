// SPDX-License-Identifier: MIT
//! By-name access to the Runtime already loaded by Anomaly's carrier.
use std::ffi::{CStr, c_char};
use windows_sys::Win32::Foundation::{FARPROC, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::core::w;

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_runtime_module() -> HMODULE {
    // The carrier keeps this module mapped until the adapter retires.
    unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_runtime_symbol(module: HMODULE, name: *const c_char) -> FARPROC {
    if module.is_null() || name.is_null() {
        return None;
    }
    unsafe { GetProcAddress(module, name.cast()) }
}

/// The module must be the carrier's borrowed handle, and F must be the named C export's signature.
pub(crate) unsafe fn export<F: Copy>(module: HMODULE, name: &[u8]) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    let name = CStr::from_bytes_with_nul(name).ok()?;
    let function = unsafe { rsf_anomaly_runtime_symbol(module, name.as_ptr()) }?;
    Some(unsafe { std::mem::transmute_copy(&function) })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_runtime_set_colour_transport(transport: bool) {
    let runtime = rsf_anomaly_runtime_module();
    let set = unsafe {
        export::<unsafe extern "C" fn(u32)>(
            runtime,
            c"rsf_dlss_pipeline_set_colour_transport".to_bytes_with_nul(),
        )
    };
    if let Some(set) = set {
        unsafe { set(u32::from(transport)) };
    }
}
