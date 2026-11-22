// SPDX-License-Identifier: MIT
//! Windows helpers for GameSDK adapters that borrow live modules.

use std::ffi::CStr;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;

/// Resolves a function export from a borrowed module.
///
/// # Safety
/// The module must remain loaded through lookup and every use of the result.
/// F must be the function pointer with the exact ABI and signature of name.
pub unsafe fn typed_export<F: Copy>(module: HMODULE, name: &CStr) -> Option<F> {
    type Export = unsafe extern "system" fn() -> isize;
    const { assert!(size_of::<F>() == size_of::<Export>()) };
    if module.is_null() {
        return None;
    }
    unsafe { GetProcAddress(module, name.as_ptr().cast()) }
        .map(|address| unsafe { std::mem::transmute_copy::<Export, F>(&address) })
}
