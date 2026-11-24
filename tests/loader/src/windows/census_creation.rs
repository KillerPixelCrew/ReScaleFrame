// SPDX-License-Identifier: MIT
use super::named;
use crate::sys::{RSF_CENSUS_ABI_VERSION, RSF_CENSUS_FLAG_NO_STATIC_SCAN, rsf_census_options};
use rsf_test_support::wide_os;
use std::ffi::{c_char, c_void};
use std::mem::transmute;
use windows_sys::Win32::{Foundation::FARPROC, System::LibraryLoader::LoadLibraryW};

type Install = unsafe extern "C" fn(*const rsf_census_options) -> i32;
type Shutdown = unsafe extern "C" fn() -> i32;

fn check_creation() -> Option<bool> {
    let path = std::env::current_exe()
        .ok()?
        .with_file_name("rsf_fixture_creation.dll");
    let path = wide_os(path);
    let module = unsafe { LoadLibraryW(path.as_ptr()) };
    if module.is_null() {
        return None;
    }
    let identity = named(module, c"fixture_creation_identity")?;
    let raw = named(module, c"fixture_creation_raw")?;
    let identity = unsafe {
        transmute::<unsafe extern "system" fn() -> isize, unsafe extern "C" fn() -> u32>(identity)
    };
    let raw = unsafe {
        transmute::<unsafe extern "system" fn() -> isize, unsafe extern "C" fn(u32) -> *mut c_void>(
            raw,
        )
    };
    let before_post_load = unsafe { identity() } == 15;
    let after_post_load = [
        c"CreateDXGIFactory",
        c"CreateDXGIFactory1",
        c"CreateDXGIFactory2",
        c"D3D12CreateDevice",
    ]
    .into_iter()
    .enumerate()
    .all(|(index, name)| {
        let resolved: FARPROC = named(module, name);
        resolved.is_some_and(|entry| entry as *mut c_void == unsafe { raw(index as u32) })
    });
    Some(before_post_load && after_post_load)
}

pub(super) unsafe fn rsf_test_census_creation(
    directory: *const c_char,
    install: Install,
    shutdown: Shutdown,
) -> i32 {
    let options = rsf_census_options {
        struct_size: size_of::<rsf_census_options>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        output_dir_utf8: directory,
        flags: RSF_CENSUS_FLAG_NO_STATIC_SCAN,
        ..Default::default()
    };
    if unsafe { install(&options) } != 0 {
        eprintln!("The census must install before loading the creation fixture.");
        return 1;
    }
    let mut failures = 0;
    if check_creation() != Some(true) {
        eprintln!(
            "The creation resolver must preserve all four pointers before and after post-load."
        );
        failures += 1;
    }
    if unsafe { shutdown() } != 0 {
        eprintln!("The census must shut down after the creation fixture.");
        failures += 1;
    }
    println!("vendor_census creation_identity: {failures} failure(s)");
    i32::from(failures != 0)
}
