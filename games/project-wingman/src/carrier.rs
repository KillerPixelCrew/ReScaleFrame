// SPDX-License-Identifier: MIT
//! The Wingman XInput carrier's optional, process-lived plugin session.
use crate::{AMD64, EXECUTABLE};
use rsf_game_sys::plugin_session_abi::{
    RSF_PLUGIN_SESSION_ABI_VERSION, rsf_plugin_session_options,
};
use rsf_game_sys::{RSF_GAME_ABI_VERSION, rsf_game_host_services, rsf_game_probe};
use std::ffi::{CStr, OsString, c_char, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock, PoisonError};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::WindowsProgramming::GetPrivateProfileIntW;
use windows_sys::core::{s, w};

type Prepare = unsafe extern "C" fn(*const rsf_plugin_session_options, *mut *mut c_void) -> i32;
type Start = unsafe extern "C" fn(*mut c_void) -> i32;
type Fingerprint = unsafe extern "C" fn(*const u16, *mut c_char) -> bool;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_WRITE: Mutex<()> = Mutex::new(());

fn module_path(module: HMODULE) -> Option<Vec<u16>> {
    let mut buffer = vec![0; 32768];
    let len =
        unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len == 0 || len >= buffer.len() {
        return None;
    }
    buffer.truncate(len);
    Some(buffer)
}

fn path_with(prefix: &[u16], suffix: &str) -> Vec<u16> {
    prefix
        .iter()
        .copied()
        .chain(suffix.encode_utf16())
        .chain([0])
        .collect()
}

fn write_log(message: &[u8]) {
    let Some(path) = LOG_PATH.get() else { return };
    let _writer = LOG_WRITE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let tick = unsafe { GetTickCount64() };
        let _ = write!(file, "[{tick}] ");
        let _ = file.write_all(message);
        let _ = file.write_all(b"\n");
    }
}

unsafe extern "C" fn host_log(_: *mut c_void, message: *const c_char) {
    if !message.is_null() {
        write_log(unsafe { CStr::from_ptr(message) }.to_bytes());
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_wingman_carrier_start(module: HMODULE, fingerprint: Fingerprint) -> u32 {
    let Some(module_path_utf16) = module_path(module) else {
        return 1;
    };
    let parent_end = module_path_utf16
        .iter()
        .rposition(|&unit| unit == 0x5c || unit == 0x2f)
        .map_or(0, |index| index + 1);
    let parent = &module_path_utf16[..parent_end];
    let config = path_with(parent, "ReScaleFrame.ini");
    if unsafe {
        GetPrivateProfileIntW(
            w!("ProjectWingman"),
            w!("ObserveNative"),
            0,
            config.as_ptr(),
        )
    } == 0
    {
        return 0;
    }
    let log_path = path_with(parent, "ReScaleFrame\\project-wingman.log");
    let _ = LOG_PATH.set(PathBuf::from(OsString::from_wide(
        &log_path[..log_path.len() - 1],
    )));
    write_log(b"XInput carrier started; Windows controller forwarding remains independent.");

    let runtime_path = path_with(parent, "ReScaleFrame\\ReScaleFrame.Runtime.dll");
    let runtime = unsafe {
        LoadLibraryExW(
            runtime_path.as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    };
    if runtime.is_null() {
        write_log(b"Runtime DLL load refused.");
        return 1;
    }
    let prepare = unsafe { GetProcAddress(runtime, s!("rsf_plugin_session_prepare")) };
    let start = unsafe { GetProcAddress(runtime, s!("rsf_plugin_session_start")) };
    let (Some(prepare), Some(start)) = (prepare, start) else {
        write_log(b"Runtime plugin lifecycle exports are unavailable.");
        return 1;
    };
    // Both names are the Runtime's public C session entry points.
    let prepare: Prepare = unsafe { std::mem::transmute(prepare) };
    let start: Start = unsafe { std::mem::transmute(start) };

    let Some(mut executable) = module_path(null_mut()) else {
        return 1;
    };
    executable.push(0);
    let mut digest = [0 as c_char; 65];
    if !unsafe { fingerprint(executable.as_ptr(), digest.as_mut_ptr()) } {
        write_log(b"Executable fingerprint could not be read.");
        return 1;
    }
    let probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: AMD64,
        reserved: 0,
        executable_name_utf8: EXECUTABLE.as_ptr(),
        sha256_hex: digest.as_ptr(),
    };
    let plugin_path = path_with(parent, "ReScaleFrame\\ReScaleFrame.Game.ProjectWingman.dll");
    // Absent services are null. The Runtime takes a copy before prepare returns.
    let mut services: rsf_game_host_services = unsafe { std::mem::zeroed() };
    services.struct_size = size_of::<rsf_game_host_services>() as u32;
    services.abi_version = RSF_GAME_ABI_VERSION;
    services.session_id = unsafe { GetTickCount64() }.wrapping_add(1);
    services.log = Some(host_log);
    let options = rsf_plugin_session_options {
        struct_size: size_of::<rsf_plugin_session_options>() as u32,
        abi_version: RSF_PLUGIN_SESSION_ABI_VERSION,
        plugin_path: plugin_path.as_ptr(),
        probe: &probe,
        services,
    };
    let mut session = null_mut();
    let prepared = unsafe { prepare(&options, &mut session) } as u32;
    write_log(format!("Project Wingman observer prepare result={prepared}").as_bytes());
    if prepared != 0 {
        return prepared;
    }
    let started = unsafe { start(session) } as u32;
    write_log(format!("Project Wingman observer start result={started}; graph facts only, SR/FG/latency not implemented.").as_bytes());
    // The loaded Runtime and admitted session stay valid until process exit.
    started
}
