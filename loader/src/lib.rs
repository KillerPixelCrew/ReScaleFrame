// SPDX-License-Identifier: MIT
#[cfg(windows)]
mod windows;

#[cfg(windows)]
mod launcher;

#[cfg(windows)]
mod diagnostics;

#[cfg(all(windows, target_env = "msvc"))]
mod census;

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_get_bootstrap_version_impl() -> *const std::ffi::c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}
