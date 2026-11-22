// SPDX-License-Identifier: MIT
mod config;
mod forwarding;
mod plugin;
mod shim;
mod thin;

use std::ffi::{CStr, OsStr, OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, HMODULE, MAX_PATH};
use windows_sys::Win32::System::LibraryLoader::{
    DisableThreadLibraryCalls, GetModuleFileNameW, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{CreateThread, LPTHREAD_START_ROUTINE};

const PATH_CAPACITY: usize = 32768;

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

fn module_path(module: HMODULE) -> Option<PathBuf> {
    let mut units = vec![0; PATH_CAPACITY];
    let count = unsafe { GetModuleFileNameW(module, units.as_mut_ptr(), units.len() as u32) };
    (count != 0 && (count as usize) < units.len())
        .then(|| PathBuf::from(OsString::from_wide(&units[..count as usize])))
}

fn load_system(name: &str, load: impl FnOnce(*const u16, u32) -> HMODULE) -> HMODULE {
    let mut buffer = [0; MAX_PATH as usize];
    let count = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), MAX_PATH) } as usize;
    if count == 0 || count >= buffer.len() {
        return null_mut();
    }
    // A bare DLL name can resolve back to the carrier.
    let mut path = buffer[..count].to_vec();
    path.push(u16::from(b'\\'));
    path.extend(name.encode_utf16().chain([0]));
    load(path.as_ptr(), LOAD_LIBRARY_SEARCH_SYSTEM32)
}

fn load_library(path: *const u16, flags: u32) -> HMODULE {
    unsafe { LoadLibraryExW(path, null_mut(), flags) }
}

fn spawn_startup(module: HMODULE, startup: LPTHREAD_START_ROUTINE) {
    // The loader lock delays the worker until process attach has returned.
    unsafe {
        DisableThreadLibraryCalls(module);
        let worker = CreateThread(null(), 0, startup, module.cast::<c_void>(), 0, null_mut());
        if !worker.is_null() {
            CloseHandle(worker);
        }
    }
}

type Entry = unsafe extern "system" fn(*const u16) -> u32;

fn entry(module: HMODULE, name: &CStr) -> Option<Entry> {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    let address = unsafe { GetProcAddress(module, name.as_ptr().cast()) }?;
    Some(unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Entry>(address) })
}
