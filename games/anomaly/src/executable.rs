// SPDX-License-Identifier: MIT
use crate::{BuildIdentity, digest::sha256_file, match_build};
use std::ffi::{CStr, OsString, c_char};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::{ptr, slice};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;

const PATH_CAPACITY: usize = 32768;

/// The module path with its trailing NUL. A null module selects the executable.
pub(crate) fn module_path(module: HMODULE) -> Option<Vec<u16>> {
    let mut path = vec![0u16; PATH_CAPACITY];
    let length =
        unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), PATH_CAPACITY as u32) } as usize;
    if length == 0 || length >= PATH_CAPACITY {
        return None;
    }
    path.truncate(length + 1);
    Some(path)
}

pub(crate) fn identity_name(identity: &BuildIdentity) -> &CStr {
    // The native profile table supplies immutable, NUL-terminated strings.
    unsafe { CStr::from_ptr(identity.executable) }
}

/// Only a profiled ASCII filename is eligible for hashing.
pub(crate) fn running_build(builds: &[BuildIdentity]) -> Result<usize, &'static CStr> {
    let path =
        module_path(ptr::null_mut()).ok_or(c"Could not read the running executable path.")?;
    let path = OsString::from_wide(&path[..path.len() - 1]);
    let path = Path::new(&path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            name.is_ascii()
                && builds
                    .iter()
                    .any(|build| identity_name(build).to_bytes().eq_ignore_ascii_case(name.as_bytes()))
        })
        .ok_or(c"Native sites belong to the profiled DX11 executables (standard or AVX), not this process.")?;
    sha256_file(path)
        .and_then(|hash| match_build(builds, name.as_bytes(), &hash))
        .ok_or(c"Running executable SHA-256 does not match a guarded native profile.")
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_running_build(
    builds: *const BuildIdentity,
    count: usize,
    index: &mut usize,
) -> *const c_char {
    // The native observer lends the same immutable table as the cold entry.
    let builds = unsafe { slice::from_raw_parts(builds, count) };
    match running_build(builds) {
        Ok(build) => {
            *index = build;
            ptr::null()
        }
        Err(reason) => reason.as_ptr(),
    }
}
