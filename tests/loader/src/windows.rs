// SPDX-License-Identifier: MIT
mod fixture;
mod forwarding;
mod thin;
#[cfg(target_env = "msvc")]
mod vendor_census_asi;
#[cfg(target_env = "msvc")]
mod vendor_census_asi_abi;

use std::ffi::{CStr, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{FARPROC, GetLastError, HMODULE, MAX_PATH};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

fn load(path: impl AsRef<OsStr>, flags: u32) -> HMODULE {
    unsafe { LoadLibraryExW(wide(path).as_ptr(), null_mut(), flags) }
}

fn system_path(name: &str) -> Vec<u16> {
    let mut buffer = [0; MAX_PATH as usize];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), MAX_PATH) } as usize;
    require(length != 0 && length < buffer.len(), "get system directory");
    let mut result = buffer[..length].to_vec();
    result.push(u16::from(b'\\'));
    result.extend(name.encode_utf16().chain([0]));
    result
}

fn named(module: HMODULE, name: &CStr) -> FARPROC {
    unsafe { GetProcAddress(module, name.as_ptr().cast()) }
}

fn ordinal(module: HMODULE, number: u16) -> FARPROC {
    unsafe { GetProcAddress(module, usize::from(number) as *const u8) }
}

fn address(module: HMODULE, name: &CStr) -> unsafe extern "system" fn() -> isize {
    named(module, name).unwrap_or_else(|| fail(&format!("resolve {}", name.to_string_lossy())))
}

fn fail(message: &str) -> ! {
    eprintln!("FAIL: {message} (Win32={})", unsafe { GetLastError() });
    std::process::exit(1);
}

fn require(condition: bool, message: &str) {
    if !condition {
        fail(message);
    }
}
