// SPDX-License-Identifier: MIT
//! The Wingman XInput carrier's optional, process-lived plugin session.
use crate::{AMD64, EXECUTABLE};
use rsf_game_sys::plugin_session_abi::rsf_plugin_session_options;
use rsf_game_sys::session::{Probe, host_services, with_session_options};
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
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

struct Session {
    handle: std::ptr::NonNull<c_void>,
    quiesce: Start,
    stop: Start,
    active: bool,
}

// Access is serialized through SESSION. Runtime operations use an opaque session handle.
unsafe impl Send for Session {}

fn retire(session: &mut Option<Session>) -> i32 {
    let Some(pending) = session.as_ref() else {
        return 0;
    };
    unsafe { (pending.quiesce)(pending.handle.as_ptr()) };
    let result = unsafe { (pending.stop)(pending.handle.as_ptr()) };
    if result == 0 {
        *session = None;
    }
    result
}

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

    let mut owner = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    if owner.as_ref().is_some_and(|session| session.active) {
        return 0;
    }
    let cleanup = retire(&mut owner);
    if cleanup != 0 {
        return cleanup as u32;
    }

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
    let quiesce = unsafe { GetProcAddress(runtime, s!("rsf_plugin_session_quiesce")) };
    let stop = unsafe { GetProcAddress(runtime, s!("rsf_plugin_session_stop")) };
    let (Some(prepare), Some(start), Some(quiesce), Some(stop)) = (prepare, start, quiesce, stop)
    else {
        write_log(b"Runtime plugin lifecycle exports are unavailable.");
        return 1;
    };
    // These names are the Runtime's public C session entry points.
    let prepare: Prepare = unsafe { std::mem::transmute(prepare) };
    let start: Start = unsafe { std::mem::transmute(start) };
    let quiesce: Start = unsafe { std::mem::transmute(quiesce) };
    let stop: Start = unsafe { std::mem::transmute(stop) };

    let Some(mut executable) = module_path(null_mut()) else {
        return 1;
    };
    executable.push(0);
    let mut digest = [0 as c_char; 65];
    if !unsafe { fingerprint(executable.as_ptr(), digest.as_mut_ptr()) } {
        write_log(b"Executable fingerprint could not be read.");
        return 1;
    }
    let probe = Probe {
        machine: AMD64,
        executable: EXECUTABLE,
        sha256: unsafe { CStr::from_ptr(digest.as_ptr()) },
    };
    let plugin_path = path_with(parent, "ReScaleFrame\\ReScaleFrame.Game.ProjectWingman.dll");
    let services = host_services(unsafe { GetTickCount64() }, Some(host_log));
    let mut session = null_mut();
    let prepared = with_session_options(&plugin_path, probe, &services, |options| unsafe {
        prepare(options, &mut session)
    }) as u32;
    write_log(format!("Project Wingman observer prepare result={prepared}").as_bytes());
    if let Some(handle) = std::ptr::NonNull::new(session) {
        *owner = Some(Session {
            handle,
            quiesce,
            stop,
            active: false,
        });
    }
    if prepared != 0 {
        let cleanup = retire(&mut owner);
        if cleanup != 0 {
            write_log(
                format!("Project Wingman observer cleanup pending result={cleanup}").as_bytes(),
            );
        }
        return prepared;
    }
    let Some(admitted) = owner.as_mut() else {
        return 1;
    };
    let started = unsafe { start(session) } as u32;
    admitted.active = started == 0;
    write_log(format!("Project Wingman observer start result={started}; graph facts only, SR/FG/latency not implemented.").as_bytes());
    if started != 0 {
        let cleanup = retire(&mut owner);
        if cleanup != 0 {
            write_log(
                format!("Project Wingman observer cleanup pending result={cleanup}").as_bytes(),
            );
        }
    }
    // The loaded Runtime and admitted session stay valid until process exit.
    started
}
