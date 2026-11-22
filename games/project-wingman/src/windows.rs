// SPDX-License-Identifier: MIT
//! Process identity, guarded native reads, MinHook and the native exception leaves.
use rsf_game_sys::win32::module_path;
use rsf_game_sys::{rsf_game_host_services, rsf_game_render_pass};
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    GetModuleHandleW,
};
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

fn matches_executable(path: &[u16]) -> bool {
    let leaf = path
        .rsplit(|&unit| matches!(unit, 0x5c | 0x2f))
        .next()
        .unwrap_or_default();
    let expected = crate::EXECUTABLE.to_bytes();
    leaf.len() == expected.len()
        && leaf.iter().zip(expected).all(|(&value, expected)| {
            u8::try_from(value).is_ok_and(|value| value.eq_ignore_ascii_case(expected))
        })
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
        .filter(|path| matches_executable(path.units()))
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
    let mut pinned = null_mut();
    let mut trampoline = null_mut();
    let created = rsf_windows::minhook::initialize()
        && unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                detour as *const u16,
                &mut pinned,
            )
        } != 0
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
