// SPDX-License-Identifier: MIT
//! Anomaly's synchronous cold entry. The thin carrier calls `rsf_anomaly_start` with the absolute adapter INI before
//! genuine DirectInput creation. This module selects the exact executable build, borrows the Runtime DLL the carrier
//! already loaded and starts Observe through the Runtime's stable plugin-session C ABI. This module owns the step
//! order, refusals, retry state and adapter log. The stage calls are bootstrap/native.rs's (the tests' fakes under test).

#[cfg(feature = "native-entry")]
mod native;
#[cfg(not(test))]
use native as leaves;
#[cfg(test)]
use tests as leaves;

use crate::AMD64;
use crate::executable::with_verified_build;
use crate::native_profile::PROFILES;
#[cfg(test)]
use rsf_game_sys::RSF_GAME_ABI_VERSION;
#[cfg(test)]
use rsf_game_sys::RSF_PLUGIN_SESSION_ABI_VERSION as SESSION_ABI_VERSION;
use rsf_game_sys::rsf_game_log_fn;
use rsf_game_sys::rsf_plugin_session_options as SessionOptions;
use rsf_game_sys::session::{Probe, host_services, with_session_options};
use rsf_game_sys::win32::module_path;
use std::ffi::{CStr, c_char, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::ptr;
use crate::lock;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleHandleExW, GetModuleHandleW,
};
use windows_sys::Win32::System::Performance::QueryPerformanceCounter;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

// rsf_anomaly_start status values. The carrier logs them; they are unchanged from the C++ entry.
const READY: u32 = 0;
const RUNTIME_REFUSED: u32 = 4;
const PREPARE_REFUSED: u32 = 5;
const START_REFUSED: u32 = 6;
const OVERLAY_REFUSED: u32 = 9;

const RSF_OK: i32 = 0;

type PrepareFn = unsafe extern "C" fn(*const SessionOptions, *mut *mut c_void) -> i32;
type ControlFn = unsafe extern "C" fn(*mut c_void) -> i32;

/// The carrier-loaded Runtime, borrowed without a reference.
struct Runtime {
    module: HMODULE,
    prepare: Option<PrepareFn>,
    start: Option<ControlFn>,
    quiesce: Option<ControlFn>,
    stop: Option<ControlFn>,
}

/// One accepted call: the NUL-terminated INI, the selected build and the requested modes.
struct Request<'a> {
    ini: &'a [u16],
    build: usize,
    observe: bool,
    overlay: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Observation {
    Unprepared,
    RetainedAfterRefusal,
    Prepared,
    Active,
}

struct Entry {
    started: bool,            // every requested mode is complete
    options_registered: bool, // restart options are in the overlay list once
    stages_configured: bool,  // Observe stages were configured and the FG bridge was attempted
    input: InputInstallation,
    observation: Observation,
    session: *mut c_void, // the Runtime's session, including one retained after refused prepare
}

// SAFETY: the session handle is only used while ENTRY is locked.
unsafe impl Send for Entry {}

impl Entry {
    const fn new() -> Self {
        Self {
            started: false,
            options_registered: false,
            stages_configured: false,
            input: InputInstallation::Pending,
            observation: Observation::Unprepared,
            session: ptr::null_mut(),
        }
    }
}

static LOG: Mutex<Option<PathBuf>> = Mutex::new(None);


fn write_log(message: &[u8]) {
    let path = lock(&LOG);
    let Some(path) = path.as_deref() else { return };
    let mut line = Vec::with_capacity(message.len() + 32);
    let _ = write!(line, "[{}:{}] ", unsafe { GetCurrentProcessId() }, unsafe {
        GetTickCount64()
    });
    line.extend_from_slice(message);
    line.push(b'\n');
    if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) {
        let _ = file.write_all(&line);
    }
}

/// The adapter log, handed to the Runtime session and the C++ stages.
unsafe extern "C" fn adapter_log(_: *mut c_void, message: *const c_char) {
    if !message.is_null() {
        write_log(unsafe { CStr::from_ptr(message) }.to_bytes());
    }
}

const LOG_FN: rsf_game_log_fn = Some(adapter_log);

fn log_result(operation: &str, result: i32) {
    write_log(format!("Anomaly {operation} result={result}").as_bytes());
}

/// The cold-start order: restart options, SR, motion, the FG bridge and input facts before the engine creates its
/// device, then the Observe session, then the overlay. A retry repeats only the steps that have not completed.
fn orchestrate(entry: &mut Entry, request: &Request, runtime: &Runtime) -> u32 {
    let ini = request.ini.as_ptr();
    if request.observe
        && entry.observation == Observation::RetainedAfterRefusal
        && !retire_partial_observation(entry, runtime)
    {
        return PREPARE_REFUSED;
    }
    if !entry.options_registered {
        leaves::register_options(request.ini);
        entry.options_registered = true;
    }
    if !entry.stages_configured {
        // SR Stage 1 rides on Observe: the observer's start installs it after the geometry hooks.
        if unsafe { leaves::sr(ini, request.observe, LOG_FN) } && !request.observe {
            write_log(b"Anomaly SR stage refused: SRStage/SRStageCapture require Mode=Observe; game left stock.");
        }
        // Observe records motion variants even when disabled, so the pack can turn on live later.
        if request.observe {
            unsafe { leaves::motion(ini, LOG_FN) };
        }
        // FG also rides on Observe. Its presentation bridge must intercept the engine's swap-chain creation, so it
        // installs here, before the device exists. A refusal leaves presentation stock.
        let fg = unsafe { leaves::fg(ini, request.observe, runtime.module, request.build, LOG_FN) };
        if fg && !request.observe {
            write_log(
                b"Anomaly FG refused: FG/FGBridge require Mode=Observe; presentation stays stock.",
            );
        }
        entry.stages_configured = request.observe;
    }
    if request.observe && leaves::fg_installed() {
        entry.input = install_input(entry.input, request.build);
    }
    if request.observe && entry.observation != Observation::Active {
        let status = start_observation(entry, request, runtime);
        if status != READY {
            return status;
        }
    }
    if request.overlay {
        if !unsafe { leaves::overlay(runtime.module, ini, LOG_FN) } {
            return OVERLAY_REFUSED;
        }
        // With the bridge presenting, the engine's Present never reaches the DXGI hooks; the panel draws from the
        // bridge's before_present into the engine-facing back buffer instead.
        if leaves::fg_installed() {
            unsafe { leaves::overlay_sink() };
        }
    }
    entry.started = true;
    READY
}

