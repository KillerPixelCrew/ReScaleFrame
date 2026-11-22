// SPDX-License-Identifier: MIT
#![cfg(windows)]

mod contract;
mod domains;
mod session;

use std::ffi::{CStr, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;
const NATIVE_REFUSED: i32 = -5;

fn wide(path: &OsStr) -> Vec<u16> {
    path.encode_wide().chain([0]).collect()
}

struct Module(HMODULE);

impl Module {
    fn beside_executable(leaf: &Path) -> Self {
        let mut path = std::env::current_exe().expect("fixture executable path");
        path.pop();
        path.push(leaf);
        let handle = unsafe {
            LoadLibraryExW(
                wide(path.as_os_str()).as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        };
        assert!(
            !handle.is_null(),
            "Loading {} failed: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
        Self(handle)
    }

    /// F must be the export's exact ABI and signature.
    unsafe fn export<F: Copy>(&self, name: &CStr) -> Option<F> {
        unsafe { rsf_game_sys::win32::typed_export(self.0, name) }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

fn run(check: impl FnOnce() -> i32 + std::panic::UnwindSafe) -> i32 {
    std::panic::catch_unwind(check).unwrap_or(1)
}
