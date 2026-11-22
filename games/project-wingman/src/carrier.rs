// SPDX-License-Identifier: MIT
//! The Wingman XInput carrier's optional, process-lived plugin session.
use crate::windows::{fingerprint, matches_executable, module_path};
use crate::{AMD64, EXECUTABLE, SHA256, overlay};
use rsf_game_sys::plugin_session_abi::rsf_plugin_session_options;
use rsf_game_sys::session::carrier_log::CarrierLog;
use rsf_game_sys::session::{Probe, host_services, with_session_options};
use std::ffi::{CStr, OsString, c_char, c_void};
use std::ptr::null_mut;
use std::sync::{Mutex, PoisonError};
use windows_sys::Win32::Foundation::{CloseHandle, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    DisableThreadLibraryCalls, GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::CreateThread;
use windows_sys::core::s;

type Prepare = unsafe extern "C" fn(*const rsf_plugin_session_options, *mut *mut c_void) -> i32;
type Start = unsafe extern "C" fn(*mut c_void) -> i32;

static LOG: CarrierLog = CarrierLog::new();
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

fn path_with(prefix: &[u16], suffix: &str) -> Vec<u16> {
    prefix
        .iter()
        .copied()
        .chain(suffix.encode_utf16())
        .chain([0])
        .collect()
}

pub(crate) fn write_log(message: &[u8]) {
    LOG.write(unsafe { GetTickCount64() }, message);
}

pub(crate) unsafe extern "C" fn host_log(_: *mut c_void, message: *const c_char) {
    unsafe { LOG.write_c_message(GetTickCount64(), message) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_wingman_carrier_attach(module: HMODULE) {
    unsafe { DisableThreadLibraryCalls(module) };
    let mut executable = [0; 32768];
    if module_path(null_mut(), &mut executable).is_some_and(matches_executable) {
        // The worker's entry runs outside loader lock. DllMain never waits for it.
        let worker =
            unsafe { CreateThread(std::ptr::null(), 0, Some(startup), module, 0, null_mut()) };
        if !worker.is_null() {
            unsafe { CloseHandle(worker) };
        }
    }
}

unsafe extern "system" fn startup(module: *mut c_void) -> u32 {
    let mut path_buffer = vec![0; 32768];
    let Some(module_path_utf16) = module_path(module, &mut path_buffer) else {
        return 1;
    };
    let parent_end = module_path_utf16
        .iter()
        .rposition(|&unit| unit == 0x5c || unit == 0x2f)
        .map_or(0, |index| index + 1);
    let parent = &module_path_utf16[..parent_end];
    let config = path_with(parent, "ReScaleFrame.ini");
    let log_path = path_with(parent, "ReScaleFrame\\project-wingman.log");
    LOG.set_path(&log_path);
    write_log(b"XInput carrier started; Windows controller forwarding remains independent.");

    let mut executable = vec![0; 32768];
    let Some(digest) = module_path(null_mut(), &mut executable).and_then(fingerprint) else {
        write_log(b"Executable fingerprint could not be read.");
        return 1;
    };
    let digest = CStr::from_bytes_with_nul(&digest).expect("hex digest is NUL terminated");
    if digest != SHA256 {
        write_log(b"Executable fingerprint differs from the supported Wingman build.");
        return 1;
    }

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
    let result = match overlay::read(runtime, config) {
        Ok(true) => start_session(runtime, parent, digest, &mut owner),
        Ok(false) => 0,
        Err(result) => {
            write_log(
                format!("Shared observer setting could not be read, result={result}.").as_bytes(),
            );
            return result as u32;
        }
    };
    overlay::start(runtime);
    result
}

fn start_session(
    runtime: HMODULE,
    parent: &[u16],
    digest: &CStr,
    owner: &mut Option<Session>,
) -> u32 {
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

    let probe = Probe {
        machine: AMD64,
        executable: EXECUTABLE,
        sha256: digest,
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
        let cleanup = retire(owner);
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
        let cleanup = retire(owner);
        if cleanup != 0 {
            write_log(
                format!("Project Wingman observer cleanup pending result={cleanup}").as_bytes(),
            );
        }
    }
    // The loaded Runtime and admitted session stay valid until process exit.
    started
}
