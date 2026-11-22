// SPDX-License-Identifier: MIT
use core::ffi::c_void;
use windows::{
    Win32::{
        Foundation::{CloseHandle, FARPROC, FreeLibrary, HANDLE, HMODULE},
        System::LibraryLoader::{
            GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
            LoadLibraryExW,
        },
    },
    core::{IUnknown, Interface, PCSTR, PCWSTR, Result},
};

/// The validated path is NUL-terminated and readable for this call. The
/// receiving capsule owns the module reference even if export admission fails.
pub(super) unsafe fn load(path: *const u16) -> Result<HMODULE> {
    unsafe {
        LoadLibraryExW(
            PCWSTR(path),
            None,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    }
}

pub(super) unsafe fn export(module: HMODULE, name: &core::ffi::CStr) -> FARPROC {
    unsafe { GetProcAddress(module, PCSTR(name.as_ptr().cast())) }
}

/// Release the capsule's transferred COM references in the supplied order,
/// then the event and library. The caller has already retired SDK consumers.
pub(super) unsafe fn release(objects: &[*mut c_void], event: *mut c_void, module: *mut c_void) {
    for &object in objects {
        if !object.is_null() {
            drop(unsafe { IUnknown::from_raw(object) });
        }
    }
    if !event.is_null() {
        let _ = unsafe { CloseHandle(HANDLE(event)) };
    }
    if !module.is_null() {
        let _ = unsafe { FreeLibrary(HMODULE(module)) };
    }
}
