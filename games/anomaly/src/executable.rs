// SPDX-License-Identifier: MIT
use crate::digest::sha256_file;
use crate::native_profile::{PROFILES, find};
use rsf_game_sys::win32::module_path;
use std::cell::Cell;
use std::ffi::{CStr, OsString};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::ptr;

/// Only a profiled ASCII filename is eligible for hashing.
pub(crate) fn running_build() -> Result<usize, &'static CStr> {
    let path = unsafe { module_path(ptr::null_mut()) }
        .ok_or(c"Could not read the running executable path.")?;
    let path = OsString::from_wide(path.units());
    let path = Path::new(&path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            name.is_ascii()
                && PROFILES
                    .iter()
                    .any(|build| build.executable.to_bytes().eq_ignore_ascii_case(name.as_bytes()))
        })
        .ok_or(c"Native sites belong to the profiled DX11 executables (standard or AVX), not this process.")?;
    sha256_file(path)
        .and_then(|hash| find(name.as_bytes(), &hash))
        .ok_or(c"Running executable SHA-256 does not match a guarded native profile.")
}

// The runtime executes prepare synchronously. The verified index is visible on
// this thread only while that call uses the immutable profile table.
thread_local! {
    static VERIFIED_BUILD: Cell<Option<usize>> = const { Cell::new(None) };
}

pub(crate) fn with_verified_build<T>(index: usize, prepare: impl FnOnce() -> T) -> T {
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            VERIFIED_BUILD.set(self.0);
        }
    }

    let _restore = Restore(VERIFIED_BUILD.replace(Some(index)));
    prepare()
}

pub(crate) fn observer_build() -> Result<usize, &'static CStr> {
    VERIFIED_BUILD.get().map_or_else(running_build, Ok)
}