#[derive(Clone, Copy)]
enum InputInstallation {
    Pending,
    DirectInput,
    Complete,
}

/// The engine input hook installs only after DirectInput observation, so its facts always have their receipts.
fn install_input(mut state: InputInstallation, build: usize) -> InputInstallation {
    if matches!(state, InputInstallation::Pending) {
        if let Err(reason) = leaves::input_observation() {
            write_log(reason.to_bytes());
            return state;
        }
        unsafe { leaves::publish_input_observer() };
        state = InputInstallation::DirectInput;
    }
    if matches!(state, InputInstallation::DirectInput) {
        let mut reason = ptr::null();
        let image = unsafe { GetModuleHandleW(ptr::null()) };
        if !unsafe { leaves::input_hook(image, build, &mut reason) } {
            log_input_refusal(reason, b"Anomaly input hook refused.");
            return state;
        }
        write_log(b"Anomaly input facts: guarded CInput slot 0 installed; buffered keyboard/mouse reads observed.");
        state = InputInstallation::Complete;
    }
    state
}

fn log_input_refusal(reason: *const c_char, fallback: &[u8]) {
    // A refusing leaf reports a static site or reason string.
    if reason.is_null() {
        write_log(fallback);
    } else {
        write_log(unsafe { CStr::from_ptr(reason) }.to_bytes());
    }
}

fn retire_partial_observation(entry: &mut Entry, runtime: &Runtime) -> bool {
    let (Some(quiesce), Some(stop)) = (runtime.quiesce, runtime.stop) else {
        write_log(b"Anomaly refused: Runtime plugin-session cleanup exports missing.");
        return false;
    };
    let result = unsafe { quiesce(entry.session) };
    log_result("partial session quiesce", result);
    if result != RSF_OK {
        return false;
    }
    let result = unsafe { stop(entry.session) };
    log_result("partial session stop", result);
    if result != RSF_OK {
        return false;
    }
    entry.session = ptr::null_mut();
    entry.observation = Observation::Unprepared;
    true
}

fn start_observation(entry: &mut Entry, request: &Request, runtime: &Runtime) -> u32 {
    let (Some(prepare), Some(start)) = (runtime.prepare, runtime.start) else {
        write_log(b"Anomaly refused: required Runtime plugin-session exports missing.");
        return RUNTIME_REFUSED;
    };
    if entry.observation == Observation::Unprepared {
        let status = prepare_observation(entry, request, prepare);
        if status != READY {
            return status;
        }
    }
    let activated = unsafe { start(entry.session) };
    log_result("session start", activated);
    if activated != RSF_OK {
        return START_REFUSED;
    }
    entry.observation = Observation::Active;
    write_log(
        b"Anomaly Observe active. This is native-boundary instrumentation, not working SR or FG.",
    );
    READY
}

fn prepare_observation(entry: &mut Entry, request: &Request, prepare: PrepareFn) -> u32 {
    static ANCHOR: u8 = 0;
    let mut module: HMODULE = ptr::null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    if unsafe { GetModuleHandleExW(flags, ptr::addr_of!(ANCHOR).cast(), &mut module) } == 0 {
        return RUNTIME_REFUSED;
    }
    let Some(plugin) = (unsafe { module_path(module) }) else {
        return RUNTIME_REFUSED;
    };
    let mut counter = 0i64;
    if unsafe { QueryPerformanceCounter(&mut counter) } == 0 || counter <= 0 {
        return RUNTIME_REFUSED;
    }
    let profile = &PROFILES[request.build];
    let probe = Probe {
        machine: AMD64,
        executable: profile.executable,
        sha256: profile.sha256,
    };
    let mut services = host_services(counter as u64, LOG_FN);
    services.session_id = counter as u64;
    write_log(b"Anomaly Observe cold startup; borrowing existing Runtime, before DirectInput/device creation.");
    let prepared = with_session_options(plugin.terminated(), probe, &services, |options| {
        with_verified_build(request.build, || unsafe {
            prepare(options, &mut entry.session)
        })
    });
    log_result("session prepare", prepared);
    if prepared != RSF_OK {
        if !entry.session.is_null() {
            entry.observation = Observation::RetainedAfterRefusal;
        }
        return PREPARE_REFUSED;
    }
    if entry.session.is_null() {
        write_log(b"Anomaly refused: Runtime returned no prepared session.");
        return PREPARE_REFUSED;
    }
    entry.observation = Observation::Prepared;
    READY
}

#[cfg(test)]
mod tests;
