// SPDX-License-Identifier: MIT
//! Process identity, guarded native reads, MinHook and the native exception leaves.
use rsf_game_sys::win32::{module_path, pin_module};
use rsf_game_sys::windows_path::leaf_is;
use rsf_game_sys::{rsf_game_host_services, rsf_game_render_pass};
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

// The native leaves contain every engine fault or foreign C++ exception (native_platform.c).
unsafe extern "C" {
    /// The D3D11 texture behind an RDG texture, referenced, or null.
    pub(crate) fn rsf_wingman_acquire_texture(rdg: usize) -> *mut c_void;
    pub(crate) fn rsf_wingman_release_texture(texture: *mut c_void);
    pub(crate) fn rsf_wingman_report_execution(
        host: *const rsf_game_host_services,
        list: usize,
        pass: *const rsf_game_render_pass,
        log: bool,
    );
}

/// Also the native texture reader's copy.
#[unsafe(export_name = "rsf_wingman_copy")]
pub(crate) unsafe extern "C" fn copy(
    output: *mut c_void,
    input: *const c_void,
    bytes: usize,
) -> bool {
    unsafe { rsf_game_sys::win32::read_process(input as usize, output, bytes) }
}

pub(crate) fn verify_executable() -> bool {
    unsafe { module_path(null_mut()) }
        .filter(|path| leaf_is(path.units(), crate::EXECUTABLE))
        .and_then(|path| {
            rsf_windows::sha256_file_hex(&PathBuf::from(OsString::from_wide(path.units())))
        })
        .is_some_and(|digest| &digest[..64] == crate::SHA256.to_bytes())
}

pub(crate) fn image_base() -> usize {
    unsafe { GetModuleHandleW(null()) as usize }
}

pub(crate) fn tick_count() -> u64 {
    unsafe { GetTickCount64() }
}

/// Creates a hook whose trampoline stays reachable during native unwinding and after disable:
/// the module holding `detour` is pinned first.
pub(crate) fn create_hook(target: usize, detour: usize, original: &mut usize) -> bool {
    let mut trampoline = null_mut();
    let created = rsf_windows::minhook::initialize()
        && pin_module(detour as *const c_void).is_some()
        && unsafe {
            rsf_windows::minhook::create(
                target as *mut c_void,
                detour as *mut c_void,
                &mut trampoline,
            )
        };
    if created {
        *original = trampoline as usize;
    }
    created
}

pub(crate) unsafe fn enable_hook(target: usize) -> bool {
    unsafe { rsf_windows::minhook::enable_status(target as *mut c_void) == 0 }
}

pub(crate) unsafe fn disable_hook(target: usize) -> bool {
    unsafe { rsf_windows::minhook::disable(target as *mut c_void) }
}
