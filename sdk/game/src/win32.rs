// SPDX-License-Identifier: MIT
//! Windows helpers for GameSDK adapters that borrow live modules.

use std::ffi::CStr;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetProcAddress};

const MODULE_PATH_CAPACITY: usize = 32768;

/// An owned Windows module path with one trailing NUL.
pub struct ModulePath(Vec<u16>);

impl ModulePath {
    pub fn units(&self) -> &[u16] {
        &self.0[..self.0.len() - 1]
    }

    pub fn terminated(&self) -> &[u16] {
        &self.0
    }
}

/// Reads a path into caller-supplied storage and returns its units without the NUL.
/// A null module selects the process executable.
///
/// # Safety
/// A non-null module must be a valid module handle kept loaded for this call.
pub unsafe fn module_path_in(module: HMODULE, buffer: &mut [u16]) -> Option<&[u16]> {
    let capacity = buffer.len().min(MODULE_PATH_CAPACITY);
    if capacity == 0 {
        return None;
    }
    let length =
        unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), capacity as u32) } as usize;
    (length != 0 && length < capacity).then(|| &buffer[..length])
}

/// Reads a path with room for the maximum Windows path and rejects truncation.
///
/// # Safety
/// A non-null module must be a valid module handle kept loaded for this call.
pub unsafe fn module_path(module: HMODULE) -> Option<ModulePath> {
    let mut path = vec![0; MODULE_PATH_CAPACITY];
    let length = unsafe { module_path_in(module, &mut path) }?.len();
    path.truncate(length + 1);
    path[length] = 0;
    Some(ModulePath(path))
}

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
