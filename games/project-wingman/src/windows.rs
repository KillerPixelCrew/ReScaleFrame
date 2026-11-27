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

/// Whether this process runs the game's executable, by name; the build is discovered.
pub(crate) fn is_game_process() -> bool {
    unsafe { module_path(null_mut()) }.is_some_and(|path| leaf_is(path.units(), crate::EXECUTABLE))
}

/// The running executable's SHA-256 and PE timestamp, logged as evidence and never compared.
pub(crate) fn executable_evidence() -> String {
    let digest = unsafe { module_path(null_mut()) }
        .and_then(|path| {
            rsf_windows::sha256_file_hex(&PathBuf::from(OsString::from_wide(path.units())))
        })
        .and_then(|digest| String::from_utf8(digest[..64].to_vec()).ok())
        .unwrap_or_else(|| "unavailable".into());
    let base = image_base();
    let field = |offset: usize| {
        let mut bytes = [0; 4];
        rsf_game_sys::win32::read_bytes(base.wrapping_add(offset), &mut bytes)
            .then(|| u32::from_le_bytes(bytes) as usize)
    };
    let timestamp = field(0x3c).and_then(|nt| field(nt + 8)).unwrap_or(0);
    format!("executable sha256 {digest}, PE timestamp 0x{timestamp:x}")
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
